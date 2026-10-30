//! Telemetry capture, synchronization and validation.
#[path = "common/flightfx.rs"]
mod flightfx;

use flightfx::{flight, steady};
use sdl_replay_decoder::cockpit::{self, CockpitParams};
use sdl_replay_decoder::report::render_validation;
use sdl_replay_decoder::simcapture::{write_capture, SimConfig};
use sdl_replay_decoder::sync::{interpolate, synchronize};
use sdl_replay_decoder::systems::{self, analyze, attach_cockpit, clock_seconds, line_fit, load_factor, pearson, AuthorSample};
use sdl_replay_decoder::telemetry::{self, default_endpoints, http_get, load, parse_http, record, utc_stamp, Command, RecordConfig, CAPTURE_FILE, SESSION_FILE};
use sdl_replay_decoder::validate::{validate, ReplayView};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sdl-telemetry-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn view<'a>(samples: &'a [AuthorSample], kills: Vec<f64>) -> ReplayView<'a> {
    ReplayView { file_name: "fixture.wrpl", sha256: "00".into(), game_version: None, start_utc: None, start_unix_s: None, span_s: 0.0, author: samples, kills_s: kills }
}

fn sim(name: &str, samples: &[AuthorSample], cfg: &SimConfig) -> telemetry::Capture {
    let d = temp(name);
    write_capture(samples, cfg, &d).unwrap();
    load(&d).unwrap()
}

/// Rewrite a capture's metadata as a real recording, to test the labels.
fn mark_real(dir: &Path, utc_ms: u64, vehicle_ok: bool) -> telemetry::Capture {
    let p = dir.join(SESSION_FILE);
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    v["synthetic"] = false.into();
    v["started_utc_ms"] = utc_ms.into();
    std::fs::write(&p, v.to_string()).unwrap();
    if !vehicle_ok {
        let c = dir.join(CAPTURE_FILE);
        let s = std::fs::read_to_string(&c).unwrap().replace("\\\"type\\\":\\\"f-5e\\\"", "\\\"type\\\":\\\"other\\\"");
        std::fs::write(&c, s).unwrap();
    }
    load(dir).unwrap()
}

#[test]
fn http_bodies_by_length_chunks_and_close() {
    assert_eq!(parse_http(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}xx").unwrap(), (200, b"{}".to_vec()));
    assert_eq!(parse_http(b"HTTP/1.1 200 OK\r\n\r\n{\"a\":1}").unwrap(), (200, b"{\"a\":1}".to_vec()));
    assert_eq!(parse_http(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n1\r\n \r\n0\r\n\r\n").unwrap(), (200, b"{} ".to_vec()));
    assert_eq!(parse_http(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").unwrap().0, 404);
    assert!(parse_http(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n{}").is_err());
    assert!(parse_http(b"garbage").is_err());
}

#[test]
fn utc_stamp_is_civil_time() {
    assert_eq!(utc_stamp(1_792_795_650_000), "20261023_224730");
}

/// A stand-in for the game's server. Only answers GET, and logs every request.
fn serve(stop: Arc<AtomicBool>, seen_write: Arc<AtomicU32>, log: Arc<std::sync::Mutex<Vec<String>>>) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let mut states = 0u32;
        while !stop.load(Ordering::Relaxed) {
            let Ok((mut s, _)) = l.accept() else {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            };
            s.set_nonblocking(false).unwrap();
            let mut buf = [0u8; 2048];
            let k = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..k]).into_owned();
            if !req.starts_with("GET ") {
                seen_write.fetch_add(1, Ordering::Relaxed);
            }
            let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
            log.lock().unwrap().push(path.clone());
            let endpoint = path.split('?').next().unwrap();
            states += (endpoint == "/state") as u32;
            let (status, body) = match endpoint {
                "/state" if states % 2 == 0 => ("200 OK", "{\"valid\":false}".to_string()),
                "/state" => ("200 OK", "{\"valid\":true,\"H, m\":1000.0,\"IAS, km/h\":360}".to_string()),
                "/indicators" => ("200 OK", "{\"valid\":true,\"type\":\"f-5e\",\"compass\":90.0}".to_string()),
                "/map_info.json" => ("200 OK", "{\"map_min\":[\"-1000.0\",\"-1000.0\"],\"map_max\":[\"1000.0\",\"1000.0\"],\"grid_steps\":[\"100\",\"100\"]}".to_string()),
                "/map_obj.json" => ("200 OK", "[{\"type\":\"aircraft\",\"icon\":\"Player\",\"x\":0.5,\"y\":0.25,\"dx\":1,\"dy\":0}]".to_string()),
                "/hudmsg" => ("200 OK", "{\"events\":[],\"damage\":[{\"id\":5,\"msg\":\"a shot down b\",\"sender\":\"\",\"enemy\":false,\"mode\":\"\"}]}".to_string()),
                _ => ("404 Not Found", String::new()),
            };
            let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        }
    });
    port
}

#[test]
fn recorder_captures_raw_responses_and_marks() {
    let (stop, writes, log) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)), Arc::new(std::sync::Mutex::new(Vec::new())));
    let port = serve(stop.clone(), writes.clone(), log.clone());
    let dir = temp("record");
    let cfg = RecordConfig { out_dir: dir.clone(), host: "127.0.0.1".into(), port, duration_s: Some(1.5), endpoints: default_endpoints() };
    let (tx, rx) = mpsc::channel();
    tx.send(Command::Mark("wheels up".into())).unwrap();
    let meta = record(&cfg, rx, |_| {}).unwrap();
    stop.store(true, Ordering::Relaxed);
    assert_eq!(meta.stop_reason.as_deref(), Some("duration reached"));
    assert!(!meta.synthetic);
    assert_eq!(writes.load(Ordering::Relaxed), 0, "the recorder only sends GET");

    let c = load(&dir).unwrap();
    assert_eq!(c.partial_lines, 0);
    assert_eq!(c.marks.len(), 1);
    assert_eq!(c.marks[0].note, "wheels up");
    let st = &c.endpoints["state"];
    assert!(st.requests >= 5, "{}", st.requests);
    assert!(st.valid_true > 0, "{st:?}");
    assert_eq!(st.valid_false + st.valid_true + st.errors, st.requests, "{st:?}");
    assert_eq!(c.state.len() as u64, st.valid_true, "invalid readings are not measurements");
    assert_eq!(c.state[0].get("H, m"), Some(1000.0));
    assert_eq!(c.indicators[0].vehicle.as_deref(), Some("f-5e"));
    assert!(c.state[0].half_rtt_s >= 0.0);
    assert_eq!(c.map_info.as_ref().unwrap().map_min, [-1000.0, -1000.0], "numeric strings parse");
    assert_eq!(c.map_obj[0].x, 0.5);
    assert_eq!(c.hud.len(), 1, "a repeated id counts once");
    assert_eq!(c.hud[0].time_s, None);
    assert!(log.lock().unwrap().iter().any(|p| p.contains("lastDmg=5")), "the HUD cursor advances");
    assert!(std::fs::read_to_string(dir.join(CAPTURE_FILE)).unwrap().contains("IAS, km/h\\\":360"), "bodies are kept raw");
    // Writing into an existing recording is refused.
    assert!(record(&cfg, mpsc::channel().1, |_| {}).is_err());
}

#[test]
fn recorder_logs_errors_when_the_game_is_closed() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let dir = temp("closed");
    let cfg = RecordConfig { out_dir: dir.clone(), host: "127.0.0.1".into(), port, duration_s: Some(0.5), endpoints: default_endpoints() };
    let (tx, rx) = mpsc::channel();
    let meta = record(&cfg, rx, |_| {}).unwrap();
    drop(tx);
    assert!(meta.ended_utc_ms.is_some());
    let c = load(&dir).unwrap();
    let st = &c.endpoints["state"];
    assert!(st.errors >= 1 && st.errors <= 3, "backs off: {}", st.errors);
    assert!(st.first_error.as_deref().unwrap().contains("connect"));
    assert!(c.state.is_empty());
}

#[test]
fn http_get_reports_a_refused_connection() {
    let a = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    assert!(http_get(&a, "127.0.0.1", "/state").unwrap_err().starts_with("connect"));
}

#[test]
fn a_cut_off_last_line_is_tolerated() {
    let s = flight(100.0, 100.0, "f-5e");
    let d = temp("cut");
    write_capture(&s, &SimConfig::default(), &d).unwrap();
    let p = d.join(CAPTURE_FILE);
    let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
    f.write_all(b"{\"seq\":99,\"endpoint\":\"st").unwrap();
    drop(f);
    assert_eq!(load(&d).unwrap().partial_lines, 1);
    assert!(write_capture(&s, &SimConfig::default(), &d).is_err(), "a capture is never overwritten");
}

#[test]
fn nibbles_and_clock_fields() {
    let mut s = flight(0.0, 1.0, "x").remove(0);
    s.controls = [0x12, 0x34, 0, 0, 0, 0xAB, 0];
    assert_eq!((s.nibble(0), s.nibble(1), s.nibble(2), s.nibble(3)), (1, 2, 3, 4));
    assert_eq!((s.nibble(10), s.nibble(11)), (10, 11));
    assert_eq!(clock_seconds(9.166667, 10.0, 13.0), Some(9.0 * 3600.0 + 613.0));
    assert_eq!(clock_seconds(12.5, 0.0, 0.0), Some(0.0), "a 12-hour dial");
    s.boost_raw = Some(32767);
    assert!((s.throttle().unwrap() - 1.1).abs() < 1e-9);
}

#[test]
fn load_factor_in_level_flight_and_a_turn() {
    let mk = |t: u32, p: [f64; 3], bank: f64| {
        let mut s = flight(0.0, 1.0, "x").remove(0);
        s.t_ms = t;
        s.position = p;
        s.heading_deg = 0.0;
        s.pitch_deg = 0.0;
        s.bank_deg = bank;
        s
    };
    let (a, b, c) = (mk(0, [0.0, 1000.0, 0.0], 0.0), mk(250, [50.0, 1000.0, 0.0], 0.0), mk(500, [100.0, 1000.0, 0.0], 0.0));
    assert!((load_factor(&a, &b, &c).unwrap() - 1.0).abs() < 1e-9);
    // Rising 1 m/s more each 250 ms: 4 m/s^2 up, wings level.
    let c2 = mk(500, [100.0, 1000.25, 0.0], 0.0);
    assert!((load_factor(&a, &b, &c2).unwrap() - (1.0 + 4.0 / 9.81)).abs() < 1e-9);
    let far = mk(900, [100.0, 1000.0, 0.0], 0.0);
    assert_eq!(load_factor(&a, &b, &far), None, "a gap");
    let jump = mk(500, [100.0, 2000.0, 0.0], 0.0);
    assert_eq!(load_factor(&a, &b, &jump), None, "a position jump is not flight");
}

#[test]
fn cockpit_attaches_within_the_limit_only() {
    let mut s = flight(0.0, 1.0, "x");
    s.iter_mut().for_each(|x| x.cockpit.clear());
    let c = vec![CockpitParams { timestamp_ms: 260, object_id: 0, params: vec![(1, 2.0)] }];
    attach_cockpit(&mut s, &c, 20);
    assert_eq!(s[1].cockpit, vec![(1, 2.0)]);
    assert!(s[0].cockpit.is_empty());
    assert_eq!(cockpit::id::COMPASS, 64);
}

#[test]
fn systems_checks_agree_on_a_consistent_flight() {
    let r = analyze(&flight(100.0, 300.0, "f-5e"));
    assert_eq!(r.samples, 1200);
    assert_eq!(r.gear_vs_cockpit.agree, r.gear_vs_cockpit.n);
    assert_eq!(r.gear_transitions, 1);
    assert!(r.throttle_abs_error.p99.unwrap() < 1e-3);
    assert_eq!(r.throttle_nibble_agree.agree, r.throttle_nibble_agree.n);
    assert!(r.compass.abs_error_deg.p99.unwrap() < 1e-3);
    assert!((r.clock.rate.unwrap() - 1.0).abs() < 1e-3);
    assert_eq!(r.fuel.len(), 1);
    assert!((r.fuel[0].rate_per_s + 0.5).abs() < 0.01);
    assert_eq!(r.engine_states.get(&7), Some(&1200));
    assert!(r.nibble_nonzero.contains_key("n11"));
}

#[test]
fn a_refuel_starts_a_new_fuel_record() {
    let mut s = flight(0.0, 200.0, "f-5e");
    for x in s.iter_mut().skip(400) {
        for p in x.cockpit.iter_mut() {
            if p.0 == cockpit::id::FUEL {
                p.1 += 500.0;
            }
        }
    }
    let r = analyze(&s);
    assert_eq!(r.fuel.len(), 2);
    assert!(r.fuel[0].refilled);
}

#[test]
fn every_candidate_is_labelled_short_of_verified() {
    use sdl_replay_decoder::evidence::Evidence;
    assert!(systems::CANDIDATES.iter().all(|c| c.evidence != Evidence::Verified), "two aircraft do not verify a fleet");
    assert!(systems::CANDIDATES.iter().any(|c| c.evidence == Evidence::Unknown), "unexercised fields stay unknown");
}

#[test]
fn clock_recovers_a_known_offset_and_drift() {
    let s = flight(5_000.0, 1_200.0, "f-5e");
    let c = sim("sync", &s, &SimConfig { offset_s: 4_321.25, drift_ppm: 40.0, ..Default::default() });
    let r = synchronize(&c, &s, &[]);
    assert_eq!(r.quality, "established");
    assert_eq!(r.method, Some("cockpit_clock"));
    let want = 4_321.25 + 40e-6 * r.reference_t_s;
    assert!((r.offset_s.unwrap() - want).abs() < 0.05, "{:?} vs {want}", r.offset_s);
    assert!((r.drift_ppm.unwrap() - 40.0).abs() <= r.drift_resolution_ppm.unwrap(), "{:?} {:?}", r.drift_ppm, r.drift_resolution_ppm);
    let motion = r.methods.iter().find(|m| m.method == "motion_correlation").unwrap();
    assert_eq!(motion.status, "ok");
    assert!((motion.offset_s.unwrap() - want).abs() < 0.05, "{:?}", motion.offset_s);
    assert_eq!(r.methods.iter().find(|m| m.method == "gear_events").unwrap().status, "cross_check");
    assert!(r.usable() && !r.discontinuity);
}

#[test]
fn a_large_drift_is_measured() {
    let s = flight(5_000.0, 1_200.0, "f-5e");
    let c = sim("drift", &s, &SimConfig { offset_s: 100.0, drift_ppm: 1_000.0, ..Default::default() });
    let r = synchronize(&c, &s, &[]);
    assert!((r.drift_ppm.unwrap() - 1_000.0).abs() < 100.0, "{:?}", r.drift_ppm);
    assert_eq!(r.quality, "established");
}

#[test]
fn motion_alone_can_align_a_flight_with_no_cockpit_clock() {
    let s = flight(1_000.0, 900.0, "f-5e");
    let c = sim("motion", &s, &SimConfig { offset_s: -250.75, include_clock: false, ..Default::default() });
    let r = synchronize(&c, &s, &[]);
    assert_eq!(r.quality, "established");
    assert_eq!(r.method, Some("motion_correlation"));
    assert!((r.offset_s.unwrap() + 250.75).abs() < 0.05, "{:?}", r.offset_s);
    assert!(r.limitations.iter().any(|l| l.contains("Motion correlation only")));
    assert_eq!(r.methods[0].status, "unavailable");
}

#[test]
fn clock_and_motion_disagreeing_is_a_conflict() {
    let s = flight(1_000.0, 900.0, "f-5e");
    let d = temp("conflict");
    write_capture(&s, &SimConfig { offset_s: 100.0, ..Default::default() }, &d).unwrap();
    // The capture's clock reads 30 s late.
    let mut c = load(&d).unwrap();
    for o in &mut c.indicators {
        if let Some(x) = o.fields.get_mut("clock_sec") {
            let total = *x + 30.0;
            *x = total % 60.0;
            if total >= 60.0 {
                *o.fields.get_mut("clock_min").unwrap() += 1.0;
            }
        }
    }
    let r = synchronize(&c, &s, &[]);
    assert_eq!(r.quality, "conflict", "{:?}", r.methods);
    assert!(!r.usable());
}

#[test]
fn a_steady_flight_cannot_be_aligned_by_motion() {
    let s = steady(1_000.0, 600.0);
    let c = sim("steady", &s, &SimConfig { offset_s: 50.0, noise: 0.0, ..Default::default() });
    let r = synchronize(&c, &s, &[]);
    assert_eq!(r.quality, "failed");
    assert_eq!(r.offset_s, None);
    assert!(!r.usable());
    assert!(!r.limitations.is_empty());
}

#[test]
fn a_game_pause_shows_as_a_discontinuity() {
    let s = flight(1_000.0, 1_200.0, "f-5e");
    let d = temp("pause");
    write_capture(&s, &SimConfig { offset_s: 10.0, include_clock: false, ..Default::default() }, &d).unwrap();
    let mut c = load(&d).unwrap();
    // Halfway through, the capture falls 1.5 s behind, as if the game had paused.
    for o in c.state.iter_mut().chain(c.indicators.iter_mut()) {
        if o.t_s > 1_590.0 {
            o.t_s += 1.5;
        }
    }
    let r = synchronize(&c, &s, &[]);
    assert!(r.discontinuity, "{:?}", r.windows);
    assert!(!r.usable());
}

#[test]
fn an_unrelated_capture_does_not_align() {
    let s = flight(1_000.0, 600.0, "f-5e");
    let other: Vec<AuthorSample> = flight(1_000.0, 600.0, "f-5e").into_iter().map(|mut x| {
        x.position[1] = 3_000.0 + 400.0 * (x.t_ms as f64 / 91_000.0).cos();
        x.velocity[1] = 2.0;
        x.pitch_deg = 5.0 * (x.t_ms as f64 / 17_000.0).cos();
        x.bank_deg = 20.0 * (x.t_ms as f64 / 5_000.0).sin();
        x
    }).collect();
    let c = sim("unrelated", &other, &SimConfig { offset_s: 0.0, include_clock: false, ..Default::default() });
    let r = synchronize(&c, &s, &[]);
    assert_ne!(r.quality, "established", "{:?}", r.methods);
}

#[test]
fn interpolation_stops_at_gaps_and_wraps_angles() {
    let mut s = flight(0.0, 2.0, "x");
    s[0].heading_deg = 179.0;
    s[1].heading_deg = -179.0;
    let i = interpolate(&s, 0.125).unwrap();
    assert!((i.heading_deg.abs() - 180.0).abs() < 1e-9);
    s[2].t_ms = 2_000;
    assert_eq!(interpolate(&s, 0.4), None);
    assert_eq!(interpolate(&s, 99.0), None);
}

#[test]
fn synthetic_is_never_independent_validation() {
    let s = flight(1_000.0, 900.0, "f-5e");
    let c = sim("label", &s, &SimConfig { offset_s: 12.0, noise: 0.0, ..Default::default() });
    let v = validate(&c, &view(&s, vec![]));
    assert!(!v.independent_validation);
    assert!(v.status.starts_with("SYNTHETIC"));
    assert!(v.verified_discoveries.is_empty());
    assert!(v.comparisons.iter().all(|x| x.evidence == sdl_replay_decoder::evidence::Evidence::Experimental));
    let md = render_validation(&v);
    assert!(md.contains("SYNTHETIC"));
    assert!(md.contains("## 2. Time alignment"));
}

#[test]
fn a_confirmed_real_capture_is_independent() {
    let s = flight(1_000.0, 900.0, "f-5e");
    let d = temp("real");
    write_capture(&s, &SimConfig { offset_s: 12.0, noise: 0.0, ..Default::default() }, &d).unwrap();
    let c = mark_real(&d, 1_800_000_000_000, true);
    let mut r = view(&s, vec![]);
    r.start_unix_s = Some(1_800_000_100.0);
    r.span_s = 900.0;
    let v = validate(&c, &r);
    assert_eq!(v.association.class, "confirmed");
    assert!(v.independent_validation, "{}", v.status);
    assert_eq!(v.comparisons[0].evidence, sdl_replay_decoder::evidence::Evidence::Derived);
    assert!(!v.verified_discoveries.is_empty());
}

#[test]
fn a_capture_from_another_day_is_withheld() {
    let s = flight(1_000.0, 900.0, "f-5e");
    let d = temp("otherday");
    write_capture(&s, &SimConfig { offset_s: 12.0, ..Default::default() }, &d).unwrap();
    let c = mark_real(&d, 1_800_000_000_000, true);
    let mut r = view(&s, vec![]);
    r.start_unix_s = Some(1_700_000_000.0);
    r.span_s = 900.0;
    let v = validate(&c, &r);
    assert_eq!(v.association.class, "unconfirmed");
    assert!(!v.independent_validation);
    assert!(v.comparisons.is_empty());
    assert!(v.data_gaps.iter().any(|g| g.contains("withheld")));
}

#[test]
fn a_different_vehicle_is_not_confirmed() {
    let s = flight(1_000.0, 900.0, "f-5e");
    let d = temp("vehicle");
    write_capture(&s, &SimConfig { offset_s: 12.0, ..Default::default() }, &d).unwrap();
    let c = mark_real(&d, 1_800_000_000_000, false);
    let mut r = view(&s, vec![]);
    r.start_unix_s = Some(1_800_000_100.0);
    r.span_s = 900.0;
    let v = validate(&c, &r);
    assert_eq!(v.association.vehicle_match, Some(false));
    assert_eq!(v.association.class, "unconfirmed");
    assert!(!v.independent_validation);
}

#[test]
fn airspeed_kinds_stay_separate_and_wind_is_not_error() {
    let s = flight(1_000.0, 900.0, "f-5e");
    let c = sim("wind", &s, &SimConfig { offset_s: 12.0, noise: 0.0, wind_mps: 10.0, ..Default::default() });
    let v = validate(&c, &view(&s, vec![]));
    let get = |n: &str| v.comparisons.iter().find(|x| x.name == n).unwrap();
    assert!((get("ground_speed_minus_tas").bias.unwrap() + 10.0).abs() < 0.5);
    assert!(get("ground_speed_minus_tas").note.as_deref().unwrap().contains("not TAS"));
    assert_eq!(get("indicated_airspeed").replay_source, "cockpit id 0");
    assert!(get("altitude").abs_error.p99.unwrap() < 5.0, "{:?}", get("altitude").abs_error);
    assert!(get("heading").abs_error.p50.unwrap() < 0.5);
    assert!(get("load_factor").n > 0);
}

#[test]
fn feet_altimeters_are_tested_against_both_feet() {
    let s = flight(1_000.0, 900.0, "f-5e");
    let c = sim("feet", &s, &SimConfig { offset_s: 12.0, noise: 0.0, ..Default::default() });
    let v = validate(&c, &view(&s, vec![]));
    let a = v.altimeter_scale.iter().find(|a| a.field == "altitude_10k").unwrap();
    assert_eq!(a.nearest, "game_foot_1_over_3.28");
    assert!(a.error_as_game_foot_m.unwrap() < a.error_as_exact_foot_m.unwrap());
    assert!((a.metres_per_unit.unwrap() - 1.0 / 3.28).abs() < 1e-3);
}

#[test]
fn the_map_fit_finds_the_transform() {
    let s = flight(1_000.0, 900.0, "f-5e");
    let c = sim("map", &s, &SimConfig { offset_s: 12.0, noise: 0.0, ..Default::default() });
    let v = validate(&c, &view(&s, vec![]));
    let h = v.horizontal.as_ref().unwrap();
    assert!(h.accepted, "{:?}", h.hypotheses);
    assert_eq!(h.best, "map_x = +X, map_y = -Z");
    assert!(h.best_rms_m < 10.0);
    assert!(h.direction_error_deg.p50.unwrap() < 1.0);
    assert!((h.free_scale_x.unwrap() - h.expected_scale_x).abs() < 1e-7);
}

#[test]
fn field_scan_reports_scale_without_naming_meaning() {
    let s = flight(1_000.0, 900.0, "f-5e");
    let c = sim("fields", &s, &SimConfig { offset_s: 12.0, noise: 0.0, ..Default::default() });
    let v = validate(&c, &view(&s, vec![]));
    let m = v.field_matches.iter().find(|m| m.replay_field == "throttle_boost" && m.telemetry_field == "state:throttle 1, %").expect("throttle match");
    assert!((m.scale - 100.0).abs() < 1.0, "{}", m.scale);
    assert!(v.field_matches.iter().all(|m| m.pearson_r.abs() >= 0.95));
    assert!(v.telemetry_fields.iter().any(|f| f == "state:H, m"));
    assert!(v.candidate_checks.iter().any(|c| c.candidate == "gear_bit (expected)" && c.telemetry_field.as_deref() == Some("state:gear, %")));
    let g = v.candidate_checks.iter().find(|c| c.candidate == "gear_bit").unwrap();
    assert!(g.pearson_r.unwrap().abs() > 0.99, "{g:?}");
    assert!(v.candidate_checks.iter().any(|c| c.candidate == "nibble_0" && c.distinct_values == 1), "an unexercised field is shown");
    assert!(v.altimeter_by_vehicle.iter().any(|a| a.nearest == "game_foot_1_over_3.28"));
    assert!(render_validation(&v).contains("Distinct values"));
}

#[test]
fn marks_map_into_replay_time() {
    let s = flight(1_000.0, 900.0, "f-5e");
    let d = temp("marks");
    write_capture(&s, &SimConfig { offset_s: 900.0, ..Default::default() }, &d).unwrap();
    let mut c = load(&d).unwrap();
    c.marks.push(telemetry::Mark { t_s: 200.0, utc_ms: 0, note: "sync waggle".into() });
    let v = validate(&c, &view(&s, vec![]));
    let m = &v.marks[0];
    assert!((m.replay_t_s.unwrap() - 1_100.0).abs() < 0.1);
    assert!(m.altitude_m.is_some());
    assert!(render_validation(&v).contains("sync waggle"));
}

#[test]
fn hud_messages_cross_check_replay_kills() {
    let s = flight(1_000.0, 900.0, "f-5e");
    let d = temp("hud");
    write_capture(&s, &SimConfig { offset_s: 900.0, ..Default::default() }, &d).unwrap();
    let mut c = load(&d).unwrap();
    c.hud.push(telemetry::HudEvent { t_s: 300.4, id: 1, msg: "a shot down b".into(), time_s: Some(1_200.0) });
    let r = synchronize(&c, &s, &[1_200.0]);
    let h = r.methods.iter().find(|m| m.method == "hud_events").unwrap();
    assert_eq!(h.status, "cross_check");
    assert!((h.offset_s.unwrap() - 0.4).abs() < 0.1);
    assert!(h.note.contains("HUD time field"));
}

#[test]
fn statistics_helpers() {
    let p = [(0.0, 1.0), (1.0, 3.0), (2.0, 5.0)];
    assert!((pearson(&p).unwrap() - 1.0).abs() < 1e-12);
    assert_eq!(line_fit(&p), Some((2.0, 1.0)));
    assert_eq!(pearson(&[(1.0, 1.0), (1.0, 2.0), (1.0, 3.0)]), None);
}
