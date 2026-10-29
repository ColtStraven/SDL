//! Synthetic `.wrpl` fixture builder.
//!
//! These fixtures exercise reader behaviour only. Passing them is NOT evidence
//! of compatibility with genuine War Thunder replays.
#![allow(dead_code)]

use sdl_replay_decoder::packets::encode_size_prefix;
use std::io::Write;

pub const SIG: [u8; 4] = [0xE5, 0xAC, 0x00, 0x10];
pub const MAGIC_18C1C: u32 = 0x0001_8C1C;
pub const MAGIC_18C0B: u32 = 0x0001_8C0B;
pub const SESSION_ID: u64 = 0x4AD2_12E8_00E5_1D58;

pub fn put_str(h: &mut [u8], off: usize, s: &str) {
    h[off..off + s.len()].copy_from_slice(s.as_bytes());
}

pub fn header(magic: u32, settings_size: u16, footer_offset: u32) -> Vec<u8> {
    let mut h = vec![0u8; 1234];
    h[0..4].copy_from_slice(&SIG);
    h[4..8].copy_from_slice(&magic.to_le_bytes());
    put_str(&mut h, 8, "levels/air_test.bin");
    put_str(&mut h, 396, "synthetic_mission");
    put_str(&mut h, 524, "day");
    put_str(&mut h, 652, "clear");
    h[684..688].copy_from_slice(&footer_offset.to_le_bytes());
    h[732..740].copy_from_slice(&SESSION_ID.to_le_bytes());
    h[740] = 0;
    h[748..750].copy_from_slice(&settings_size.to_le_bytes());
    h[908..912].copy_from_slice(&1_792_795_650u32.to_le_bytes());
    h[1224..1226].copy_from_slice(&60u16.to_le_bytes());
    h[1226..1234].copy_from_slice(&0x0002_003B_0000_0056u64.to_le_bytes());
    h
}

/// One packet. `ts = None` sets the inherit-time flag.
pub fn packet(type_id: u16, ts: Option<u32>, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    match ts {
        Some(t) => {
            body.extend_from_slice(&type_id.to_le_bytes());
            body.extend_from_slice(&t.to_le_bytes());
        }
        None => body.extend_from_slice(&(type_id | 0x10).to_le_bytes()),
    }
    body.extend_from_slice(payload);
    let mut out = encode_size_prefix(body.len() as u32);
    out.extend(body);
    out
}

/// A small, well-formed decompressed stream: 7 packets ending with an end marker.
pub fn sample_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend(packet(8, Some(1000), &[0xAA; 20])); // replay_header_info
    s.extend(packet(1, None, &[])); // start_marker, inherited 1000
    s.extend(packet(6, None, &[1, 2, 3])); // ecs, inherited 1000
    s.extend(packet(4, Some(1050), &[9; 200])); // mpi, 2-byte prefix
    s.extend(packet(2, Some(1100), &[7; 300])); // aircraft_small
    s.extend(packet(3, None, b"hello")); // chat, inherited 1100
    s.extend(packet(0, Some(1200), &[])); // end_marker
    s
}

pub fn zstd(data: &[u8]) -> Vec<u8> {
    ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest)
}

pub fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

pub const SETTINGS: &[u8] = b"\x01synthetic-settings-blk\x00mission\x00";
pub const FOOTER: &[u8] = b"\x01synthetic-footer-blk\x00timePlayed\x00";

/// Assemble a replay from parts; footer offset is computed unless overridden.
pub fn replay(magic: u32, compressed: &[u8], with_footer: bool) -> Vec<u8> {
    let stream_start = 1234 + SETTINGS.len();
    let footer_offset = if with_footer { (stream_start + compressed.len()) as u32 } else { 0 };
    let mut f = header(magic, SETTINGS.len() as u16, footer_offset);
    f.extend_from_slice(SETTINGS);
    f.extend_from_slice(compressed);
    if with_footer {
        f.extend_from_slice(FOOTER);
    }
    f
}
