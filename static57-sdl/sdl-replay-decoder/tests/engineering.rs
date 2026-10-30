//! Strike Master engineering findings, cross-replay opportunities and the
//! research index (synthetic replays: reader behaviour only).
mod common;

use common::*;
use sdl_replay_decoder::engineering::{self, EngineeringFinding, Nature, Priority, ENGINEERING_SCHEMA};
use sdl_replay_decoder::evidence::Evidence;
use sdl_replay_decoder::export::export_research;
use sdl_replay_decoder::index::{self, INDEX_FILE, LATEST_FILE, VALIDATION_FILE};
use sdl_replay_decoder::report;
use sdl_replay_decoder::research::{self, ResearchData, DATA_FILE, REPORT_FILE};
use sdl_replay_decoder::strikemaster::{Databases, AIRCRAFT_DB, WEAPONS_DB};
use sdl_replay_decoder::survey;
use sdl_replay_decoder::systems::FuelObservation;
use std::fs;
use std::path::{Path, PathBuf};

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

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sdl-eng-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn base() -> ResearchData {
    data("f-5e", 200.0).1
}

fn find<'a>(d: &'a ResearchData, code: &str) -> Option<&'a EngineeringFinding> {
    d.engineering.findings.iter().find(|f| f.finding_id.starts_with(code))
}

fn all_text(f: &EngineeringFinding) -> String {
    format!("{} {} {} {} {} {}", f.title, f.observation, f.derived_analysis, f.limitations.join(" "), f.engineering_relevance, f.recommended_follow_up.as_deref().unwrap_or(""))
}

fn stretch(rate: f64, seconds: f64) -> FuelObservation {
    FuelObservation { vehicle: "f-5e".into(), start: 1000.0, end: 1000.0 + rate * seconds, duration_s: seconds, rate_per_s: rate, refilled: false, mean_throttle: Some(0.9), mean_altitude_m: Some(2000.0), mean_speed_mps: Some(250.0), airborne_fraction: 1.0, above_100_percent_fraction: 0.1, evidence: Evidence::Experimental }
}

fn validation(sha: &str, independent: bool) -> serde_json::Value {
    serde_json::json!({
        "independent_validation": independent,
        "provenance": { "replay_sha256": sha, "capture_dir": "telemetry/x", "capture_sha256": "abcd1234ef" },
        "sync": { "quality": "established" },
        "comparisons": [
            { "name": "indicated_airspeed", "bias": 0.0, "abs_error": { "p50": 0.2, "p99": 1.0 } },
            { "name": "ground_speed_minus_tas", "bias": 0.7, "abs_error": { "p50": 0.8, "p99": 2.0 } }
        ]
    })
}

/// A database folder listing the fixture's aircraft without its weapon.
fn database(name: &str) -> Databases {
    let dir = temp(name);
    fs::write(dir.join(AIRCRAFT_DB), r#"{"aircraft":[{"id":"S57-AC-1","source_url":"https://wiki.warthunder.com/unit/f-5e","compatible_weapons":["S57-WPN-9"]}]}"#).unwrap();
    fs::write(dir.join(WEAPONS_DB), r#"{"weapons":[{"id":"S57-WPN-2","name":"AIM-9J Sidewinder air-to-air missiles"}]}"#).unwrap();
    Databases::load(&dir).unwrap()
}

#[test]
fn every_finding_has_the_required_structure() {
    let d = base();
    assert!(!d.engineering.findings.is_empty());
    assert_eq!(d.engineering.schema, ENGINEERING_SCHEMA);
    assert_eq!(d.engineering.status, "proposed_for_review");
    for f in &d.engineering.findings {
        assert!(!f.finding_id.is_empty() && f.finding_id.ends_with(&d.replay.sha256[..8]), "{}", f.finding_id);
        assert!(!f.observation.is_empty(), "{}", f.finding_id);
        assert!(!f.derived_analysis.is_empty(), "{}", f.finding_id);
        assert!(!f.engineering_relevance.is_empty(), "{}", f.finding_id);
        assert!(["high", "medium", "low"].contains(&f.confidence));
        assert!(["MAP", "NAV", "HUD", "FLT", "WPN", "STRK", "LAR", "OPS", "AIRCRAFT_DB", "WEAPONS_DB"].contains(&f.subsystem), "{}", f.subsystem);
        assert_eq!(f.replay_reference.sha256, d.replay.sha256);
        assert_eq!(f.replay_reference.file_name, d.replay.file_name);
        assert_eq!(f.game_version.as_deref(), Some(d.replay.game_version.as_str()));
        assert_eq!(f.aircraft_id.as_deref(), Some("f-5e"));
    }
}

#[test]
fn finding_ids_are_stable_and_ordered_by_priority() {
    let (a, b) = (base(), base());
    let ids = |d: &ResearchData| d.engineering.findings.iter().map(|f| f.finding_id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&a), ids(&b));
    let p: Vec<Priority> = a.engineering.findings.iter().map(|f| f.priority).collect();
    let mut sorted = p.clone();
    sorted.sort();
    assert_eq!(p, sorted);
}

#[test]
fn json_shape_matches_the_schema() {
    let d = base();
    let v = serde_json::to_value(&d).unwrap();
    let f = &v["engineering"]["findings"][0];
    for key in ["finding_id", "subsystem", "aircraft_id", "weapon_id", "observation", "measured_values", "units", "evidence_class", "confidence", "limitations", "engineering_relevance", "priority", "recommended_follow_up", "replay_reference", "telemetry_reference", "game_version"] {
        assert!(f.get(key).is_some(), "missing {key}");
    }
    assert!(["VERIFIED", "DERIVED", "REPORTED", "EXPERIMENTAL", "UNKNOWN"].contains(&f["evidence_class"].as_str().unwrap()));
    assert!(["HIGH", "MEDIUM", "LOW"].contains(&f["priority"].as_str().unwrap()));
    assert!(f["telemetry_reference"].is_null());
    // Older fields stay, so existing readers keep working.
    assert!(v["findings"].is_array() && v["weapons"].is_object() && v["systems"].is_object());
}

#[test]
fn a_missing_measurement_is_null_not_zero() {
    let mut d = base();
    d.cockpit_validation.matched = 500;
    d.engineering = engineering::analyze(&d, None, None);
    let f = find(&d, "HUD-INSTR").expect("instrument finding");
    assert_eq!(f.measured_values["ias_estimate_abs_error_p50"], None);
    let v = serde_json::to_value(f).unwrap();
    assert!(v["measured_values"]["ias_estimate_abs_error_p50"].is_null());
    assert_eq!(f.nature, Nature::DataGap);
    assert!(f.derived_analysis.contains("indicated airspeed"), "{}", f.derived_analysis);
}

#[test]
fn markdown_and_json_agree() {
    let d = base();
    let md = report::render(&d);
    assert!(md.contains("## Strike Master Engineering Findings"));
    assert!(md.contains("## Executive summary"));
    assert!(md.contains("## Recommended research priorities"));
    assert!(md.contains("## 2. Main observations"));
    assert!(md.contains("## 9. Important data limitations"));
    for l in &d.engineering.executive_summary {
        assert!(md.contains(l), "{l}");
    }
    for f in &d.engineering.findings {
        for part in [&f.finding_id, &f.observation, &f.derived_analysis, &f.engineering_relevance] {
            assert!(md.contains(part.as_str()), "{part}");
        }
        for l in &f.limitations {
            assert!(md.contains(l.as_str()), "{l}");
        }
        assert!(md.contains(engineering::evidence_label(f.evidence_class)));
        assert!(md.contains(f.priority.label()));
    }
}

#[test]
fn language_rules_hold_for_weapons_with_unknown_outcomes() {
    let mut d = base();
    for w in &mut d.weapons.events {
        w.launcher.is_author = true;
        w.outcome.class = "unknown";
        w.target.status = "unavailable";
        w.geometry = None;
    }
    d.engineering = engineering::analyze(&d, None, None);
    let f = d.engineering.findings.iter().find(|f| f.subsystem == "WPN").expect("weapon finding");
    assert!(f.derived_analysis.contains("says nothing about guidance or tracking"));
    assert!(f.limitations.iter().any(|l| l.contains("It is not a miss")));
    for f in &d.engineering.findings {
        let t = all_text(f).to_lowercase();
        for banned in ["missed", "miss rate", "confirmed miss", "hit probability", "true speed", "tracking failure", "bug"] {
            assert!(!t.contains(banned), "{banned} in {t}");
        }
        assert!(!t.replace("not a miss", "").contains(" miss "), "{t}");
    }
}

#[test]
fn launch_geometry_claims_no_envelope() {
    let mut d = base();
    let mut w = d.weapons.events[0].clone();
    assert!(w.geometry.is_some());
    w.launcher.is_author = true;
    d.weapons.events[0].launcher.is_author = true;
    w.weapon_id = "w2".into();
    d.weapons.events.push(w);
    d.engineering = engineering::analyze(&d, None, None);
    let f = d.engineering.findings.iter().find(|f| f.subsystem == "LAR").expect("LAR finding");
    assert_eq!(f.nature, Nature::ResearchOpportunity);
    assert!(f.limitations.iter().any(|l| l.contains("No launch-envelope boundary or success rate")));
    assert!(f.limitations.iter().any(|l| l.contains("LAR calibration readiness is not claimed")));
    assert!(!f.replay_reference.events.is_empty());
    assert!(f.replay_reference.events.iter().all(|e| e.packet_index.is_some() && e.timestamp_ms.is_some()));
}

#[test]
fn a_category_with_no_evidence_is_absent() {
    let mut d = base();
    d.author = None;
    d.engineering = engineering::analyze(&d, None, None);
    assert!(d.engineering.findings.is_empty());
    assert!(report::render(&d).contains("No engineering findings"));
    let d = base();
    assert!(find(&d, "FLT-FUEL").is_none(), "no fuel data, no fuel finding");
    assert!(find(&d, "HUD-ALTFT").is_none(), "no feet altimeter");
    assert!(find(&d, "ACDB-MATCH").is_none(), "no database loaded");
}

#[test]
fn only_a_decoding_failure_is_a_software_error() {
    let d = base();
    assert!(d.engineering.findings.iter().all(|f| f.nature != Nature::SoftwareError));
    let mut d = base();
    d.replay.status = "partial";
    d.engineering = engineering::analyze(&d, None, None);
    let f = find(&d, "SDL-DECODE").unwrap();
    assert_eq!((f.nature, f.priority), (Nature::SoftwareError, Priority::High));
    assert!(d.engineering.executive_summary.iter().any(|l| l.contains("software error")));
}

#[test]
fn fuel_priority_and_class_follow_the_evidence() {
    let mut d = base();
    d.systems.fuel = vec![stretch(-1.0, 700.0)];
    d.engineering = engineering::analyze(&d, None, None);
    let f = find(&d, "FLT-FUEL").unwrap();
    assert_eq!(f.priority, Priority::Low);
    assert_eq!(f.evidence_class, Evidence::Experimental);
    assert!(f.limitations.iter().any(|l| l.contains("kg is assumed")));

    d.systems.fuel = vec![stretch(-1.0, 700.0), stretch(-3.0, 400.0)];
    d.engineering = engineering::analyze(&d, None, None);
    let f = find(&d, "FLT-FUEL").unwrap();
    assert_eq!(f.priority, Priority::Medium);
    assert_eq!(f.nature, Nature::ResearchOpportunity);
    assert_eq!(f.measured_values["burn_ratio"], Some(3.0));
    assert_eq!(f.units["burn_max"], "kg/s");

    d.author.as_mut().unwrap().vehicles = vec!["ffa_p16".into()];
    d.engineering = engineering::analyze(&d, None, None);
    assert_eq!(find(&d, "FLT-FUEL").unwrap().evidence_class, Evidence::Derived);

    // Ground time and short stretches are not flight burn.
    d.systems.fuel = vec![stretch(-1.0, 60.0), FuelObservation { airborne_fraction: 0.1, ..stretch(-1.0, 700.0) }];
    d.engineering = engineering::analyze(&d, None, None);
    assert!(find(&d, "FLT-FUEL").is_none());
}

#[test]
fn unidentified_fields_stay_unknown() {
    let mut d = base();
    d.systems.nibble_nonzero.insert("n5".into(), 300);
    d.engineering = engineering::analyze(&d, None, None);
    let f = find(&d, "FLT-FIELDS").unwrap();
    assert_eq!(f.evidence_class, Evidence::Unknown);
    assert!(f.derived_analysis.contains("no meaning is assigned"));
    d.systems.nibble_nonzero.insert("n5".into(), 5);
    d.engineering = engineering::analyze(&d, None, None);
    assert!(find(&d, "FLT-FIELDS").is_none(), "too few samples");
}

#[test]
fn validation_joins_only_its_own_replay() {
    let mut d = base();
    let sha = d.replay.sha256.clone();
    assert!(engineering::matching_validation(Some(validation("other", true)), &sha).is_none());
    let v = engineering::matching_validation(Some(validation(&sha, true)), &sha).unwrap();
    d.engineering = engineering::analyze(&d, None, Some(&v));
    let f = find(&d, "HUD-SPEED").unwrap();
    let t = f.telemetry_reference.as_ref().unwrap();
    assert_eq!(t.capture_sha256.as_deref(), Some("abcd1234ef"));
    assert!(t.independent_validation);
    assert_eq!(f.measured_values["ground_speed_minus_tas_bias"], Some(0.7));
    assert!(all_text(f).contains("not a decoding error"));
    assert!(d.engineering.executive_summary.iter().any(|l| l.contains("independent comparison")));
    assert!(report::render(&d).contains("8111 capture abcd1234"));
    assert!(!report::render(&d).contains("no 8111 capture"));
}

#[test]
fn saved_validation_reaches_the_report_on_export() {
    let (f, d) = data("f-5e", 200.0);
    let dir = temp("export");
    let input = dir.join("in.wrpl");
    fs::write(&input, &f).unwrap();
    fs::write(dir.join(VALIDATION_FILE), validation(&d.replay.sha256, true).to_string()).unwrap();
    let e = export_research(&input, &f, &dir, None).unwrap();
    assert!(e.engineering.findings.iter().all(|x| x.telemetry_reference.is_some()));
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(dir.join(DATA_FILE)).unwrap()).unwrap();
    assert_eq!(saved["engineering"]["findings"][0]["telemetry_reference"]["capture_sha256"], "abcd1234ef");
}

#[test]
fn database_checks_report_without_calling_a_fault() {
    let (f, _) = data("f-5e", 200.0);
    let db = database("db1");
    let d = research::run("fixture.wrpl", &f, Some(&db)).unwrap().data;
    let w = d.engineering.findings.iter().find(|x| x.subsystem == "WEAPONS_DB").expect("weapon database finding");
    assert_eq!(w.nature, Nature::UnresolvedDiscrepancy);
    assert_eq!(w.priority, Priority::High, "a kill record names the weapon");
    assert_eq!(w.s57_weapon_id.as_deref(), Some("S57-WPN-2"));
    assert_eq!(w.s57_aircraft_id.as_deref(), Some("S57-AC-1"));
    assert!(w.observation.contains("does not list"));
    assert!(w.derived_analysis.contains("naming difference"));
    assert!(find(&d, "ACDB-MATCH").is_none());
}

#[test]
fn an_unknown_aircraft_and_weapon_are_gaps_in_the_database() {
    let dir = temp("db2");
    fs::write(dir.join(AIRCRAFT_DB), r#"{"aircraft":[{"id":"S57-AC-1","source_url":"https://wiki.warthunder.com/unit/other"}]}"#).unwrap();
    fs::write(dir.join(WEAPONS_DB), r#"{"weapons":[]}"#).unwrap();
    let db = Databases::load(&dir).unwrap();
    let (f, _) = data("f-5e", 200.0);
    let d = research::run("fixture.wrpl", &f, Some(&db)).unwrap().data;
    assert!(find(&d, "ACDB-MATCH").unwrap().observation.contains("matches none of the 1 aircraft"));
    let w = d.engineering.findings.iter().find(|x| x.subsystem == "WEAPONS_DB").unwrap();
    assert!(w.observation.contains("matches no database entry"));
    assert_eq!(w.priority, Priority::Medium);
}

#[test]
fn reports_call_replay_speed_ground_speed() {
    let d = base();
    let md = report::render(&d);
    assert!(!md.to_lowercase().contains("true speed"));
    assert!(md.contains("ground speed"));
    assert!(md.contains("Max ground speed m/s"));
}

#[test]
fn summary_lists_opportunities_with_scope() {
    let mut a = data("f-5e", 200.0).1;
    let mut b = data("f-5e", 210.0).1;
    assert_ne!(a.replay.sha256, b.replay.sha256);
    a.systems.fuel = vec![stretch(-1.0, 700.0)];
    b.systems.fuel = vec![stretch(-2.0, 700.0)];
    let c = survey::analyze(&[a.clone(), b], &["a".into(), "b".into()]);
    let o = &c.research_opportunities;
    assert_eq!(o.fuel[0].recordings, 2);
    assert_eq!(o.fuel[0].stretches, 2);
    assert!(!o.fuel[0].matched_to_8111);
    let fuel = o.opportunities.iter().find(|p| p.opportunity_id == "SMO-FUEL").unwrap();
    assert_eq!(fuel.scope, "multiple_recordings");
    assert_eq!(o.game_versions.iter().map(|g| g.replays).sum::<u64>(), 2);
    let version = o.opportunities.iter().find(|p| p.opportunity_id == "SMO-VERSION").unwrap();
    assert!(version.observation.contains("No statistical test"));
    assert!(o.opportunities.iter().all(|p| p.priority != Priority::High), "a coverage gap is not a defect");

    let single = survey::analyze(&[a], &["a".into()]);
    let fuel = single.research_opportunities.opportunities.iter().find(|p| p.opportunity_id == "SMO-FUEL").unwrap();
    assert_eq!(fuel.scope, "single_sortie");

    let md = report::render_summary(&c);
    assert!(md.contains("## Strike Master Research Opportunities"));
    for p in &o.opportunities {
        assert!(md.contains(p.opportunity_id) && md.contains(&p.observation) && md.contains(&p.uncertainty), "{}", p.opportunity_id);
        assert!(!p.coverage.is_empty() && !p.recommended_follow_up.is_empty());
    }
    let text = serde_json::to_string(o).unwrap().to_lowercase();
    assert!(!text.contains("hit probability") && !text.contains("miss rate"));
    assert!(serde_json::to_value(&c).unwrap()["research_opportunities"]["opportunities"].is_array());
}

fn dataset(dir: &Path, json: serde_json::Value) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(DATA_FILE), json.to_string()).unwrap();
}

#[test]
fn latest_follows_the_recording_time_not_the_file_time() {
    let root = temp("index");
    let new = serde_json::json!({ "decoder": { "version": "0.7.0" }, "replay": { "file_name": "#new.wrpl", "start_time_utc": "2026-10-30T21:49:54Z", "game_version": "2.59.0.92", "mission": "map_a" }, "author": { "vehicles": ["f_2a_adtw"] } });
    let old = serde_json::json!({ "replay": { "file_name": "#old.wrpl", "start_time_utc": "2026-10-20T00:00:00Z" } });
    // The newest recording is written first, so its file time is the oldest.
    dataset(&root.join("new"), new);
    std::thread::sleep(std::time::Duration::from_millis(30));
    dataset(&root.join("old"), old);
    let e = index::scan(&root).unwrap();
    assert_eq!(e[0].file_name, "#new.wrpl");
    let latest = index::render_latest(&e).unwrap();
    assert!(latest.contains("#new.wrpl") && latest.contains("f_2a_adtw") && latest.contains("(new/research-report.md)"));
    assert!(!latest.contains('\\'));
}

#[test]
fn old_datasets_and_missing_fields_are_still_listed() {
    let root = temp("old");
    dataset(&root.join("schema02"), serde_json::json!({ "schema": "static57.sdl.research-data/0.2", "replay": { "file_name": "#a.wrpl", "start_time_utc": "2026-10-23T00:00:00Z", "game_version": "2.59.0.80" } }));
    dataset(&root.join("bare"), serde_json::json!({}));
    fs::create_dir_all(root.join("not_a_report")).unwrap();
    let e = index::scan(&root).unwrap();
    assert_eq!(e.len(), 2);
    assert_eq!(e[0].file_name, "#a.wrpl");
    assert_eq!(e[1].recorded_utc, None, "no recording time sorts last");
    let md = index::render_index(&e);
    assert!(md.contains("unavailable") && md.contains("#a.wrpl") && md.contains("2 reports"));
    assert!(!md.contains("null"));
}

#[test]
fn no_recording_time_means_no_latest_file() {
    let root = temp("notime");
    dataset(&root.join("bare"), serde_json::json!({}));
    let f = index::write(&root).unwrap();
    assert!(f.latest.is_none());
    assert!(!root.join(LATEST_FILE).exists());
    assert!(root.join(INDEX_FILE).exists());
}

#[test]
fn index_shows_validation_state() {
    let root = temp("state");
    let d = serde_json::json!({ "replay": { "file_name": "#a.wrpl", "start_time_utc": "2026-10-23T00:00:00Z" } });
    for (name, body) in [("real", Some(r#"{"independent_validation":true}"#)), ("weak", Some(r#"{"independent_validation":false}"#)), ("broken", Some("{")), ("none", None)] {
        let dir = root.join(name);
        dataset(&dir, d.clone());
        if let Some(b) = body {
            fs::write(dir.join(VALIDATION_FILE), b).unwrap();
        }
    }
    let e = index::scan(&root).unwrap();
    let state = |n: &str| e.iter().find(|x| x.folder == n).unwrap().validation;
    assert_eq!(state("real"), "independent");
    assert_eq!(state("weak"), "not independent");
    assert_eq!(state("broken"), "unreadable");
    assert_eq!(state("none"), "none");
}

#[test]
fn index_writing_preserves_report_folders() {
    let root = temp("preserve");
    let (f, _) = data("f-5e", 200.0);
    let dir = root.join("flight");
    fs::create_dir_all(&dir).unwrap();
    let input = root.join("in.wrpl");
    fs::write(&input, &f).unwrap();
    export_research(&input, &f, &dir, None).unwrap();
    let before = fs::read(dir.join(REPORT_FILE)).unwrap();
    let w = index::write(&root).unwrap();
    assert_eq!(w.reports, 1);
    assert_eq!(fs::read(dir.join(REPORT_FILE)).unwrap(), before);
    let index_text = fs::read_to_string(root.join(INDEX_FILE)).unwrap();
    assert!(index_text.contains("flight/research-report.md"));
    assert!(root.join("flight/research-report.md").exists());
    assert!(root.join(LATEST_FILE).exists());
}

#[test]
fn custom_output_folders_keep_working() {
    assert_eq!(index::root_for(Path::new("research/flight")), Path::new("research"));
    assert_eq!(index::root_for(Path::new("mine")), Path::new("mine"), "a bare name never indexes the working directory");
    let root = temp("single");
    dataset(&root, serde_json::json!({ "replay": { "file_name": "#a.wrpl", "start_time_utc": "2026-10-23T00:00:00Z" } }));
    let e = index::scan(&root).unwrap();
    assert_eq!(e[0].folder, ".");
    assert!(index::render_index(&e).contains("(research-report.md)"));
    assert!(index::absolute(Path::new("x")).is_absolute());
}
