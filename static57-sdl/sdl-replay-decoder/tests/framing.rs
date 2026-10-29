mod common;

use common::*;
use sdl_replay_decoder::packets::{
    decode_size_prefix, encode_size_prefix, frame_packets, prefix_len, FrameOptions, FramingSummary, PacketRecord,
    RecordKind, TimestampSource,
};

const OPTS: FrameOptions = FrameOptions { segment: 0, include_payload_hex: true };

fn frame(stream: &[u8]) -> (FramingSummary, Vec<PacketRecord>) {
    let mut recs = Vec::new();
    let s = frame_packets(stream, OPTS, |r, _| {
        recs.push(r.clone());
        Ok(())
    })
    .unwrap();
    (s, recs)
}

#[test]
fn size_prefix_vectors_from_genuine_replay() {
    // Prefixes observed in Static 57 replay #2026.10.23 15.47.41.
    assert_eq!(decode_size_prefix(&[0x41, 0x32]), 306);
    assert_eq!(decode_size_prefix(&[0x4a, 0x4f]), 2639);
    assert_eq!(decode_size_prefix(&[0x86]), 6);
    assert_eq!(prefix_len(0x86), 1);
    assert_eq!(prefix_len(0x41), 2);
}

#[test]
fn size_prefix_round_trips_at_boundaries() {
    for size in [0u32, 1, 0x7f, 0x80, 0x3fff, 0x4000, 0x1f_ffff, 0x20_0000, 0x0fff_ffff, 0x1000_0000, u32::MAX] {
        let enc = encode_size_prefix(size);
        assert_eq!(prefix_len(enc[0]), enc.len(), "size {size:#x}");
        assert_eq!(decode_size_prefix(&enc), size, "size {size:#x}");
    }
    assert_eq!(encode_size_prefix(0x7f).len(), 1);
    assert_eq!(encode_size_prefix(0x80).len(), 2);
    assert_eq!(encode_size_prefix(0x4000).len(), 3);
    assert_eq!(encode_size_prefix(0x20_0000).len(), 4);
    assert_eq!(encode_size_prefix(0x1000_0000).len(), 5);
}

#[test]
fn non_canonical_prefix_is_decoded_and_counted() {
    // Value 6 written in the 2-byte form (0x4000 ^ 6).
    let mut s = vec![0x40, 0x06];
    s.extend_from_slice(&0u16.to_le_bytes());
    s.extend_from_slice(&5u32.to_le_bytes());
    let (sum, recs) = frame(&s);
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].size, 6);
    assert_eq!(sum.non_canonical_prefixes, 1);
    assert!(sum.all_bytes_accounted);
}

#[test]
fn five_byte_prefix_form() {
    let body_len = 6u32;
    let mut s = vec![0x00];
    s.extend_from_slice(&body_len.to_le_bytes());
    s.extend_from_slice(&4u16.to_le_bytes());
    s.extend_from_slice(&77u32.to_le_bytes());
    let (sum, recs) = frame(&s);
    assert_eq!(recs[0].prefix_len, 5);
    assert_eq!(recs[0].timestamp_ms, Some(77));
    assert_eq!(*sum.prefix_forms.get(&5).unwrap(), 1);
}

#[test]
fn frames_sample_stream_with_timestamps() {
    let stream = sample_stream();
    let (sum, recs) = frame(&stream);
    assert_eq!(recs.len(), 7);
    let types: Vec<_> = recs.iter().map(|r| r.type_id.unwrap()).collect();
    assert_eq!(types, [8, 1, 6, 4, 2, 3, 0]);
    let ts: Vec<_> = recs.iter().map(|r| r.timestamp_ms.unwrap()).collect();
    assert_eq!(ts, [1000, 1000, 1000, 1050, 1100, 1100, 1200]);
    let src: Vec<_> = recs.iter().map(|r| r.timestamp_source).collect();
    use TimestampSource::*;
    assert_eq!(src, [Explicit, Inherited, Inherited, Explicit, Explicit, Inherited, Explicit]);
    assert_eq!(recs[1].type_raw, Some(0x11));
    assert_eq!(recs[3].payload_length, 200);
    assert_eq!(recs[3].prefix_len, 2);
    assert_eq!(recs[5].payload_hex.as_deref(), Some("68656c6c6f"));
    // Offsets chain exactly.
    for w in recs.windows(2) {
        assert_eq!(w[1].offset, w[0].offset + w[0].prefix_len as u64 + w[0].size as u64);
    }
    assert_eq!(sum.end_marker_index, Some(6));
    assert_eq!(sum.records_after_end_marker, 0);
    assert_eq!(sum.recognized_packets, 7);
    assert_eq!(sum.clock.first_timestamp_ms, Some(1000));
    assert_eq!(sum.clock.last_timestamp_ms, Some(1200));
    assert_eq!(sum.clock.span_ms, Some(200));
    assert_eq!(sum.clock.explicit_count, 4);
    assert_eq!(sum.clock.inherited_count, 3);
    assert_eq!(sum.clock.largest_forward_gap_ms, 100);
    assert!(sum.clock.monotonic);
    assert!(sum.all_bytes_accounted);
    assert_eq!(sum.stream_bytes, stream.len() as u64);
}

#[test]
fn unknown_types_are_preserved() {
    let mut s = packet(0x0C, Some(5), &[1, 2, 3, 4]);
    s.extend(packet(0x0120, Some(6), &[5]));
    let (sum, recs) = frame(&s);
    assert_eq!(recs.len(), 2);
    assert_eq!(recs[0].kind, None);
    assert_eq!(recs[0].payload_hex.as_deref(), Some("01020304"));
    assert_eq!(recs[0].decode_state, "raw");
    assert_eq!(sum.unknown_type_packets, 2);
    assert_eq!(sum.type_high_bits_packets, 1);
    assert!(sum.types.contains_key(&0x0C));
}

#[test]
fn backward_time_is_reported() {
    let mut s = packet(4, Some(500), &[]);
    s.extend(packet(4, Some(300), &[]));
    let (sum, _) = frame(&s);
    assert!(!sum.clock.monotonic);
    assert_eq!(sum.clock.backward_steps, 1);
    assert_eq!(sum.clock.largest_backward_step_ms, 200);
}

#[test]
fn inherit_before_explicit_assigns_zero() {
    let mut s = packet(8, None, &[1]);
    s.extend(packet(4, Some(27), &[]));
    let (sum, recs) = frame(&s);
    assert_eq!(recs[0].timestamp_source, TimestampSource::InheritedInitial);
    assert_eq!(recs[0].timestamp_ms, Some(0));
    assert_eq!(sum.clock.inherited_before_first_explicit, 1);
    assert_eq!(sum.clock.first_timestamp_ms, Some(27));
}

#[test]
fn zero_size_and_malformed_records_are_kept() {
    let mut s = vec![0x80]; // size 0
    s.extend([0x81, 0x04]); // size 1: too short for a type
    s.extend([0x84, 0x04, 0x00, 0x01, 0x02]); // explicit type, timestamp cut short
    s.extend(packet(0, Some(9), &[]));
    let (sum, recs) = frame(&s);
    assert_eq!(recs[0].record, RecordKind::ZeroSize);
    assert_eq!(recs[1].record, RecordKind::Malformed);
    assert_eq!(recs[2].record, RecordKind::Malformed);
    assert_eq!(recs[2].decode_state, "error");
    assert_eq!(recs[3].record, RecordKind::Packet);
    assert_eq!(sum.zero_size_records, 1);
    assert_eq!(sum.malformed_records, 2);
    assert!(sum.all_bytes_accounted);
}

#[test]
fn truncated_body_is_preserved() {
    let mut s = sample_stream();
    s.extend(packet(4, Some(2000), &[3; 50]));
    s.truncate(s.len() - 10);
    let (sum, recs) = frame(&s);
    let last = recs.last().unwrap();
    assert_eq!(last.record, RecordKind::Truncated);
    assert_eq!(last.size, 56);
    assert_eq!(last.payload_length, 46);
    assert!(sum.truncated);
    assert!(sum.all_bytes_accounted, "truncated tail must still be counted");
    assert_eq!(sum.bytes_framed + sum.truncated_tail_bytes, s.len() as u64);
}

#[test]
fn truncated_prefix_is_preserved() {
    let s = [0x20u8, 0x01]; // 3-byte prefix cut after 2 bytes
    let (sum, recs) = frame(&s);
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].record, RecordKind::Truncated);
    assert_eq!(recs[0].prefix_hex, "2001");
    assert!(sum.truncated);
    assert_eq!(sum.truncated_tail_bytes, 2);
}

#[test]
fn records_after_end_marker_are_counted() {
    let mut s = sample_stream();
    s.extend(packet(4, Some(1300), &[1]));
    let (sum, recs) = frame(&s);
    assert_eq!(recs.len(), 8);
    assert_eq!(sum.records_after_end_marker, 1);
}

#[test]
fn empty_stream_yields_no_records() {
    let (sum, recs) = frame(&[]);
    assert!(recs.is_empty());
    assert!(sum.all_bytes_accounted);
    assert_eq!(sum.end_marker_index, None);
}
