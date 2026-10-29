//! Packet framing over the decompressed stream.
//!
//! Framing (REPORTED by third-party parsers, verified here by exact stream
//! accounting on real files):
//!
//! ```text
//! packet  := size_prefix body[size]
//! body    := type:u16le [timestamp_ms:u32le if !(type & 0x10)] payload
//! ```
//!
//! The size prefix is big-endian and variable length; the first byte selects
//! the form:
//!
//! | first byte  | length | value                                   |
//! |-------------|--------|-----------------------------------------|
//! | `1xxx_xxxx` | 1      | `b0 & 0x7f`                             |
//! | `01xx_xxxx` | 2      | `be16 ^ 0x4000`                         |
//! | `001x_xxxx` | 3      | `be24 ^ 0x20_0000`                      |
//! | `0001_xxxx` | 4      | `be32 ^ 0x1000_0000`                    |
//! | `0000_xxxx` | 5      | next four bytes as `u32le`              |
//!
//! Every packet is emitted, including unknown types, malformed bodies, zero
//! sizes and a truncated tail. Nothing is silently dropped.

use crate::evidence::{hex, sha256_hex, Evidence};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read};

pub const TYPE_INHERIT_TIME_FLAG: u16 = 0x10;

/// Total prefix length selected by the first byte.
pub fn prefix_len(first: u8) -> usize {
    if first & 0x80 != 0 {
        1
    } else if first & 0x40 != 0 {
        2
    } else if first & 0x20 != 0 {
        3
    } else if first & 0x10 != 0 {
        4
    } else {
        5
    }
}

/// Decode a complete size prefix (`bytes.len() == prefix_len(bytes[0])`).
pub fn decode_size_prefix(bytes: &[u8]) -> u32 {
    let be = |b: &[u8]| b.iter().fold(0u32, |acc, &x| (acc << 8) | x as u32);
    match bytes.len() {
        1 => (bytes[0] & 0x7f) as u32,
        2 => be(bytes) ^ 0x4000,
        3 => be(bytes) ^ 0x20_0000,
        4 => be(bytes) ^ 0x1000_0000,
        5 => u32::from_le_bytes(bytes[1..5].try_into().unwrap()),
        n => panic!("invalid size prefix length {n}"),
    }
}

/// Shortest encoding of `size` (inverse of [`decode_size_prefix`]).
pub fn encode_size_prefix(size: u32) -> Vec<u8> {
    if size < 0x80 {
        vec![0x80 | size as u8]
    } else if size < 0x4000 {
        (0x4000 ^ size).to_be_bytes()[2..].to_vec()
    } else if size < 0x20_0000 {
        (0x20_0000 ^ size).to_be_bytes()[1..].to_vec()
    } else if size < 0x1000_0000 {
        (0x1000_0000 ^ size).to_be_bytes().to_vec()
    } else {
        let mut v = vec![0u8];
        v.extend_from_slice(&size.to_le_bytes());
        v
    }
}

/// Published packet classes. Names are REPORTED; payloads are not decoded in
/// this milestone.
pub fn packet_kind(type_id: u16) -> Option<&'static str> {
    Some(match type_id {
        0 => "end_marker",
        1 => "start_marker",
        2 => "aircraft_small",
        3 => "chat",
        4 => "mpi",
        5 => "next_segment",
        6 => "ecs",
        7 => "snapshot",
        8 => "replay_header_info",
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Packet,
    /// A size prefix that decodes to 0 (reported as padding).
    ZeroSize,
    /// Body shorter than the type/timestamp it must contain.
    Malformed,
    /// Stream ended inside the size prefix or body.
    Truncated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimestampSource {
    Explicit,
    Inherited,
    /// Inherited before any explicit timestamp was seen; value is 0.
    InheritedInitial,
    None,
}

#[derive(Debug, Clone, Serialize)]
pub struct PacketRecord {
    pub index: u64,
    pub segment: u32,
    pub record: RecordKind,
    /// Offset of the size prefix in the decompressed stream.
    pub offset: u64,
    pub prefix_len: u8,
    pub prefix_hex: String,
    /// Body size declared by the prefix.
    pub size: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_raw: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_id: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
    pub kind_evidence: Evidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u32>,
    pub timestamp_source: TimestampSource,
    pub payload_offset: u64,
    pub payload_length: u64,
    pub payload_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_hex: Option<String>,
    /// `raw`: framed only, payload not interpreted. `error`: framing problem.
    pub decode_state: &'static str,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TypeStats {
    pub type_id: u16,
    pub kind: Option<&'static str>,
    pub evidence: Option<Evidence>,
    pub count: u64,
    pub payload_bytes: u64,
    pub explicit_timestamps: u64,
    pub inherited_timestamps: u64,
    pub min_payload: u64,
    pub max_payload: u64,
    pub first_timestamp_ms: Option<u32>,
    pub last_timestamp_ms: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ClockReport {
    pub first_timestamp_ms: Option<u32>,
    pub last_timestamp_ms: Option<u32>,
    /// DERIVED: last - first.
    pub span_ms: Option<u32>,
    pub explicit_count: u64,
    pub inherited_count: u64,
    pub inherited_before_first_explicit: u64,
    /// Explicit timestamps lower than the running time.
    pub backward_steps: u64,
    pub largest_backward_step_ms: u32,
    pub largest_forward_gap_ms: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub largest_forward_gap_at_packet: Option<u64>,
    pub monotonic: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FramingSummary {
    pub records: u64,
    pub packets: u64,
    pub recognized_packets: u64,
    pub unknown_type_packets: u64,
    pub zero_size_records: u64,
    pub malformed_records: u64,
    pub truncated: bool,
    /// Bytes in complete records (prefix + declared body).
    pub bytes_framed: u64,
    /// Bytes of an incomplete final record.
    pub truncated_tail_bytes: u64,
    pub stream_bytes: u64,
    /// True when framed + truncated-tail bytes equal the stream length and the
    /// decompressor reported no error.
    pub all_bytes_accounted: bool,
    /// Count of prefixes by length (1..=5 bytes).
    pub prefix_forms: BTreeMap<u8, u64>,
    /// Prefixes longer than the shortest encoding of their value.
    pub non_canonical_prefixes: u64,
    /// 5-byte prefixes whose first byte has non-zero low bits.
    pub five_byte_prefix_nonzero_lead: u64,
    /// Packets whose u16 type has bits set outside `0x0F | 0x10`.
    pub type_high_bits_packets: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_marker_index: Option<u64>,
    pub records_after_end_marker: u64,
    pub types: BTreeMap<u16, TypeStats>,
    pub clock: ClockReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct FrameOptions {
    pub segment: u32,
    pub include_payload_hex: bool,
}

struct Counting<R> {
    inner: BufReader<R>,
    pos: u64,
}

impl<R: Read> Counting<R> {
    /// Read up to `buf.len()` bytes; returns how many were read before EOF.
    fn fill(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut got = 0;
        while got < buf.len() {
            let avail = self.inner.fill_buf()?;
            if avail.is_empty() {
                break;
            }
            let n = avail.len().min(buf.len() - got);
            buf[got..got + n].copy_from_slice(&avail[..n]);
            self.inner.consume(n);
            got += n;
        }
        self.pos += got as u64;
        Ok(got)
    }
}

/// Frame every packet in `stream`, calling `sink` for each record with its
/// body bytes. Returns the framing summary; I/O errors from the decompressor
/// end framing and are reported, not propagated. Errors from `sink` are
/// propagated.
pub fn frame_packets<R: Read>(
    stream: R,
    opts: FrameOptions,
    mut sink: impl FnMut(&PacketRecord, &[u8]) -> io::Result<()>,
) -> io::Result<FramingSummary> {
    let mut r = Counting { inner: BufReader::with_capacity(1 << 16, stream), pos: 0 };
    let mut s = FramingSummary { clock: ClockReport { monotonic: true, ..Default::default() }, ..Default::default() };
    let mut running: Option<u32> = None;
    let mut index = 0u64;
    let mut body = Vec::new();

    loop {
        let offset = r.pos;
        let mut prefix = [0u8; 5];
        let got = match r.fill(&mut prefix[..1]) {
            Ok(n) => n,
            Err(e) => {
                s.read_error = Some(e.to_string());
                s.truncated = true;
                break;
            }
        };
        if got == 0 {
            break;
        }
        let plen = prefix_len(prefix[0]);
        let got = match r.fill(&mut prefix[1..plen]) {
            Ok(n) => n,
            Err(e) => {
                s.read_error = Some(e.to_string());
                0
            }
        };
        let mut rec = PacketRecord {
            index,
            segment: opts.segment,
            record: RecordKind::Packet,
            offset,
            prefix_len: plen as u8,
            prefix_hex: hex(&prefix[..1 + got]),
            size: 0,
            type_raw: None,
            type_id: None,
            kind: None,
            kind_evidence: Evidence::Unknown,
            timestamp_ms: None,
            timestamp_source: TimestampSource::None,
            payload_offset: r.pos,
            payload_length: 0,
            payload_sha256: sha256_hex(&[]),
            payload_hex: None,
            decode_state: "error",
        };
        if got < plen - 1 {
            rec.record = RecordKind::Truncated;
            s.truncated = true;
            emit(&mut s, &rec, &[], &mut sink)?;
            break;
        }
        let size = decode_size_prefix(&prefix[..plen]);
        rec.size = size;
        *s.prefix_forms.entry(plen as u8).or_default() += 1;
        if encode_size_prefix(size).len() != plen {
            s.non_canonical_prefixes += 1;
        }
        if plen == 5 && prefix[0] & 0x0f != 0 {
            s.five_byte_prefix_nonzero_lead += 1;
        }

        body.clear();
        body.resize(size as usize, 0);
        let got = match r.fill(&mut body) {
            Ok(n) => n,
            Err(e) => {
                s.read_error = Some(e.to_string());
                0
            }
        };
        body.truncate(got);
        let body_start = rec.payload_offset;

        if size == 0 {
            rec.record = RecordKind::ZeroSize;
            rec.decode_state = "raw";
            emit(&mut s, &rec, &[], &mut sink)?;
            index += 1;
            continue;
        }
        if got < size as usize {
            rec.record = RecordKind::Truncated;
            s.truncated = true;
            fill_payload(&mut rec, body_start, &body, opts);
            emit(&mut s, &rec, &body, &mut sink)?;
            break;
        }
        if body.len() < 2 {
            rec.record = RecordKind::Malformed;
            fill_payload(&mut rec, body_start, &body, opts);
            emit(&mut s, &rec, &body, &mut sink)?;
            index += 1;
            continue;
        }

        let type_raw = u16::from_le_bytes([body[0], body[1]]);
        let inherit = type_raw & TYPE_INHERIT_TIME_FLAG != 0;
        let type_id = type_raw & !TYPE_INHERIT_TIME_FLAG;
        rec.type_raw = Some(type_raw);
        rec.type_id = Some(type_id);
        rec.kind = packet_kind(type_id);
        rec.kind_evidence = if rec.kind.is_some() { Evidence::Reported } else { Evidence::Unknown };
        let mut header_len = 2usize;
        if inherit {
            match running {
                Some(t) => {
                    rec.timestamp_ms = Some(t);
                    rec.timestamp_source = TimestampSource::Inherited;
                }
                None => {
                    rec.timestamp_ms = Some(0);
                    rec.timestamp_source = TimestampSource::InheritedInitial;
                }
            }
        } else if body.len() < 6 {
            rec.record = RecordKind::Malformed;
        } else {
            let t = u32::from_le_bytes(body[2..6].try_into().unwrap());
            header_len = 6;
            if let Some(prev) = running {
                if t < prev {
                    s.clock.backward_steps += 1;
                    s.clock.largest_backward_step_ms = s.clock.largest_backward_step_ms.max(prev - t);
                    s.clock.monotonic = false;
                } else if t - prev > s.clock.largest_forward_gap_ms {
                    s.clock.largest_forward_gap_ms = t - prev;
                    s.clock.largest_forward_gap_at_packet = Some(index);
                }
            }
            running = Some(t);
            rec.timestamp_ms = Some(t);
            rec.timestamp_source = TimestampSource::Explicit;
        }
        let payload = &body[header_len.min(body.len())..];
        fill_payload(&mut rec, body_start + header_len as u64, payload, opts);
        if rec.record == RecordKind::Packet {
            rec.decode_state = "raw";
        }
        emit(&mut s, &rec, payload, &mut sink)?;
        index += 1;
    }
    s.stream_bytes = r.pos;
    s.all_bytes_accounted = s.read_error.is_none() && s.bytes_framed + s.truncated_tail_bytes == s.stream_bytes;
    Ok(s)
}

fn fill_payload(rec: &mut PacketRecord, offset: u64, payload: &[u8], opts: FrameOptions) {
    rec.payload_offset = offset;
    rec.payload_length = payload.len() as u64;
    rec.payload_sha256 = sha256_hex(payload);
    if opts.include_payload_hex {
        rec.payload_hex = Some(hex(payload));
    }
}

fn emit(
    s: &mut FramingSummary,
    rec: &PacketRecord,
    payload: &[u8],
    sink: &mut impl FnMut(&PacketRecord, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    s.records += 1;
    if rec.record == RecordKind::Truncated {
        s.truncated_tail_bytes += (rec.payload_offset + rec.payload_length) - rec.offset;
    } else {
        s.bytes_framed += rec.prefix_len as u64 + rec.size as u64;
    }
    if s.end_marker_index.is_some() {
        s.records_after_end_marker += 1;
    }
    match rec.record {
        RecordKind::ZeroSize => s.zero_size_records += 1,
        RecordKind::Malformed => s.malformed_records += 1,
        _ => {}
    }
    if let Some(type_id) = rec.type_id {
        s.packets += 1;
        if rec.kind.is_some() {
            s.recognized_packets += 1;
        } else {
            s.unknown_type_packets += 1;
        }
        if rec.type_raw.unwrap_or(0) & !0x1f != 0 {
            s.type_high_bits_packets += 1;
        }
        if type_id == 0 && s.end_marker_index.is_none() {
            s.end_marker_index = Some(rec.index);
        }
        let t = s.types.entry(type_id).or_insert_with(|| TypeStats {
            type_id,
            kind: rec.kind,
            evidence: Some(rec.kind_evidence),
            min_payload: u64::MAX,
            ..Default::default()
        });
        t.count += 1;
        t.payload_bytes += rec.payload_length;
        t.min_payload = t.min_payload.min(rec.payload_length);
        t.max_payload = t.max_payload.max(rec.payload_length);
        match rec.timestamp_source {
            TimestampSource::Explicit => {
                t.explicit_timestamps += 1;
                s.clock.explicit_count += 1;
            }
            TimestampSource::Inherited => {
                t.inherited_timestamps += 1;
                s.clock.inherited_count += 1;
            }
            TimestampSource::InheritedInitial => {
                t.inherited_timestamps += 1;
                s.clock.inherited_count += 1;
                s.clock.inherited_before_first_explicit += 1;
            }
            TimestampSource::None => {}
        }
        if let Some(ts) = rec.timestamp_ms {
            t.first_timestamp_ms.get_or_insert(ts);
            t.last_timestamp_ms = Some(ts);
            if rec.timestamp_source == TimestampSource::Explicit {
                s.clock.first_timestamp_ms.get_or_insert(ts);
                s.clock.last_timestamp_ms = Some(ts);
            }
        }
    }
    if let (Some(a), Some(b)) = (s.clock.first_timestamp_ms, s.clock.last_timestamp_ms) {
        s.clock.span_ms = Some(b.saturating_sub(a));
    }
    sink(rec, payload)
}
