//! End-to-end tests on synthetic replays (reader behaviour only).
mod common;

use common::*;
use sdl_replay_decoder::evidence::Evidence;
use sdl_replay_decoder::manifest::{analyze, Depth, Manifest, Options};
use sdl_replay_decoder::versions::Codec;

fn run(file: &[u8], depth: Depth) -> Manifest {
    let opts = Options { depth, ..Default::default() };
    analyze("fixture.wrpl", file, opts, |_, _| Ok(())).unwrap()
}

fn field_evidence(m: &Manifest, name: &str) -> Evidence {
    m.header_fields.iter().find(|f| f.name == name).unwrap().evidence
}

fn has(m: &Manifest, code: &str) -> bool {
    m.diagnostics.0.iter().any(|d| d.code == code)
}

#[test]
fn zstd_replay_end_to_end() {
    let stream = sample_stream();
    let file = replay(MAGIC_18C1C, &zstd(&stream), true);
    let m = run(&file, Depth::Packets);
    assert_eq!(m.status, "ok", "{:?}", m.diagnostics);
    let st = m.stream.as_ref().unwrap();
    assert_eq!(st.codec_detected, Some(Codec::Zstd));
    assert_eq!(st.codec_matches_profile, Some(true));
    let inf = st.inflation.as_ref().unwrap();
    assert_eq!(inf.decompressed_length, stream.len() as u64);
    assert_eq!(inf.trailing_bytes, 0);
    assert_eq!(inf.compressed_consumed, inf.compressed_length);
    let p = m.packets.as_ref().unwrap();
    assert_eq!(p.records, 7);
    assert!(p.all_bytes_accounted);
    // Structural cross-checks promote these two fields.
    assert_eq!(field_evidence(&m, "settings_blk_size"), Evidence::Verified);
    assert_eq!(field_evidence(&m, "footer_blk_offset"), Evidence::Verified);
    assert_eq!(field_evidence(&m, "session_id"), Evidence::Reported);
    let l = m.layout.as_ref().unwrap();
    assert_eq!(l.footer_blk.as_ref().unwrap().length, FOOTER.len() as u64);
    assert_eq!(m.settings_blk.as_ref().unwrap().leading_byte, Some(1));
    assert_eq!(m.session.as_ref().unwrap().start_time_utc.value, "2026-10-23T22:47:30Z");
}

#[test]
fn zlib_replay_end_to_end() {
    let stream = sample_stream();
    let file = replay(MAGIC_18C0B, &zlib(&stream), true);
    let m = run(&file, Depth::Packets);
    assert_eq!(m.status, "ok", "{:?}", m.diagnostics);
    let st = m.stream.as_ref().unwrap();
    assert_eq!(st.codec_detected, Some(Codec::Zlib));
    assert_eq!(st.codec_matches_profile, Some(true));
    assert_eq!(st.inflation.as_ref().unwrap().trailing_bytes, 0);
    assert_eq!(m.packets.as_ref().unwrap().records, 7);
    assert_eq!(field_evidence(&m, "footer_blk_offset"), Evidence::Verified);
}

#[test]
fn inspect_depth_inflates_without_framing() {
    let file = replay(MAGIC_18C1C, &zstd(&sample_stream()), true);
    let m = run(&file, Depth::Inspect);
    assert!(m.packets.is_none());
    assert!(m.stream.unwrap().inflation.unwrap().decompressed_length > 0);
}

#[test]
fn no_footer_runs_stream_to_eof() {
    let file = replay(MAGIC_18C1C, &zstd(&sample_stream()), false);
    let m = run(&file, Depth::Packets);
    assert_eq!(m.status, "ok");
    assert!(has(&m, "no_footer"));
    assert!(m.layout.as_ref().unwrap().footer_blk.is_none());
    assert_eq!(field_evidence(&m, "footer_blk_offset"), Evidence::Reported);
}

#[test]
fn codec_mismatch_is_warned() {
    let file = replay(MAGIC_18C1C, &zlib(&sample_stream()), true);
    let m = run(&file, Depth::Packets);
    assert!(has(&m, "codec_profile_mismatch"));
    // Still decodes: the codec is probed, not assumed.
    assert_eq!(m.packets.as_ref().unwrap().records, 7);
}

#[test]
fn unknown_codec_is_an_error() {
    let file = replay(MAGIC_18C1C, b"not compressed data at all", true);
    let m = run(&file, Depth::Packets);
    assert_eq!(m.status, "partial");
    assert!(has(&m, "unknown_stream_codec"));
    assert_eq!(field_evidence(&m, "settings_blk_size"), Evidence::Reported);
}

#[test]
fn unknown_format_magic_still_reports() {
    let file = replay(0x0001_9000, &zstd(&sample_stream()), true);
    let m = run(&file, Depth::Packets);
    assert!(has(&m, "unknown_format_magic"));
    assert!(!m.format.as_ref().unwrap().recognized);
    assert_eq!(m.stream.as_ref().unwrap().codec_matches_profile, None);
    assert_eq!(field_evidence(&m, "session_id"), Evidence::Experimental);
    assert_eq!(m.packets.as_ref().unwrap().records, 7);
}

#[test]
fn truncated_compressed_stream_is_partial_not_panic() {
    let c = zstd(&sample_stream());
    let cut = &c[..c.len() - 6];
    let file = replay(MAGIC_18C1C, cut, false);
    let m = run(&file, Depth::Packets);
    assert_eq!(m.status, "partial");
    assert!(has(&m, "decompression_failed") || has(&m, "packet_stream_truncated"));
}

#[test]
fn trailing_garbage_after_frame_is_reported() {
    let mut c = zstd(&sample_stream());
    c.extend_from_slice(b"GARBAGE!");
    let file = replay(MAGIC_18C1C, &c, true);
    let m = run(&file, Depth::Packets);
    let inf = m.stream.as_ref().unwrap().inflation.as_ref().unwrap();
    assert_eq!(inf.trailing_bytes, 8);
    assert_eq!(inf.trailing_prefix_hex.as_deref(), Some("4741524241474521"));
    assert!(has(&m, "stream_trailing_bytes"));
    assert_eq!(field_evidence(&m, "footer_blk_offset"), Evidence::Reported);
}

#[test]
fn multiple_zstd_frames_are_tracked() {
    let s = sample_stream();
    let (a, b) = s.split_at(s.len() / 2);
    let mut c = zstd(a);
    let first_len = c.len() as u64;
    c.extend(zstd(b));
    let file = replay(MAGIC_18C1C, &c, true);
    let m = run(&file, Depth::Packets);
    let inf = m.stream.as_ref().unwrap().inflation.as_ref().unwrap();
    assert_eq!(inf.frames.len(), 2);
    assert_eq!(inf.frames[1].compressed_offset, first_len);
    assert_eq!(inf.frames[1].decompressed_offset, a.len() as u64);
    assert_eq!(inf.trailing_bytes, 0);
    // A packet straddling the frame boundary still frames correctly.
    assert_eq!(m.packets.as_ref().unwrap().records, 7);
    assert!(m.packets.as_ref().unwrap().all_bytes_accounted);
}

#[test]
fn decompression_limit_stops_output() {
    let big = sample_stream().repeat(200);
    let file = replay(MAGIC_18C1C, &zstd(&big), true);
    let opts = Options { depth: Depth::Packets, max_output: 1000, include_payload_hex: false };
    let m = analyze("bomb.wrpl", &file, opts, |_, _| Ok(())).unwrap();
    let inf = m.stream.as_ref().unwrap().inflation.as_ref().unwrap();
    assert!(inf.output_limit_hit);
    assert_eq!(inf.decompressed_length, 1000);
    assert_eq!(m.status, "partial");
}

#[test]
fn not_a_replay_is_rejected() {
    let m = run(b"PK\x03\x04 zip file", Depth::Packets);
    assert_eq!(m.status, "rejected");
    assert!(has(&m, "not_a_replay"));
    assert_eq!(m.input.size, 13);
}

/// Deterministic mutation sweep: corrupted inputs must never panic.
#[test]
fn corrupted_inputs_do_not_panic() {
    let base = replay(MAGIC_18C1C, &zstd(&sample_stream().repeat(5)), true);
    let mut seed = 0x5735_7357_u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for i in 0..400 {
        let mut f = base.clone();
        match i % 3 {
            0 => {
                for _ in 0..4 {
                    let at = 1234 + (next() as usize % (f.len() - 1234));
                    f[at] ^= (next() & 0xff) as u8 | 1;
                }
            }
            1 => {
                // Header numeric fields.
                let at = [684, 685, 686, 687, 748, 749][next() as usize % 6];
                f[at] = (next() & 0xff) as u8;
            }
            _ => {
                let cut = next() as usize % f.len();
                f.truncate(cut);
            }
        }
        let m = run(&f, Depth::Packets);
        assert!(["ok", "partial", "rejected"].contains(&m.status));
    }
}
