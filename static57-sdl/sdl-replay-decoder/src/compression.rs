//! Codec probing and bounded streaming inflation of the packet stream.
//!
//! The inflater works over the in-memory compressed section and records the
//! exact compressed/decompressed span of every frame (zstd) or member (zlib),
//! so all compressed bytes are accounted for: decoded, skipped, or trailing.

use crate::evidence::hex;
use crate::versions::Codec;
use ruzstd::decoding::{FrameDecoder, StreamingDecoder};
use serde::Serialize;
use std::io::{self, Read};

pub const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// Default ceiling on decompressed output (decompression-bomb guard).
pub const DEFAULT_MAX_OUTPUT: u64 = 4 << 30;

pub fn is_zstd_skippable(b: &[u8]) -> bool {
    b.len() >= 8 && (u32::from_le_bytes(b[0..4].try_into().unwrap()) & 0xFFFF_FFF0) == 0x184D_2A50
}

pub fn is_zlib_header(b: &[u8]) -> bool {
    b.len() >= 2 && (b[0] & 0x0F) == 8 && (b[0] >> 4) <= 7 && ((b[0] as u16) << 8 | b[1] as u16) % 31 == 0
}

/// Identify the codec from the first bytes of the stream.
pub fn probe(b: &[u8]) -> Option<Codec> {
    if b.starts_with(&ZSTD_MAGIC) || is_zstd_skippable(b) {
        Some(Codec::Zstd)
    } else if is_zlib_header(b) {
        Some(Codec::Zlib)
    } else {
        None
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameInfo {
    pub index: usize,
    /// `zstd`, `zstd_skippable` or `zlib`.
    pub kind: &'static str,
    /// Offset relative to the start of the packet-stream section.
    pub compressed_offset: u64,
    pub compressed_length: u64,
    pub decompressed_offset: u64,
    pub decompressed_length: u64,
    /// Whether the decoder reached the codec's own end-of-frame marker.
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamReport {
    pub codec: Option<Codec>,
    pub compressed_length: u64,
    pub compressed_consumed: u64,
    pub trailing_bytes: u64,
    /// First bytes of any trailing (unconsumed) compressed data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trailing_prefix_hex: Option<String>,
    pub decompressed_length: u64,
    pub output_limit: u64,
    pub output_limit_hit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub frames: Vec<FrameInfo>,
}

enum Member<'a> {
    Zstd(Box<StreamingDecoder<&'a [u8], FrameDecoder>>),
    Zlib(flate2::bufread::ZlibDecoder<&'a [u8]>),
    Done,
}

/// `Read` adapter yielding the concatenated decompressed output of every
/// frame/member in the section, in order.
pub struct Inflater<'a> {
    data: &'a [u8],
    codec: Option<Codec>,
    member: Member<'a>,
    member_start: usize,
    member_out_start: u64,
    out_total: u64,
    limit: u64,
    limit_hit: bool,
    error: Option<String>,
    frames: Vec<FrameInfo>,
    trailing_at: Option<usize>,
}

impl<'a> Inflater<'a> {
    pub fn new(data: &'a [u8], limit: u64) -> Self {
        let mut inf = Inflater {
            data,
            codec: probe(data),
            member: Member::Done,
            member_start: 0,
            member_out_start: 0,
            out_total: 0,
            limit,
            limit_hit: false,
            error: None,
            frames: Vec::new(),
            trailing_at: None,
        };
        inf.start_member(0);
        inf
    }

    /// Begin the next frame/member at `pos`, or mark the rest as trailing.
    fn start_member(&mut self, mut pos: usize) {
        loop {
            let rest = &self.data[pos..];
            if rest.is_empty() {
                self.member = Member::Done;
                return;
            }
            match self.codec {
                Some(Codec::Zstd) if is_zstd_skippable(rest) => {
                    let len = u32::from_le_bytes(rest[4..8].try_into().unwrap()) as usize;
                    let total = (8 + len).min(rest.len());
                    self.frames.push(FrameInfo {
                        index: self.frames.len(),
                        kind: "zstd_skippable",
                        compressed_offset: pos as u64,
                        compressed_length: total as u64,
                        decompressed_offset: self.out_total,
                        decompressed_length: 0,
                        complete: total == 8 + len,
                    });
                    pos += total;
                }
                Some(Codec::Zstd) if rest.starts_with(&ZSTD_MAGIC) => {
                    match StreamingDecoder::new(rest) {
                        Ok(d) => {
                            self.member = Member::Zstd(Box::new(d));
                            break;
                        }
                        Err(e) => {
                            self.error = Some(format!("zstd frame header at stream offset {pos}: {e}"));
                            self.trailing_at = Some(pos);
                            self.member = Member::Done;
                            return;
                        }
                    }
                }
                Some(Codec::Zlib) if is_zlib_header(rest) => {
                    self.member = Member::Zlib(flate2::bufread::ZlibDecoder::new(rest));
                    break;
                }
                _ => {
                    self.trailing_at = Some(pos);
                    self.member = Member::Done;
                    return;
                }
            }
        }
        self.member_start = pos;
        self.member_out_start = self.out_total;
    }

    fn remaining_input(&self) -> usize {
        match &self.member {
            Member::Zstd(d) => d.get_ref().len(),
            Member::Zlib(d) => d.get_ref().len(),
            Member::Done => 0,
        }
    }

    fn finish_member(&mut self, complete: bool) {
        let end = self.data.len() - self.remaining_input();
        let kind = match self.member {
            Member::Zstd(_) => "zstd",
            Member::Zlib(_) => "zlib",
            Member::Done => return,
        };
        self.frames.push(FrameInfo {
            index: self.frames.len(),
            kind,
            compressed_offset: self.member_start as u64,
            compressed_length: (end - self.member_start) as u64,
            decompressed_offset: self.member_out_start,
            decompressed_length: self.out_total - self.member_out_start,
            complete,
        });
        self.member = Member::Done;
        if complete {
            self.start_member(end);
        } else {
            self.trailing_at = Some(end);
        }
    }

    pub fn report(&self) -> StreamReport {
        let consumed: u64 = self.frames.iter().map(|f| f.compressed_length).sum();
        let trailing_at = self.trailing_at.unwrap_or(self.data.len());
        let trailing = &self.data[trailing_at.min(self.data.len())..];
        StreamReport {
            codec: self.codec,
            compressed_length: self.data.len() as u64,
            compressed_consumed: consumed,
            trailing_bytes: trailing.len() as u64,
            trailing_prefix_hex: (!trailing.is_empty()).then(|| hex(&trailing[..trailing.len().min(32)])),
            decompressed_length: self.out_total,
            output_limit: self.limit,
            output_limit_hit: self.limit_hit,
            error: self.error.clone(),
            frames: self.frames.clone(),
        }
    }
}

impl Read for Inflater<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.out_total >= self.limit {
                if !matches!(self.member, Member::Done) {
                    self.limit_hit = true;
                    self.error.get_or_insert_with(|| format!("decompressed output exceeded limit of {} bytes", self.limit));
                    self.finish_member(false);
                }
                return Ok(0);
            }
            let cap = buf.len().min((self.limit - self.out_total) as usize);
            let r = match &mut self.member {
                Member::Zstd(d) => d.read(&mut buf[..cap]),
                Member::Zlib(d) => d.read(&mut buf[..cap]),
                Member::Done => return Ok(0),
            };
            match r {
                Ok(0) => self.finish_member(true),
                Ok(n) => {
                    self.out_total += n as u64;
                    return Ok(n);
                }
                Err(e) => {
                    let at = self.member_start;
                    self.error = Some(format!("decompression error in frame starting at stream offset {at}: {e}"));
                    self.finish_member(false);
                    return Err(e);
                }
            }
        }
    }
}
