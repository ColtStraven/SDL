mod common;

use common::*;
use sdl_replay_decoder::container::{self, ContainerError, HEADER_FIELDS, HEADER_SIZE};
use sdl_replay_decoder::diagnostics::Diagnostics;
use sdl_replay_decoder::evidence::Evidence;

#[test]
fn header_field_table_tiles_header_exactly() {
    let mut next = 0;
    for f in HEADER_FIELDS {
        assert_eq!(f.offset, next, "gap or overlap before field {}", f.name);
        next = f.offset + f.size;
    }
    assert_eq!(next, HEADER_SIZE);
}

#[test]
fn signature_is_little_endian_on_disk() {
    let h = header(MAGIC_18C1C, 0, 0);
    assert_eq!(&h[0..4], &[0xE5, 0xAC, 0x00, 0x10]);
    assert_eq!(&h[4..8], &[0x1C, 0x8C, 0x01, 0x00]);
    let mut d = Diagnostics::default();
    let p = container::parse_header(&h, &mut d).unwrap();
    assert_eq!(p.header.signature, 0x1000_ACE5);
    assert_eq!(p.header.format_magic, 0x0001_8C1C);
}

#[test]
fn parses_documented_offsets() {
    let h = header(MAGIC_18C1C, 694, 149_457);
    let mut d = Diagnostics::default();
    let p = container::parse_header(&h, &mut d).unwrap();
    assert_eq!(p.header.session_id, SESSION_ID);
    assert_eq!(p.header.settings_blk_size, 694);
    assert_eq!(p.header.footer_blk_offset, 149_457);
    assert_eq!(p.header.start_time, 1_792_795_650);
    assert_eq!(p.header.level_path, "levels/air_test.bin");
    assert_eq!(p.header.mission_name, "synthetic_mission");
    assert_eq!(p.header.weather, "clear");
    assert_eq!(container::game_version_string(p.header.game_version_raw), "2.59.0.86");
    assert_eq!(p.profile.unwrap().name, "18c1c");
    assert!(d.0.is_empty());
}

#[test]
fn byte_swapped_session_id_is_not_misread() {
    // Big-endian placement must not decode as the little-endian value.
    let mut h = header(MAGIC_18C1C, 0, 0);
    h[732..740].copy_from_slice(&SESSION_ID.to_be_bytes());
    let p = container::parse_header(&h, &mut Diagnostics::default()).unwrap();
    assert_eq!(p.header.session_id, SESSION_ID.swap_bytes());
}

#[test]
fn rejects_wrong_signature() {
    let mut h = header(MAGIC_18C1C, 0, 0);
    h[0] = 0x00;
    match container::parse_header(&h, &mut Diagnostics::default()) {
        Err(ContainerError::BadSignature { found }) => assert_eq!(found, 0x1000_AC00),
        Err(e) => panic!("unexpected error {e}"),
        Ok(_) => panic!("accepted a bad signature"),
    }
}

#[test]
fn rejects_wrong_endian_signature() {
    let mut h = header(MAGIC_18C1C, 0, 0);
    h[0..4].copy_from_slice(&0x1000_ACE5u32.to_be_bytes());
    assert!(matches!(container::parse_header(&h, &mut Diagnostics::default()), Err(ContainerError::BadSignature { .. })));
}

#[test]
fn rejects_short_files() {
    for len in [0, 3, 7, 8, 100, HEADER_SIZE - 1] {
        let h = header(MAGIC_18C1C, 0, 0);
        let r = container::parse_header(&h[..len], &mut Diagnostics::default());
        assert!(matches!(r, Err(ContainerError::TooShort { .. })), "len {len} accepted");
    }
}

#[test]
fn unknown_magic_is_flagged_and_downgraded() {
    let h = header(0x0001_9999, 0, 0);
    let mut d = Diagnostics::default();
    let p = container::parse_header(&h, &mut d).unwrap();
    assert!(p.profile.is_none());
    assert!(d.0.iter().any(|x| x.code == "unknown_format_magic"));
    let sid = p.fields.iter().find(|f| f.name == "session_id").unwrap();
    assert_eq!(sid.evidence, Evidence::Experimental);
}

#[test]
fn unknown_fields_are_preserved_raw() {
    let mut h = header(MAGIC_18C1C, 0, 0);
    h[704..724].copy_from_slice(&[0x5A; 20]);
    let p = container::parse_header(&h, &mut Diagnostics::default()).unwrap();
    let unk0 = p.fields.iter().find(|f| f.name == "unk0").unwrap();
    assert_eq!(unk0.raw_hex, "5a".repeat(20));
    assert_eq!(unk0.evidence, Evidence::Unknown);
    assert!(unk0.value.is_null());
}

#[test]
fn string_residue_after_nul_is_kept() {
    let mut h = header(MAGIC_18C1C, 0, 0);
    h[652 + 10] = b'X';
    let p = container::parse_header(&h, &mut Diagnostics::default()).unwrap();
    let w = p.fields.iter().find(|f| f.name == "weather").unwrap();
    assert_eq!(w.value, "clear");
    assert!(w.residue_hex.as_ref().unwrap().contains("58"));
}

#[test]
fn layout_rejects_out_of_bounds_sections() {
    let mut f = header(MAGIC_18C1C, 5000, 0);
    f.extend_from_slice(&[0u8; 10]);
    let mut d = Diagnostics::default();
    let p = container::parse_header(&f, &mut d).unwrap();
    let l = container::layout(&f, &p.header, &mut d);
    assert!(l.settings_blk.is_none());
    assert!(d.0.iter().any(|x| x.code == "settings_out_of_bounds"));

    let mut f = header(MAGIC_18C1C, 0, 0xFFFF_FFF0);
    f.extend_from_slice(&[0u8; 10]);
    let mut d = Diagnostics::default();
    let p = container::parse_header(&f, &mut d).unwrap();
    let l = container::layout(&f, &p.header, &mut d);
    assert!(d.0.iter().any(|x| x.code == "footer_offset_invalid"));
    assert!(l.footer_blk.is_none());
    assert_eq!(l.packet_stream.unwrap().length, 10);
}

#[test]
fn classifies_client_and_server() {
    let p = container::parse_header(&header(MAGIC_18C1C, 0, 0), &mut Diagnostics::default()).unwrap();
    assert_eq!(container::classify_recording(&p.header).kind, "client");

    let mut h = header(MAGIC_18C1C, 0, 0);
    h[932..936].copy_from_slice(&i32::MIN.to_le_bytes());
    h[742..744].copy_from_slice(&90u16.to_le_bytes());
    let p = container::parse_header(&h, &mut Diagnostics::default()).unwrap();
    assert_eq!(container::classify_recording(&p.header).kind, "server");

    h[742..744].copy_from_slice(&0u16.to_le_bytes());
    let p = container::parse_header(&h, &mut Diagnostics::default()).unwrap();
    assert_eq!(container::classify_recording(&p.header).kind, "unknown");
}
