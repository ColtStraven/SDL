//! Compatibility tests against genuine War Thunder replays.
//!
//! Ignored by default because replays are not stored in the repository.
//! Run with:
//!
//! ```text
//! SDL_REPLAY_DIR=<folder with .wrpl files> cargo test --release -- --ignored
//! ```
//!
//! Without `SDL_REPLAY_DIR`, the local War Thunder replay folder
//! (`%LOCALAPPDATA%/WarThunder/Replays`) is used. Files are only read.

use sdl_replay_decoder::manifest::{analyze, Depth, Options};
use std::path::PathBuf;

fn replay_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("SDL_REPLAY_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("WarThunder").join("Replays")))?;
    dir.is_dir().then_some(dir)
}

#[test]
#[ignore = "needs genuine .wrpl files (SDL_REPLAY_DIR)"]
fn genuine_replays_frame_completely() {
    let dir = replay_dir().expect("no replay directory: set SDL_REPLAY_DIR");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wrpl")))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no .wrpl files in {}", dir.display());

    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let opts = Options { depth: Depth::Packets, ..Default::default() };
        let m = analyze(&name, &bytes, opts, |_, _| Ok(())).unwrap();
        assert_eq!(m.status, "ok", "{name}: {:?}", m.diagnostics);
        assert!(m.format.as_ref().unwrap().recognized, "{name}: unregistered format");
        let st = m.stream.as_ref().unwrap();
        assert_eq!(st.codec_matches_profile, Some(true), "{name}");
        let inf = st.inflation.as_ref().unwrap();
        assert_eq!(inf.trailing_bytes, 0, "{name}");
        assert_eq!(inf.compressed_consumed, inf.compressed_length, "{name}");
        let p = m.packets.as_ref().unwrap();
        assert!(p.all_bytes_accounted && !p.truncated, "{name}");
        assert_eq!(p.malformed_records, 0, "{name}");
        assert!(p.clock.monotonic, "{name}");
        let end = p.end_marker_index.expect("end marker");
        assert_eq!(end + 1, p.records, "{name}: end marker is not the last record");
        eprintln!("{name}: {} packets, {} unknown-type, sha256 {}", p.records, p.unknown_type_packets, m.input.sha256);
    }
    eprintln!("{} genuine replays framed completely from {}", files.len(), dir.display());
}

#[test]
#[ignore = "needs genuine .wrpl files (SDL_REPLAY_DIR)"]
fn genuine_research_reports() {
    use sdl_replay_decoder::{report, research, survey};
    let dir = replay_dir().expect("no replay directory: set SDL_REPLAY_DIR");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wrpl")))
        .collect();
    files.sort();
    let mut all = Vec::new();
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let d = research::run(&name, &bytes, None).unwrap().data;
        let w = &d.weapons.stats;
        assert_eq!(w.shells_failed, 0, "{name}");
        assert_eq!(w.shells_exact, w.shells_messages, "{name}: every ShellsData message must consume its declared length");
        // Unit identity must stay consistent with the battle log.
        let i = &d.identity;
        let (good, bad) = (i.kill_records_vehicle_match + i.damage_records_vehicle_match, i.kill_records_vehicle_mismatch + i.damage_records_vehicle_mismatch);
        assert!(bad <= good.max(20), "{name}: identity {good} good, {bad} bad");
        let md = report::render(&d);
        assert!(md.contains(&d.replay.sha256), "{name}");
        for f in &d.findings {
            assert!(md.contains(&f.statement), "{name}: {}", f.id);
        }
        assert!(md.lines().count() < 260, "{name}: report too long ({} lines)", md.lines().count());
        assert!(!md.contains("NaN") && !md.contains("inf "), "{name}");
        all.push(d);
    }
    let dirs: Vec<String> = all.iter().map(|d| d.replay.file_name.clone()).collect();
    let s = survey::analyze(&all, &dirs);
    assert_eq!(s.totals.replays as usize, files.len());
    assert_eq!(s.totals.replays_ok, s.totals.replays);
    assert!(s.totals.weapons_tracked > 0);
    eprintln!("{} replays: {} weapons, {} named, {} by author", files.len(), s.totals.weapons_tracked, s.totals.weapons_named, s.totals.weapons_by_author);
}

#[test]
#[ignore = "needs genuine .wrpl files (SDL_REPLAY_DIR)"]
fn genuine_flight_tracks_validate() {
    use sdl_replay_decoder::export::analyze_flight;
    use sdl_replay_decoder::tracks::TrackConfig;
    let dir = replay_dir().expect("no replay directory: set SDL_REPLAY_DIR");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wrpl")))
        .collect();
    files.sort();
    let (mut packets, mut samples, mut tracks) = (0u64, 0u64, 0u64);
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let ex = analyze_flight(&name, &bytes, Options::default(), TrackConfig::default(), |_| Ok(())).unwrap();
        let r = &ex.report;
        let c = &r.coverage;
        assert_eq!(c.structural_failures, 0, "{name}: type-2 structural failures");
        assert_eq!(c.structurally_decoded_packets, c.sync_packets, "{name}");
        assert_eq!(c.physically_decoded_bits + c.structural_only_bits + c.trailer_bits + c.unparsed_bits, c.payload_bits, "{name}");
        assert_eq!(r.kinematic_validation.timestamp_reversals, 0, "{name}");
        // The author's own flight model must be self-consistent where it moves.
        let a = &r.kinematic_validation.author_displacement_residual_m;
        if a.n > 100 {
            assert!(a.p50.unwrap() < 0.2, "{name}: author displacement residual p50 {:?}", a.p50);
        }
        let v = &r.cockpit_validation;
        if v.roll_deg.n > 500 {
            // Bound: ~1 quantum (0.18 deg) plus interpolation over 250 ms syncs.
            assert!(v.roll_deg.abs_error.p50.unwrap() < 0.5, "{name}: roll p50 {:?}", v.roll_deg.abs_error.p50);
            assert!(v.pitch_deg.abs_error.p50.unwrap() < 0.3, "{name}: pitch p50 {:?}", v.pitch_deg.abs_error.p50);
        }
        if v.altitude_m.n > 500 {
            assert!(v.altitude_m.abs_error.p50.unwrap() < 1.0, "{name}: altitude p50");
        }
        packets += c.sync_packets;
        samples += r.samples;
        tracks += r.tracks;
    }
    eprintln!("{} replays: {packets} type-2 packets, {samples} samples, {tracks} track segments", files.len());
}
