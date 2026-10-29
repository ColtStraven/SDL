//! Structural tests for the type-2 aircraft sync decoder (synthetic payloads).
mod common;

use common::*;
use sdl_replay_decoder::aircraft::{decode_sync, EntryBody};

fn st(x: f32, y: f32, z: f32) -> State {
    State { pos: [x, y, z], euler: pack_euler(10.0, 2.0, -5.0), vel: pack_velocity(200.0, 0.0, -50.0) }
}

#[test]
fn multiple_aircraft_with_implicit_uids() {
    let mut w = BitWriter::default();
    write_state(&mut w, Some(726), &st(1.0, 1500.0, 2.0));
    write_state(&mut w, None, &st(3.0, 1400.0, 4.0)); // 727
    write_state(&mut w, None, &st(5.0, 1300.0, 6.0)); // 728
    write_uid(&mut w, Some(900));
    w.bit(true); // no data
    write_uid(&mut w, None); // 901
    w.bit(false).bit(true).bit(true); // withheld
    write_end(&mut w, [1.0, 1500.0, 2.0]);
    let d = decode_sync(&w.bytes);
    assert!(d.terminated, "{:?}", d.error);
    assert!(d.error.is_none());
    let uids: Vec<u16> = d.entries.iter().map(|e| e.uid).collect();
    assert_eq!(uids, [726, 727, 728, 900, 901]);
    assert!(d.entries[0].uid_explicit && !d.entries[1].uid_explicit);
    match &d.entries[2].body {
        EntryBody::State(s) => {
            assert_eq!(s.position, [5.0, 1300.0, 6.0]);
            assert_eq!(s.euler_raw, pack_euler(10.0, 2.0, -5.0));
            assert_eq!(s.packed_velocity_raw, pack_velocity(200.0, 0.0, -50.0));
            assert_eq!(s.controls[0], 0x88);
        }
        b => panic!("expected state, got {b:?}"),
    }
    assert!(matches!(d.entries[3].body, EntryBody::NoData));
    assert!(matches!(d.entries[4].body, EntryBody::Withheld));
    assert_eq!(d.entries[4].hidden_flags, Some((true, true)));
    assert_eq!(d.trailer, Some([1.0, 1500.0, 2.0]));
    assert_eq!(d.consumed_bits, w.bits);
    // Entries tile the payload up to the terminator.
    for p in d.entries.windows(2) {
        assert_eq!(p[1].bit_offset, p[0].bit_offset + p[0].bit_length);
    }
}

#[test]
fn optional_structures_keep_boundaries() {
    let mut w = BitWriter::default();
    write_uid(&mut w, Some(5));
    w.bit(false).bit(true).bit(false); // data, hidden (1, 0)
    // ground=1, unk_u32, unk_bit=1, gun block with 3 entries, 2 weapon slots.
    w.bit(true).u32(0x3E55_5556).bit(true);
    w.bit(true).bit(true).bit(false).bit(true).bits(4, 3).bit(true).bit(false).bit(true);
    w.zigzag(2).zigzag(7).zigzag(2).zigzag(-3).zigzag(4).zigzag(1).zigzag(0);
    w.f32(10.0).f32(20.0).f32(30.0).u32(pack_euler(-90.0, 10.0, 45.0)).u32(pack_velocity(0.0, 0.0, -300.0));
    w.align();
    w.raw(&[1, 2, 3, 4, 5, 6, 7]);
    // Engines: one with boost and health, one plain.
    w.byte(2);
    w.bit(true).byte(0x80).u16((-1000i16) as u16).bit(true).byte(100).byte(10).byte(20);
    w.bit(false).byte(0x08).bit(false).byte(0).byte(0);
    // Sensors: kind 1 (full, negative packed -> extra byte, flag2), kind 2 on, kind 4 with block + contacts.
    w.byte(3);
    w.bit(true).byte(0x10).bit(true).u16(0x8001).u32(0).u16(0).u16(0).u16(0).byte(9).bit(true).byte(1).bit(false);
    w.bit(true).byte(0x21).bit(false).u16(0).raw(&[0; 12]).raw(&[0; 12]).u32(0).bit(false);
    w.bit(false).byte(0x42).bit(true).raw(&[0; 12]).bit(true).bits(6, 2).u32(0xFFFF_0001).u32(0xFFFF_0002).bits(6, 0);
    w.byte(0xAA); // sensors tail
    w.byte(2).u16(0x0102).u16(0x0304).byte(0xBB); // countermeasures
    // One target via the compressed-position branch, all optional parts present.
    w.bits(4, 1);
    w.byte(1).byte(2).bit(true).raw(&[0; 12]).bit(true).raw(&[0; 12]).raw(&[0; 6]).u32(0);
    w.bit(false).bit(false).bit(true).byte(3).bit(false).bit(true).bit(false).byte(4).bit(true).u32(5);
    w.bit(true).u16(11).u16(22); // tail pair
    w.bit(true).u32(37).bits(37, 0x1_2345_6789 & ((1 << 37) - 1)); // seeker block
    // A second, minimal record must still be found at the right boundary.
    write_state(&mut w, None, &st(-1.0, 50.0, -2.0));
    write_end(&mut w, [0.0, 0.0, 0.0]);

    let d = decode_sync(&w.bytes);
    assert!(d.terminated && d.error.is_none(), "{:?}", d.error);
    assert_eq!(d.entries.len(), 2);
    let EntryBody::State(s) = &d.entries[0].body else { panic!() };
    assert!(s.ground_flag && s.unk_bit);
    assert_eq!(s.unk_u32, 0x3E55_5556);
    let g = s.guns.as_ref().unwrap();
    assert_eq!(g.triggers, [true, false, true]);
    assert_eq!(g.entry_bits, [true, false, true]);
    assert_eq!(s.weapons.len(), 2);
    assert_eq!(s.weapons[0].a, 7);
    assert_eq!(s.weapons[0].values, [-3, 4]);
    assert_eq!(s.weapons[1].values.len(), 0);
    assert_eq!(s.position, [10.0, 20.0, 30.0]);
    assert_eq!(s.controls, [1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(s.engines.len(), 2);
    assert_eq!(s.engines[0].boost_raw, Some(-1000));
    assert_eq!(s.engines[0].health_raw, Some(100));
    assert_eq!(s.engines[0].radiators, [10, 20]);
    assert_eq!(s.engines[1].state, 8);
    assert_eq!((s.sensor_count, s.countermeasure_count, s.target_count), (3, 2, 1));
    assert_eq!(s.tail_pair, Some((11, 22)));
    assert_eq!(s.seeker_bits, Some(37));
    let EntryBody::State(s2) = &d.entries[1].body else { panic!() };
    assert_eq!(s2.position, [-1.0, 50.0, -2.0]);
    assert_eq!(d.entries[1].uid, 6);
}

#[test]
fn terminator_only_and_empty_payloads() {
    let mut w = BitWriter::default();
    write_end(&mut w, [1.0, 2.0, 3.0]);
    let d = decode_sync(&w.bytes);
    assert!(d.terminated && d.entries.is_empty());
    assert_eq!(d.trailer, Some([1.0, 2.0, 3.0]));

    let d = decode_sync(&[]);
    assert!(!d.terminated);
    assert!(d.error.is_some());
}

#[test]
fn trailer_absent_when_too_short() {
    let mut w = BitWriter::default();
    w.bit(true).varint(0x7FF).u32(0);
    let d = decode_sync(&w.bytes);
    assert!(d.terminated);
    assert!(d.trailer.is_none());
    assert!(d.error.is_none());
}

#[test]
fn truncation_at_every_length_is_reported() {
    let mut w = BitWriter::default();
    write_state(&mut w, Some(1), &st(1.0, 2.0, 3.0));
    write_state(&mut w, None, &st(4.0, 5.0, 6.0));
    let body_bytes = (w.bits / 8) as usize; // last byte holds the start of the terminator
    write_end(&mut w, [0.0; 3]);
    for len in 0..body_bytes {
        let d = decode_sync(&w.bytes[..len]);
        assert!(!d.terminated, "len {len} terminated");
        let e = d.error.expect("truncation must be an error");
        assert!(e.bit <= len as u64 * 8);
    }
    assert!(decode_sync(&w.bytes).terminated);
}

#[test]
fn invalid_counts_are_rejected() {
    let mut w = BitWriter::default();
    write_uid(&mut w, Some(1));
    w.bit(false).bit(false).bit(false);
    write_state_body(&mut w, &st(0.0, 0.0, 0.0), |w| {
        w.byte(16); // engines > 15
    });
    let d = decode_sync(&w.bytes);
    assert!(!d.terminated);
    assert_eq!(d.error.unwrap().what, "engines.count > 15");

    let mut w = BitWriter::default();
    write_uid(&mut w, Some(1));
    w.bit(false).bit(false).bit(false);
    write_state_body(&mut w, &st(0.0, 0.0, 0.0), |w| {
        w.byte(0).byte(1).bit(true).byte(0x30); // sensor kind 3
    });
    let d = decode_sync(&w.bytes);
    assert_eq!(d.error.unwrap().what, "sensor kind not in {1,2,4}");

    let mut w = BitWriter::default();
    write_uid(&mut w, Some(1));
    w.bit(false).bit(false).bit(false);
    w.bit(false).u32(0).bit(false).bit(false).zigzag(-1);
    assert_eq!(decode_sync(&w.bytes).error.unwrap().what, "weapons.count out of range");
}
