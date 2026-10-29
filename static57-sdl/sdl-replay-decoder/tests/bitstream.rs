mod common;

use common::BitWriter;
use sdl_replay_decoder::bitstream::{unpack_sign_magnitude, unpack_signed, BitReader};

#[test]
fn bits_are_msb_first() {
    let data = [0b1010_0000u8];
    let mut r = BitReader::new(&data);
    assert!(r.bit("a").unwrap());
    assert!(!r.bit("b").unwrap());
    assert_eq!(r.bits(2, "c").unwrap(), 0b10);
    assert_eq!(r.position(), 4);
    assert_eq!(r.remaining(), 4);
}

#[test]
fn unaligned_byte_and_integer_reads() {
    let mut w = BitWriter::default();
    w.bit(true).u16(0x1234).u32(0xDEAD_BEEF).f32(-1.5);
    // Unaligned layout: flag bit, then the low byte 0x34 shifted right by one.
    assert_eq!(w.bytes[0], 0x80 | (0x34 >> 1));
    let mut r = BitReader::new(&w.bytes);
    assert!(r.bit("flag").unwrap());
    assert_eq!(r.u16_le("u16").unwrap(), 0x1234);
    assert_eq!(r.u32_le("u32").unwrap(), 0xDEAD_BEEF);
    assert_eq!(r.f32_le("f32").unwrap(), -1.5);
}

#[test]
fn signed_reads() {
    let mut w = BitWriter::default();
    w.bits(3, 0b101).u16((-2i16) as u16);
    let mut r = BitReader::new(&w.bytes);
    r.skip(3, "pad").unwrap();
    assert_eq!(r.i16_le("i16").unwrap(), -2);
}

#[test]
fn varints_and_zigzag() {
    let mut w = BitWriter::default();
    w.bit(false).varint(0).varint(127).varint(128).varint(0x7FF).varint(u64::MAX);
    w.zigzag(0).zigzag(-1).zigzag(1).zigzag(i32::MIN).zigzag(i32::MAX);
    let mut r = BitReader::new(&w.bytes);
    r.bit("pad").unwrap();
    for v in [0u64, 127, 128, 0x7FF, u64::MAX] {
        assert_eq!(r.compressed("v").unwrap(), v);
    }
    for v in [0i32, -1, 1, i32::MIN, i32::MAX] {
        assert_eq!(r.zigzag_i32("z").unwrap(), v);
    }
}

#[test]
fn overlong_varint_is_an_error() {
    let data = [0xFFu8; 12];
    let mut r = BitReader::new(&data);
    assert!(r.compressed("v").is_err());
}

#[test]
fn reads_past_end_fail_without_advancing() {
    let data = [0xAB, 0xCD];
    let mut r = BitReader::new(&data);
    r.skip(9, "pad").unwrap();
    let e = r.byte("late").unwrap_err();
    assert_eq!(e.bit, 9);
    assert_eq!(e.available_bits, 7);
    assert_eq!(e.wanted_bits, 8);
    assert_eq!(r.position(), 9);
    assert!(r.u32_le("x").is_err());
    assert_eq!(r.bits(7, "rest").unwrap(), 0xCD & 0x7F);
    assert!(r.bit("eof").is_err());
    assert!(r.skip(1, "eof").is_err());
}

#[test]
fn align_is_noop_when_aligned() {
    let data = [0u8; 3];
    let mut r = BitReader::new(&data);
    r.align();
    assert_eq!(r.position(), 0);
    r.skip(1, "x").unwrap();
    r.align();
    assert_eq!(r.position(), 8);
    r.skip(8, "x").unwrap();
    r.align();
    assert_eq!(r.position(), 16);
}

#[test]
fn slice_bits_extracts_unaligned_spans() {
    let data = [0b0110_1100, 0b1111_0000];
    assert_eq!(BitReader::slice_bits(&data, 1, 5), vec![0b1101_0000]);
    assert_eq!(BitReader::slice_bits(&data, 4, 12), vec![0b1100_1111]);
}

#[test]
fn quantisation_helpers() {
    assert_eq!(unpack_signed(32767, 16, std::f64::consts::PI), std::f64::consts::PI);
    assert_eq!(unpack_signed(-32767, 16, 1.0), -1.0);
    assert_eq!(unpack_signed(0, 8, 5.0), 0.0);
    assert_eq!(unpack_sign_magnitude(1023, 10, true, 180.0), -180.0);
    assert_eq!(unpack_sign_magnitude(0, 9, false, 90.0), 0.0);
}
