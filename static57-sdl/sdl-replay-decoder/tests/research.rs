//! Research reports end to end (synthetic replays: reader behaviour only).
mod common;

use common::*;
use sdl_replay_decoder::evidence::{sha256_hex, Evidence};
use sdl_replay_decoder::export::export_research;
use sdl_replay_decoder::report;
use sdl_replay_decoder::research::{self, ResearchData};
use sdl_replay_decoder::strikemaster::{database_designator, weapon_core, Databases};
use sdl_replay_decoder::survey;

const WEAPON_EID: u32 = (2 << 22) | 700;

/// Author uid 10 (f-5e) flies 30 s, fires at uid 20, which is then killed.
/// `vehicle` names the author's aircraft.
fn sortie(vehicle: &str, peak: f32) -> Vec<u8> {
    let mut s = Vec::new();
    s.extend(packet(6, Some(100), &ecs_create(1, 1, "aircraft+player_unit", true, &unit_data(10, [0.0, 3000.0, 0.0], &[vehicle, "t1_player01_0", "custom1"]))));
    s.extend(packet(6, None, &ecs_create(2, 1, "", false, &unit_data(20, [8000.0, 3000.0, 0.0], &["mig_23ml", "t2_player02_0", "default"]))));
    for k in 0..120u32 {
        let t = 1000 + k * 250;
        let ts = k as f64 * 0.25;
        let a = State { pos: [peak * ts as f32, 3000.0, 0.0], euler: pack_euler(0.0, 0.0, 10.0), vel: pack_velocity(peak as f64, 0.0, 0.0) };
        let tr = a.pos;
        let b = State { pos: [8000.0, 3000.0, 0.0], euler: 0, vel: pack_velocity(100.0, 0.0, 0.0) };
        let mut w = BitWriter::default();
        write_state(&mut w, Some(10), &a);
        if k <= 72 {
            write_state(&mut w, Some(20), &b);
        }
        write_end(&mut w, tr);
        s.extend(packet(2, Some(t), &w.bytes));
        if k % 2 == 0 {
            s.extend(packet(4, None, &cockpit_payload(&[(40, 3000.0), (52, 10.0), (53, 0.0), (39, 0.0)])));
        }
        if k == 40 {
            s.extend(packet(6, None, &ecs_create(WEAPON_EID, 2, "rocket", true, &store_data(tr))));
        }
        if (41..=70).contains(&k) {
            let f = (k - 40) as f32 * 0.25;
            let launch = peak * 10.0;
            let pos = [launch + (8000.0 - launch) * (k - 40) as f32 / 30.0, 3000.0, 0.0];
            s.extend(packet(4, None, &shells_payload(WEAPON_EID, pos, [peak as f64 + 400.0, 0.0, 0.0], f as f64)));
        }
        if k == 71 {
            s.extend(packet(4, None, &kill_payload(20, 10, vehicle, "us_aim9j_sidewinder")));
            s.extend(packet(4, None, &damage_payload(20, vehicle, 10)));
        }
    }
    s.extend(packet(0, Some(33_000), &[]));
    replay(MAGIC_18C1C, &zstd(&s), true)
}

fn data(vehicle: &str, peak: f32) -> (Vec<u8>, ResearchData) {
    let f = sortie(vehicle, peak);
    let d = research::run("fixture.wrpl", &f, None).unwrap().data;
    (f, d)
}

#[test]
fn provenance_and_replay_identity() {
    let (f, d) = data("f-5e", 200.0);
    assert_eq!(d.schema, "static57.sdl.research-data/0.4");
    assert_eq!(d.replay.sha256, sha256_hex(&f));
    assert_eq!(d.replay.size_bytes, f.len() as u64);
    assert_eq!(d.replay.format_magic, "0x00018C1C");
    assert_eq!(d.replay.game_version, "2.59.0.86");
    assert_eq!(d.replay.game_version_evidence, Evidence::Experimental);
    assert_eq!(d.replay.status, "ok");
    let p = &d.strike_master.proposals[0];
    assert_eq!(p.replay_sha256, d.replay.sha256);
    assert_eq!(p.replay_file, "fixture.wrpl");
    assert_eq!(p.status, "proposed_for_review");
    assert!(p.first_packet_index > 0);
}

#[test]
fn tracks_identity_and_measurements() {
    let (_, d) = data("f-5e", 200.0);
    let a = d.tracks.iter().find(|t| t.uid == 10).unwrap();
    assert_eq!(a.role, "author");
    assert_eq!(a.identity.vehicle.as_deref(), Some("f-5e"));
    assert_eq!(a.identity.confidence, "battle_log_confirmed");
    assert_eq!(a.identity.evidence, Evidence::Verified);
    assert_eq!(a.identity.loadout.as_deref(), Some("custom1"));
    assert_eq!(a.samples, 120);
    assert_eq!(a.sample_coverage, Some(1.0));
    assert_eq!(a.altitude_m.max, Some(3000.0));
    assert!((a.speed_mps.max.unwrap() - 200.0).abs() < 0.3);
    let g = a.maneuver.as_ref().unwrap();
    assert!((g.load_factor_g.p50.unwrap() - 1.0).abs() < 0.05);
    let b = d.tracks.iter().find(|t| t.uid == 20).unwrap();
    assert_eq!(b.role, "remote");
    assert!(b.maneuver.is_none(), "remote aircraft get no maneuver figures");
    assert_eq!(b.identity.vehicle.as_deref(), Some("mig_23ml"));
    assert_eq!(b.identity.confidence, "spawn_match");
    assert_eq!(b.identity.evidence, Evidence::Derived);
    assert_eq!(b.destruction.as_ref().unwrap().weapon.as_deref(), Some("us_aim9j_sidewinder"));
    assert_eq!(d.identity.kill_records_vehicle_match, 1);
    assert_eq!(d.identity.damage_records_vehicle_match, 1);
    let au = d.author.as_ref().unwrap();
    assert_eq!(au.vehicles, ["f-5e"]);
    assert_eq!(au.weapons_released, 1);
    assert_eq!(d.combat.author_kills.len(), 1);
    assert_eq!(d.cockpit_validation.altimeter_unit, Some("m"));
}

#[test]
fn weapon_results_carry_status_and_limitations() {
    let (_, d) = data("f-5e", 200.0);
    let w = &d.weapons.events[0];
    assert_eq!(w.launcher.status, "creation_record");
    assert_eq!(w.launcher.evidence, Evidence::Verified);
    assert_eq!(w.outcome.class, "confirmed_kill");
    assert_eq!(w.weapon_name.as_deref(), Some("us_aim9j_sidewinder"));
    assert_eq!(w.trajectory.samples, 30);
    assert_eq!(d.weapons.stats.shells_exact, 30);
    assert!(!d.weapons.limitations.is_empty());
    assert!(d.findings.iter().any(|f| f.evidence == Evidence::Derived && f.statement.contains("us_aim9j_sidewinder")));
    let p = &d.strike_master.proposals[0];
    assert_eq!(p.target_vehicle.as_deref(), Some("mig_23ml"));
    assert!(p.target_distance_at_launch_m.is_some());
    assert!(p.complete_record);
    assert_eq!(p.outcome, "confirmed_kill");
    assert_eq!(p.launcher_evidence, Evidence::Verified);
    assert!(p.caution.contains("not a launch envelope"));
}

#[test]
fn missing_data_stays_null_and_is_labelled() {
    // No cockpit, weapons or battle log: a bare flight.
    let mut s = Vec::new();
    for k in 0..12u32 {
        let t = 1000 + k * 250 + if k >= 6 { 2000 } else { 0 };
        let mut w = BitWriter::default();
        write_state(&mut w, Some(5), &State { pos: [50.0 * k as f32, 500.0, 0.0], euler: 0, vel: pack_velocity(100.0, 0.0, 0.0) });
        write_end(&mut w, [50.0 * k as f32, 500.0, 0.0]);
        s.extend(packet(2, Some(t), &w.bytes));
    }
    s.extend(packet(0, Some(9000), &[]));
    let d = research::run("bare.wrpl", &replay(MAGIC_18C1C, &zstd(&s), true), None).unwrap().data;
    assert_eq!(d.cockpit_validation.altimeter_param_id, None);
    assert_eq!(d.tracks[0].identity.vehicle, None);
    assert_eq!(d.tracks[0].identity.confidence, "unresolved");
    assert_eq!(d.tracks[0].identity.evidence, Evidence::Unknown);
    assert_eq!(d.tracks[0].internal_gaps_over_1s, 1);
    assert_eq!(d.missing_data.intervals_total, 1);
    assert_eq!(d.missing_data.intervals[0].gap.duration_ms, 2250);
    assert!(d.weapons.events.is_empty());
    assert!(d.strike_master.proposals.is_empty());
    assert!(d.unresolved_questions.iter().any(|q| q.contains("no cockpit")));
    let json = serde_json::to_value(&d).unwrap();
    assert!(json["cockpit_validation"]["altimeter_unit"].is_null());
    assert!(json["tracks"][0]["identity"]["vehicle"].is_null());
    let md = report::render(&d);
    assert!(md.contains("unresolved"));
    assert!(md.contains("unavailable"));
    assert!(!md.contains("NaN"));
    assert!(!md.contains("null"));
}

#[test]
fn field_coverage_never_claims_undecoded_families() {
    let (_, d) = data("f-5e", 200.0);
    let status = |f: &str| d.decoded_field_coverage.iter().find(|x| x.family == f).unwrap();
    for f in ["radar_sensors", "countermeasures", "localhost_8111_fusion"] {
        assert_eq!(status(f).status, "not_decoded", "{f}");
        assert_eq!(status(f).evidence, Evidence::Unknown, "{f}");
    }
    assert_eq!(status("weapon_identity").status, "partial");
    assert_eq!(status("aircraft_position_altitude").evidence, Evidence::Verified);
    assert_eq!(status("weapon_launcher").evidence, Evidence::Verified);
}

#[test]
fn markdown_agrees_with_json() {
    let (_, d) = data("f-5e", 200.0);
    let md = report::render(&d);
    assert!(md.contains(&d.replay.sha256));
    assert!(md.contains(&d.replay.game_version));
    for f in &d.findings {
        assert!(md.contains(&f.id), "{} missing", f.id);
        assert!(md.contains(&f.statement), "{} statement missing", f.id);
        assert!(md.contains(&format!("{:?}", f.evidence)));
    }
    for t in d.tracks.iter().filter(|t| t.role == "author") {
        assert!(md.contains(&t.track_id));
        assert!(md.contains(&report::num(t.altitude_m.max)));
        assert!(md.contains(&report::num(t.speed_mps.max)));
    }
    for q in &d.unresolved_questions {
        assert!(md.contains(q));
    }
    for f in &d.decoded_field_coverage {
        assert!(md.contains(f.family));
    }
    let w = &d.weapons.events[0];
    assert!(md.contains(&w.weapon_id));
    assert!(md.contains(w.weapon_name.as_deref().unwrap()));
    // Round trip: the JSON file holds the same figures the report prints.
    let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
    assert_eq!(v["findings"][0]["statement"], d.findings[0].statement);
    assert_eq!(v["replay"]["sha256"], d.replay.sha256);
}

#[test]
fn export_writes_files_and_leaves_the_replay_untouched() {
    let dir = std::env::temp_dir().join(format!("sdl-test-{}-research", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("fixture.wrpl");
    let file = sortie("f-5e", 200.0);
    std::fs::write(&input, &file).unwrap();
    let out = dir.join("out");
    let d = export_research(&input, &file, &out, None).unwrap();
    assert_eq!(std::fs::read(&input).unwrap(), file, "replay must be unchanged");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(out.join("research-data.json")).unwrap()).unwrap();
    assert_eq!(v["replay"]["sha256"], sha256_hex(&file));
    assert_eq!(v["tracks"].as_array().unwrap().len(), d.tracks.len());
    assert!(std::fs::read_to_string(out.join("research-report.md")).unwrap().starts_with("# SDL Research Report"));
    let line = std::fs::read_to_string(out.join("weapon_tracks.jsonl")).unwrap();
    let w: serde_json::Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert_eq!(w["samples"].as_array().unwrap().len(), 30);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cross_replay_analysis() {
    let (_, a) = data("f-5e", 200.0);
    let (_, b) = data("f-5e", 250.0);
    let (_, c) = data("mig_23ml", 300.0);
    let dirs = ["a".to_string(), "b".to_string(), "c".to_string()];
    let s = survey::analyze(&[a, b, c], &dirs);
    assert_eq!(s.schema, "static57.sdl.research-summary/0.4");
    assert_eq!(s.totals.replays, 3);
    assert_eq!(s.totals.replays_with_author, 3);
    assert_eq!(s.totals.weapons_named, 3);
    assert_eq!(s.game_versions, ["2.59.0.86"]);
    let f5 = s.vehicles.iter().find(|v| v.vehicle == "f-5e").unwrap();
    assert_eq!(f5.replays, 2);
    assert!((f5.max_speed_mps.unwrap() - 250.0).abs() < 0.5);
    assert!((f5.max_speed_low_mps.unwrap() - 200.0).abs() < 0.5);
    let w = &s.weapons[0];
    assert_eq!(w.weapon_name, "us_aim9j_sidewinder");
    assert_eq!(w.shots, 3);
    assert_eq!(w.launchers, ["f-5e", "mig_23ml"]);
    assert!(w.caution.contains("not a launch envelope"));
    assert_eq!(s.replays[2].report_dir, "c");
    assert!(!s.recommendations.is_empty());
    let md = report::render_summary(&s);
    assert!(md.contains("us_aim9j_sidewinder"));
    assert!(md.contains("Controlled LAR shots"));
    assert!(md.contains("Independent 8111 validation"));
    assert_eq!(s.totals.launchers_verified, 3);
    assert_eq!(s.totals.outcome_classes.get("confirmed_kill"), Some(&3));
    assert!(!s.totals.outcome_classes.contains_key("validated_miss"));
    assert!(md.contains("SDL does not classify misses"));
    assert!(md.contains(&report::num(f5.max_speed_mps)));
    for r in &s.replays {
        assert!(md.contains(&r.file_name));
    }
}

#[test]
fn cross_replay_handles_empty_input() {
    let s = survey::analyze(&[], &[]);
    assert_eq!(s.totals.replays, 0);
    assert!(s.vehicles.is_empty());
    assert!(s.consistency.iter().all(|c| c.best.is_none()));
    assert!(report::render_summary(&s).contains("unavailable"));
}

#[test]
fn weapon_designators_and_database_matching() {
    assert_eq!(weapon_core("us_aim9l_sidewinder_default"), "aim9l");
    assert_eq!(weapon_core("us_agm_65b"), "agm65b");
    assert_eq!(weapon_core("su_r_73e"), "r73e");
    assert_eq!(weapon_core("uk_redtop"), "redtop");
    assert_eq!(database_designator("AIM-9L Sidewinder air-to-air missiles"), "aim9l");
    assert_eq!(database_designator("RB 99 air-to-air missiles"), "rb99");
    assert_eq!(database_designator("Red Top air-to-air missiles"), "redtop");

    let dir = std::env::temp_dir().join(format!("sdl-test-{}-db", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let aircraft = r#"{"aircraft":[{"id":"S57-AC-F5E","source_url":"https://wiki.warthunder.com/unit/f-5e"}]}"#;
    let weapons = r#"{"weapons":[{"id":"S57-WPN-001","name":"AIM-9J Sidewinder air-to-air missiles"},{"id":"S57-WPN-002","name":"R-60 missiles"},{"id":"S57-WPN-003","name":"R-60M missiles"}]}"#;
    std::fs::write(dir.join("static57-war-thunder-aircraft.json"), aircraft).unwrap();
    std::fs::write(dir.join("static57-war-thunder-weapons.json"), weapons).unwrap();
    let db = Databases::load(&dir).unwrap();
    assert_eq!(db.aircraft_id("f-5e"), Some("S57-AC-F5E"));
    assert_eq!(db.aircraft_id("mig_23ml"), None);
    assert_eq!(db.weapon_id("us_aim9j_sidewinder"), Some("S57-WPN-001"));
    assert_eq!(db.weapon_id("su_r_60"), Some("S57-WPN-002"));
    assert_eq!(db.weapon_id("su_r_60m"), Some("S57-WPN-003"), "R-60 Missiles must not swallow R-60M");
    assert_eq!(db.weapon_id("su_r_60mk"), None);
    assert!(!db.threats_present);

    let f = sortie("f-5e", 200.0);
    let d = research::run("x.wrpl", &f, Some(&db)).unwrap().data;
    let p = &d.strike_master.proposals[0];
    assert_eq!(p.s57_weapon_id.as_deref(), Some("S57-WPN-001"));
    assert_eq!(p.s57_aircraft_id.as_deref(), Some("S57-AC-F5E"));
    assert_eq!(p.database_match_evidence, Evidence::Experimental);
    assert!(d.strike_master.databases_loaded);
    // The databases are only read.
    assert_eq!(std::fs::read_to_string(dir.join("static57-war-thunder-aircraft.json")).unwrap(), aircraft);
    assert_eq!(std::fs::read_to_string(dir.join("static57-war-thunder-weapons.json")).unwrap(), weapons);
    let _ = std::fs::remove_dir_all(&dir);
}
