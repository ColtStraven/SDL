//! Track reconstruction, validation statistics and flight export (synthetic).
mod common;

use common::*;
use sdl_replay_decoder::aircraft::decode_sync;
use sdl_replay_decoder::cockpit;
use sdl_replay_decoder::export::{analyze_flight, export_flight};
use sdl_replay_decoder::manifest::Options;
use sdl_replay_decoder::tracks::{FlightReport, FlightSample, ReplayRef, Reconstructor, TrackConfig};

fn replay_ref() -> ReplayRef {
    ReplayRef { replay_sha256: "00".repeat(32), format_magic: "0x00018C1C".into(), game_version: "2.59.0.86".into() }
}

/// Aircraft flying straight along +X at `speed` m/s.
fn straight(x0: f32, y: f32, z: f32, speed: f64, t_s: f64) -> State {
    State { pos: [x0 + (speed * t_s) as f32, y, z], euler: pack_euler(0.0, 0.0, 15.0), vel: pack_velocity(speed, 0.0, 0.0) }
}

fn sync(states: &[(u16, State)], trailer: [f32; 3]) -> Vec<u8> {
    let mut w = BitWriter::default();
    for (uid, s) in states {
        write_state(&mut w, Some(*uid), s);
    }
    write_end(&mut w, trailer);
    w.bytes
}

fn run(frames: &[(u32, Vec<u8>)], cockpits: Vec<cockpit::CockpitParams>) -> (FlightReport, Vec<FlightSample>) {
    let mut r = Reconstructor::new(replay_ref(), TrackConfig::default());
    let mut out = Vec::new();
    for (i, (t, p)) in frames.iter().enumerate() {
        let d = decode_sync(p);
        r.push_sync(i as u64, *t, p.len() as u64 * 8, &d, |s| out.push(s.clone()));
    }
    for c in cockpits {
        r.push_cockpit(c);
    }
    (r.finish(), out)
}

#[test]
fn two_aircraft_author_and_cockpit_validation() {
    let mut frames = Vec::new();
    let mut cockpits = Vec::new();
    for k in 0..20u32 {
        let t = 1000 + k * 250;
        let ts = k as f64 * 0.25;
        let a = straight(0.0, 1000.0, 0.0, 200.0, ts);
        let trailer = a.pos;
        let b = straight(5000.0, 3000.0, 100.0, 150.0, ts);
        frames.push((t, sync(&[(10, a), (11, b)], trailer)));
        let mut p = cockpit_payload(&[(0, 0.0), (39, 0.0), (40, 1000.0), (52, 15.0), (53, -0.0)]);
        // Cockpit messages arrive a few ms before the sync, as observed.
        cockpits.push(cockpit::decode(t - 30, &p).unwrap());
        p.clear();
    }
    let (rep, samples) = run(&frames, cockpits);
    assert_eq!(rep.samples, 40);
    assert_eq!(rep.tracks, 2);
    assert_eq!(rep.uids, 2);
    assert_eq!(rep.author_uids, [10]);
    assert_eq!(rep.coverage.structural_failures, 0);
    assert_eq!(rep.coverage.packets_with_author_match, 20);
    assert_eq!(
        rep.coverage.physically_decoded_bits + rep.coverage.structural_only_bits + rep.coverage.trailer_bits + rep.coverage.unparsed_bits,
        rep.coverage.payload_bits
    );
    let k = &rep.kinematic_validation;
    assert!(k.displacement_residual_m.p99.unwrap() < 0.5, "{:?}", k.displacement_residual_m);
    assert!(k.heading_minus_track_abs_deg.p99.unwrap() < 0.2);
    assert!(k.author_displacement_residual_m.n > 0);
    let v = &rep.cockpit_validation;
    // The first cockpit message precedes every author sample: no bracket, not compared.
    assert_eq!(v.matched, 19);
    assert_eq!(v.pairing_offset_ms.p50, Some(30.0));
    assert_eq!(v.altimeter_param_id, Some(40));
    assert_eq!(v.altimeter_unit, Some("m"));
    assert_eq!(v.altitude_within_2m_fraction, Some(1.0));
    assert!(v.roll_deg.abs_error.max.unwrap() < 0.1);
    assert!(v.pitch_deg.abs_error.max.unwrap() < 0.1);

    let first = &samples[0];
    assert_eq!(first.track_id, "u10.s0");
    assert!(first.is_author);
    assert!(first.flags.contains(&"segment_start"));
    assert!(first.derived.fd_speed_mps.is_none());
    assert!((samples[2].derived.fd_speed_mps.unwrap() - 200.0).abs() < 0.5);
    assert!((first.velocity.speed_mps - 200.0).abs() < 0.25);
    assert!((first.orientation.bank_deg - 15.0).abs() < 0.1);
    assert!(!samples[1].is_author);
}

#[test]
fn gaps_and_jumps_split_segments_but_small_jumps_only_flag() {
    let mut frames = Vec::new();
    let mut t = 0u32;
    let mut x = 0.0f32;
    let push = |frames: &mut Vec<(u32, Vec<u8>)>, t: u32, x: f32| {
        let s = State { pos: [x, 500.0, 0.0], euler: 0, vel: pack_velocity(100.0, 0.0, 0.0) };
        frames.push((t, sync(&[(7, s)], [0.0; 3])));
    };
    for _ in 0..4 {
        push(&mut frames, t, x);
        t += 250;
        x += 25.0;
    }
    t += 6000; // gap > 5 s
    push(&mut frames, t, x);
    t += 250;
    x += 25.0 + 300.0; // 300 m unexplained: flag only
    push(&mut frames, t, x);
    t += 250;
    x += 25.0 + 5000.0; // 5 km: new segment
    push(&mut frames, t, x);
    t += 250;
    x += 25.0;
    push(&mut frames, t, x);
    // Timestamp reversal within the segment.
    push(&mut frames, t - 100, x + 15.0);

    let (rep, samples) = run(&frames, vec![]);
    let reasons: Vec<_> = rep.track_summaries.iter().map(|t| (t.track_id.as_str(), t.start_reason, t.samples)).collect();
    assert_eq!(reasons, [("u7.s0", "first_record", 4), ("u7.s1", "gap", 2), ("u7.s2", "position_jump", 3)]);
    assert_eq!(rep.track_summaries[1].gap_before_ms, Some(6250));
    assert_eq!(rep.track_summaries[1].flagged_jumps, 1);
    assert!(samples[5].flags.contains(&"position_jump"));
    assert!(samples[6].flags.contains(&"segment_start"));
    assert!(samples[8].flags.contains(&"timestamp_reversal"));
    assert_eq!(rep.kinematic_validation.timestamp_reversals, 1);
    assert!(rep.author_uids.is_empty());
    assert_eq!(rep.cockpit_validation.matched, 0);
}

#[test]
fn withheld_and_saturated_records() {
    let mut w = BitWriter::default();
    let fast = State { pos: [0.0, 9000.0, 0.0], euler: 0, vel: 0x3FF << 19 | 0x1FF << 9 };
    write_state(&mut w, Some(3), &fast);
    write_end(&mut w, [0.0; 3]);
    let first = w.bytes;
    let mut w = BitWriter::default();
    write_uid(&mut w, Some(3));
    w.bit(false).bit(true).bit(true);
    write_end(&mut w, [0.0; 3]);
    let (rep, samples) = run(&[(0, first), (250, w.bytes)], vec![]);
    assert_eq!(rep.coverage.withheld_entries, 1);
    assert_eq!(rep.track_summaries[0].withheld_records, 1);
    assert_eq!(rep.track_summaries[0].saturated_speed_samples, 1);
    assert!(samples[0].flags.contains(&"speed_saturated"));
    assert!(samples[0].velocity.saturated);
}

#[test]
fn feet_altimeter_on_full_altitude_id_is_selected() {
    let mut frames = Vec::new();
    let mut cockpits = Vec::new();
    for k in 0..10u32 {
        let s = straight(0.0, 3884.0, 0.0, 150.0, k as f64 * 0.25);
        let tr = s.pos;
        frames.push((k * 250, sync(&[(1, s)], tr)));
        let ft = 3884.0 * 3.28;
        let p = cockpit_payload(&[(40, ft % 10_000.0), (42, ft)]);
        cockpits.push(cockpit::decode(k * 250 + 20, &p).unwrap());
    }
    let (rep, _) = run(&frames, cockpits);
    assert_eq!(rep.cockpit_validation.altimeter_param_id, Some(42));
    assert_eq!(rep.cockpit_validation.altimeter_unit, Some("ft"));
    assert_eq!(rep.cockpit_validation.altitude_within_2m_fraction, Some(1.0));
}

#[test]
fn cockpit_decoder_rejects_bad_bodies() {
    assert!(cockpit::decode(0, &[0x03, 0x58, 0x4B, 0xD0, 0x80]).is_none()); // other message id
    assert!(cockpit::decode(0, &[0x03, 0x58]).is_none());
    // Declared length shorter than the count requires.
    let mut w = BitWriter::default();
    w.u16(0x5803).u16(0xD04A).varint(16).u16(3);
    assert!(cockpit::decode(0, &w.bytes).is_none());
    let p = cockpit_payload(&[(0, 120.5), (53, -3.0)]);
    let c = cockpit::decode(42, &p).unwrap();
    assert_eq!(c.object_id, 0x5803);
    assert_eq!(c.get(0), Some(120.5));
    assert_eq!(c.get(53), Some(-3.0));
    assert_eq!(c.get(40), None);
}

#[test]
fn flight_export_end_to_end() {
    let mut stream = Vec::new();
    for k in 0..8u32 {
        let t = 5000 + k * 250;
        let s = straight(100.0, 800.0, -50.0, 120.0, k as f64 * 0.25);
        let tr = s.pos;
        let mut w = BitWriter::default();
        write_state(&mut w, Some(42), &s);
        write_state(&mut w, None, &straight(-900.0, 1200.0, 10.0, 90.0, k as f64 * 0.25));
        write_end(&mut w, tr);
        stream.extend(packet(2, Some(t), &w.bytes));
        stream.extend(packet(4, None, &cockpit_payload(&[(40, 800.0), (52, 15.0), (53, 0.0), (39, 0.0)])));
    }
    stream.extend(packet(0, Some(9000), &[]));
    let file = replay(MAGIC_18C1C, &zstd(&stream), true);

    let ex = analyze_flight("fx.wrpl", &file, Options::default(), TrackConfig::default(), |_| Ok(())).unwrap();
    assert_eq!(ex.manifest.status, "ok");
    assert_eq!(ex.report.samples, 16);
    assert_eq!(ex.report.tracks, 2);
    assert_eq!(ex.report.author_uids, [42]);
    assert_eq!(ex.report.cockpit_validation.matched, 8);

    let dir = std::env::temp_dir().join(format!("sdl-test-{}-flight", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("fx.wrpl");
    std::fs::write(&input, &file).unwrap();
    let (_, tracks_path, summary_path) = export_flight(&input, &file, &dir.join("out"), Options::default()).unwrap();
    assert_eq!(std::fs::read(&input).unwrap(), file);

    let lines: Vec<serde_json::Value> =
        std::fs::read_to_string(&tracks_path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 16);
    let a = &lines[0];
    assert_eq!(a["track_id"], "u42.s0");
    assert_eq!(a["uid"], 42);
    assert_eq!(a["is_author"], true);
    assert_eq!(a["timestamp_ms"], 5000);
    assert_eq!(a["format_magic"], "0x00018C1C");
    assert_eq!(a["game_version"], "2.59.0.86");
    assert_eq!(a["replay_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(a["position"]["y_m"], 800.0);
    assert_eq!(a["altitude_msl_m"], 800.0);
    assert!(a["derived"]["fd_speed_mps"].is_null());
    assert!(lines[2]["derived"]["fd_speed_mps"].as_f64().is_some());
    assert_eq!(a["evidence"]["position"], "VERIFIED");
    assert_eq!(a["evidence"]["derived"], "DERIVED");
    assert_eq!(a["evidence"]["track_identity"], "EXPERIMENTAL");
    assert_eq!(lines[1]["track_id"], "u43.s0");
    assert_eq!(lines[1]["evidence"]["author"], "UNKNOWN");
    assert!(a["raw"]["euler_hex"].as_str().unwrap().starts_with("0x"));
    assert!(a["velocity"]["speed_mps"].as_f64().unwrap() > 119.0);

    let s: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&summary_path).unwrap()).unwrap();
    assert_eq!(s["schema"], "static57.sdl.flight-tracks/0.1");
    assert_eq!(s["tracks"], 2);
    assert_eq!(s["track_summaries"].as_array().unwrap().len(), 2);
    assert_eq!(s["cockpit_validation"]["altimeter_param_id"], 40);
    let _ = std::fs::remove_dir_all(&dir);
}
