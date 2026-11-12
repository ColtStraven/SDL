//! Flight / cockpit / combat decoders on synthetic payloads (reader behaviour
//! only). The genuine-replay checks live in `genuine_replays.rs`.

use sdl_replay_decoder::bits::{BitReader, BitWriter};
use sdl_replay_decoder::flight::{
    parse_ownship_sync, parse_ownship_sync_hinted, trailer_position, OffsetHints, SyncSkip, SYNC_PREFIX, SYNC_PREFIX_BITS,
};
use sdl_replay_decoder::mpi::{length_prefixed_strings, parse_cockpit, parse_combat, read_leb128, CockpitError, SIG_COCKPIT};

#[test]
fn bit_reader_is_msb_first_with_little_endian_values() {
    // Byte-aligned reads equal ordinary little-endian reads.
    let mut r = BitReader::new(&[0x34, 0x12, 0x78, 0x56, 0x34, 0x12]);
    assert_eq!(r.u16(), Some(0x1234));
    assert_eq!(r.u32(), Some(0x1234_5678));
    assert_eq!(r.u8(), None);
    // Single bits come from the top of each byte.
    let mut r = BitReader::new(&[0b1010_0000]);
    assert_eq!(r.bits(3), Some(0b101));
    // An unaligned f32 round-trips through the writer.
    let mut w = BitWriter::new();
    w.bit(true).f32(541.0188).bits(0b10, 2);
    let b = w.finish();
    let mut r = BitReader::new(&b);
    assert_eq!(r.bit(), Some(true));
    assert_eq!(r.f32(), Some(541.0188));
    assert_eq!(r.bits(2), Some(0b10));
}

#[test]
fn genuine_sync_prefix_bytes() {
    // First bytes of every ownship sync in replay #2026.11.09 18.50.56.
    let mut r = BitReader::new(&[0xd6, 0x81, 0xf5, 0x80, 0x62]);
    assert_eq!(r.bits(SYNC_PREFIX_BITS), Some(SYNC_PREFIX));
    assert_eq!(r.bit(), Some(true), "0x62 marks on-ground");
}

/// Leading record with an explicit weapon block. Layout: prefix, on_ground,
/// unknown bit, u32, flags(4), entry count(4), [8 bits when firing], weapon
/// block, closing bit, position, opaque bits, optional trailing copy.
fn sync_with_block(on_ground: bool, flags: u8, count: u8, block: &[(u64, usize)], pos: [f32; 3], trailer: bool) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.bits(SYNC_PREFIX, SYNC_PREFIX_BITS).bit(on_ground).bit(false).u32(0);
    w.bits(flags as u64, 4).bits(count as u64, 4);
    if flags & 0b1000 != 0 {
        w.u8(1);
    }
    for &(v, n) in block {
        w.bits(v, n);
    }
    w.bit(false);
    for v in pos {
        w.f32(v);
    }
    w.bits(0x1234, 13); // other records (opaque)
    if trailer {
        w.bits(0, 3); // pad so the trailer ends 5 bits before the last byte boundary
        for v in pos {
            w.f32(v);
        }
    }
    w.finish()
}

/// Me 262 layout: one 16-bit counter entry (`u16le`, written MSB-first per byte).
fn sync(on_ground: bool, firing: bool, counter: u8, pos: [f32; 3], trailer: bool) -> Vec<u8> {
    let flags = if firing { 0b1010 } else { 0 };
    sync_with_block(on_ground, flags, 1, &[(counter as u64, 8), (0, 8)], pos, trailer)
}

#[test]
fn parses_ownship_record_and_firing_block() {
    let pos = [-7317.92, 541.02, -29963.81];
    let s = parse_ownship_sync(&sync(true, false, 32, pos, false)).unwrap();
    assert!(s.on_ground && !s.firing);
    assert_eq!((s.counter, s.position, s.position_bit_offset), (Some(32), pos, 97));
    assert_eq!((s.entry_count, s.weapon_block_bits, s.position_source), (1, 16, "single_entry_layout"));

    let s = parse_ownship_sync(&sync(false, true, 30, pos, false)).unwrap();
    assert!(!s.on_ground && s.firing);
    assert_eq!((s.flags, s.triggers, s.firing_extra, s.counter, s.position_bit_offset), (0b1010, 0b010, Some(1), Some(30), 105));
    assert_eq!(s.position, pos);
}

#[test]
fn genuine_me262_header_byte_reads_one_entry() {
    // Bits 72..80 of a genuine Me 262 sync (`00000001`) and a firing one
    // (`10100001`): flags then entry count 1.
    for (byte, flags) in [(0b0000_0001u8, 0u8), (0b1010_0001, 0b1010)] {
        let buf = [byte];
        let mut r = BitReader::new(&buf);
        assert_eq!((r.bits(4), r.bits(4)), (Some(flags as u64), Some(1)));
    }
}

#[test]
fn trailer_position_is_found() {
    let pos = [1279.07, 1741.59, -16057.5];
    let b = sync(false, false, 32, pos, true);
    assert_eq!(trailer_position(&b), Some(pos));
    let s = parse_ownship_sync(&b).unwrap();
    assert!(s.trailer_matches);
    assert_eq!(s.position_source, "trailer_match");
}

/// F-84B-like block: 6 x 24-bit gun entries (`6c 02 00`, as in the genuine
/// replay) and 8 x 16-bit store entries (`01 00`): 14 entries, 272 bits.
fn f84b_block() -> Vec<(u64, usize)> {
    let mut b = vec![(0x6c0200, 24); 6];
    b.extend(vec![(0x0100, 16); 8]);
    b
}

#[test]
fn multi_entry_block_is_located_by_trailer() {
    let pos = [5215.8, 1794.3, 4362.1];
    let b = sync_with_block(true, 0, 14, &f84b_block(), pos, true);
    let s = parse_ownship_sync(&b).unwrap();
    assert_eq!((s.entry_count, s.weapon_block_bits, s.position_bit_offset), (14, 272, 80 + 272 + 1));
    assert_eq!((s.position, s.position_source, s.counter), (pos, "trailer_match", None));
    assert!(s.weapon_block_hex.starts_with("6c0200"));
}

#[test]
fn multi_entry_block_without_trailer_uses_hint_or_refuses() {
    let pos = [5215.8, 1794.3, 4362.1];
    let b = sync_with_block(false, 0, 14, &f84b_block(), pos, false);
    assert_eq!(parse_ownship_sync(&b).unwrap_err(), SyncSkip::UnexpectedLayout);
    let mut hints = OffsetHints::new();
    hints.insert((14, false), 353);
    let s = parse_ownship_sync_hinted(&b, &hints).unwrap();
    assert_eq!((s.position, s.position_source), (pos, "previous_offset"));
    // A hint for another entry count or firing state is not used.
    let mut other = OffsetHints::new();
    other.insert((14, true), 353);
    assert_eq!(parse_ownship_sync_hinted(&b, &other).unwrap_err(), SyncSkip::UnexpectedLayout);
}

#[test]
fn zero_regions_and_shifted_floats_are_not_positions() {
    // A zero weapon block must not "match" a zero-padding trailer read, and
    // the earliest exact copy of the trailer is the real position.
    let pos = [-7211.67, 541.02, -28988.91];
    let b = sync_with_block(true, 0, 3, &[(0, 32), (0, 32), (0, 32)], pos, true);
    let s = parse_ownship_sync(&b).unwrap();
    assert_eq!((s.position, s.position_bit_offset), (pos, 80 + 96 + 1));
}

#[test]
fn pre_spawn_sync_has_no_ownship_record() {
    // A 19-byte sync from before the author spawned (genuine bytes).
    let b = [0xd6, 0x81, 0xf5, 0x80, 0x7f, 0xfb, 0xf8, 0xfb, 0x8f, 0x4a, 0x2b, 0x30, 0xf0, 0xb2, 0x2d, 0x9d, 0xca, 0x5e, 0x38];
    assert_eq!(parse_ownship_sync(&b).unwrap_err(), SyncSkip::NoOwnshipRecord);
    assert_eq!(parse_ownship_sync(&[0xd6]).unwrap_err(), SyncSkip::Truncated);
}

#[test]
fn implausible_single_entry_position_is_refused() {
    // One entry but a non-position where the position should be (no trailer).
    let b = sync_with_block(false, 0, 1, &[(0, 16)], [0.0, 0.0, 0.0], false);
    assert_eq!(parse_ownship_sync(&b).unwrap_err(), SyncSkip::UnexpectedLayout);
}

fn cockpit(pairs: &[(u16, f32)], declared_extra: u64) -> Vec<u8> {
    let mut b = SIG_COCKPIT.to_vec();
    let mut bits = (16 + 48 * pairs.len()) as u64 + declared_extra;
    loop {
        let byte = (bits & 0x7f) as u8;
        bits >>= 7;
        if bits == 0 {
            b.push(byte);
            break;
        }
        b.push(byte | 0x80);
    }
    b.extend((pairs.len() as u16).to_le_bytes());
    for (id, v) in pairs {
        b.extend(id.to_le_bytes());
        b.extend(v.to_le_bytes());
    }
    b
}

#[test]
fn leb128_matches_genuine_length_prefix() {
    // `b0 0b` precedes 30 cockpit parameters: 16 + 48 * 30 = 1456 bits.
    let mut p = 0;
    assert_eq!(read_leb128(&[0xb0, 0x0b], &mut p), Some(1456));
    assert_eq!(p, 2);
    assert_eq!(read_leb128(&[0x80], &mut 0), None);
}

#[test]
fn parses_cockpit_params() {
    let b = cockpit(&[(0, 176.3), (40, 891.7), (53, 15.86)], 0);
    let s = parse_cockpit(&b).unwrap();
    assert_eq!(s.get(40), Some(891.7));
    assert_eq!(s.params[0].name, Some("ias_m_s"));
    assert_eq!(s.declared_bits, 16 + 48 * 3);
}

#[test]
fn cockpit_length_mismatch_is_rejected() {
    let b = cockpit(&[(0, 1.0)], 8);
    assert!(matches!(parse_cockpit(&b), Err(CockpitError::LengthMismatch { .. })));
    let mut b = cockpit(&[(0, 1.0)], 0);
    b.pop();
    assert!(parse_cockpit(&b).is_err());
    assert_eq!(parse_cockpit(&[0x02, 0x58, 0x4a, 0xd0]).unwrap_err(), CockpitError::NotCockpit);
}

#[test]
fn kill_record_surfaces_vehicle_and_unit_ids() {
    // Genuine kill record prefix from replay #2026.11.09 18.50.56.
    let hexs = "025858f03800feff07000000000d6d652d323632612d31615f75341c09ac01ffffffff01";
    let b: Vec<u8> = (0..hexs.len()).step_by(2).map(|i| u8::from_str_radix(&hexs[i..i + 2], 16).unwrap()).collect();
    let c = parse_combat(&b).unwrap();
    assert_eq!(c.kind, "kill");
    assert_eq!(c.message_id_hex, "0xF058");
    assert_eq!(c.vehicle_names, vec!["me-262a-1a_u4".to_string()]);
    assert_eq!(c.unit_ids_after_name, Some([0x091c, 0x01ac]));
    assert!(parse_combat(&[0x02, 0x58, 0x99, 0xf0]).is_none());
    assert!(length_prefixed_strings(&[3, b'a', 0, b'c'], 3).is_empty());
}

#[test]
fn cockpit_ids_map_to_8111_indicator_keys() {
    use sdl_replay_decoder::mpi::{cockpit_param_indicator_key, cockpit_param_name, COCKPIT_PARAMS};
    // Verified against the Phosphor 8111 capture for replay #2026.11.09 18.50.56.
    assert_eq!((cockpit_param_name(0), cockpit_param_indicator_key(0)), (Some("ias_m_s"), Some("speed")));
    assert_eq!(cockpit_param_indicator_key(52), Some("aviahorizon_roll"));
    assert_eq!(cockpit_param_name(195), Some("fuel_kg"));
    assert_eq!(cockpit_param_name(273), None, "firing-related id stays unnamed until confirmed");
    let mut ids: Vec<u16> = COCKPIT_PARAMS.iter().map(|p| p.0).collect();
    let n = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), n, "ids are sorted and unique");
}
