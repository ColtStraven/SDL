//! Flight / cockpit / combat decoders on synthetic payloads (reader behaviour
//! only). The genuine-replay checks live in `genuine_replays.rs`.

use sdl_replay_decoder::bits::{BitReader, BitWriter};
use sdl_replay_decoder::flight::{parse_ownship_sync, trailer_position, SyncSkip, SYNC_PREFIX, SYNC_PREFIX_BITS};
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

fn sync(on_ground: bool, firing: bool, counter: u8, pos: [f32; 3], trailer: bool) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.bits(SYNC_PREFIX, SYNC_PREFIX_BITS).bit(on_ground).bit(false).u32(0);
    w.bit(firing).bits(if firing { 0b0100 } else { 0 }, 4).bits(0b001, 3);
    if firing {
        w.u8(1);
    }
    w.u8(counter).bits(0, 9);
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

#[test]
fn parses_ownship_record_and_firing_block() {
    let pos = [-7317.92, 541.02, -29963.81];
    let s = parse_ownship_sync(&sync(true, false, 32, pos, false)).unwrap();
    assert!(s.on_ground && !s.firing);
    assert_eq!((s.counter, s.position, s.position_bit_offset), (32, pos, 97));

    let s = parse_ownship_sync(&sync(false, true, 30, pos, false)).unwrap();
    assert!(!s.on_ground && s.firing);
    assert_eq!((s.triggers, s.firing_extra, s.counter, s.position_bit_offset), (0b0100, Some(1), 30, 105));
    assert_eq!(s.position, pos);
}

#[test]
fn trailer_position_is_found() {
    let pos = [1279.07, 1741.59, -16057.5];
    let b = sync(false, false, 32, pos, true);
    assert_eq!(trailer_position(&b), Some(pos));
    assert!(parse_ownship_sync(&b).unwrap().trailer_matches);
}

#[test]
fn pre_spawn_sync_has_no_ownship_record() {
    // A 19-byte sync from before the author spawned (genuine bytes).
    let b = [0xd6, 0x81, 0xf5, 0x80, 0x7f, 0xfb, 0xf8, 0xfb, 0x8f, 0x4a, 0x2b, 0x30, 0xf0, 0xb2, 0x2d, 0x9d, 0xca, 0x5e, 0x38];
    assert_eq!(parse_ownship_sync(&b).unwrap_err(), SyncSkip::NoOwnshipRecord);
    assert_eq!(parse_ownship_sync(&[0xd6]).unwrap_err(), SyncSkip::Truncated);
}

#[test]
fn unexpected_group_bits_are_not_interpreted() {
    let mut w = BitWriter::new();
    w.bits(SYNC_PREFIX, SYNC_PREFIX_BITS).bit(false).bit(false).u32(0).bit(false).bits(0, 4).bits(0b011, 3);
    w.u8(0).bits(0, 9).f32(1.0).f32(2.0).f32(3.0);
    assert_eq!(parse_ownship_sync(&w.finish()).unwrap_err(), SyncSkip::UnexpectedLayout);
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
