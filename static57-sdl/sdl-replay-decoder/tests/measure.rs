//! Measurement and statistics tests.
use sdl_replay_decoder::measure::*;

fn pt(t: u32, pos: [f32; 3], vel: [f32; 3]) -> Pt {
    let speed = (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt();
    Pt { t, packet: t as u64, pos, vel, speed, ground_flag: false, author: true, jump: false, saturated: false, engine: 7 }
}

/// Level straight flight along +X at `v` m/s, 4 Hz.
fn level(n: u32, v: f32) -> Vec<Pt> {
    (0..n).map(|i| pt(i * 250, [v * i as f32 * 0.25, 1000.0, 0.0], [v, 0.0, 0.0])).collect()
}

/// Level circular flight at `omega` rad/s.
fn circle(n: u32, v: f32, omega: f32) -> Vec<Pt> {
    let r = v / omega;
    (0..n)
        .map(|i| {
            let a = omega * i as f32 * 0.25;
            pt(i * 250, [r * a.sin(), 1000.0, r * (1.0 - a.cos())], [v * a.cos(), 0.0, v * a.sin()])
        })
        .collect()
}

#[test]
fn stat_summarises_and_locates_extremes() {
    let a = |t| At { timestamp_ms: t, packet_index: t as u64 };
    let s = stat(vec![(3.0, a(1)), (1.0, a(2)), (2.0, a(3)), (f64::NAN, a(4))]);
    assert_eq!(s.n, 3);
    assert_eq!((s.min, s.max, s.mean, s.p50), (Some(1.0), Some(3.0), Some(2.0), Some(2.0)));
    assert_eq!(s.min_at.unwrap().timestamp_ms, 2);
    assert_eq!(s.max_at.unwrap().packet_index, 1);
    let e = stat(vec![]);
    assert_eq!(e.n, 0);
    assert_eq!(e.max, None);
}

#[test]
fn round3_has_no_negative_zero() {
    assert_eq!(round3(1.23456), 1.235);
    assert_eq!(round3(-0.0001).to_string(), "0");
}

#[test]
fn level_flight_is_one_g() {
    let m = maneuver(&level(40, 200.0));
    let lf = m.load_factor_g;
    assert!(lf.n > 30);
    assert!((lf.max.unwrap() - 1.0).abs() < 0.001, "{lf:?}");
    assert_eq!(m.turn_rate_deg_s.max, Some(0.0));
    assert!(m.climb_rate_10s_mps.max.is_none() || m.climb_rate_10s_mps.max == Some(0.0));
}

#[test]
fn coordinated_turn_load_factor_and_rate() {
    // 200 m/s at 0.2 rad/s: 40 m/s2 lateral, about 4.2 g total.
    let m = maneuver(&circle(60, 200.0, 0.2));
    let expected = (40.0f64.powi(2) + G * G).sqrt() / G;
    assert!((m.load_factor_g.p50.unwrap() - expected).abs() < 0.05, "{:?} vs {expected}", m.load_factor_g.p50);
    assert!((m.turn_rate_deg_s.p50.unwrap() - 0.2f64.to_degrees()).abs() < 0.1);
    assert!(m.load_factor_cross_check_g.unwrap() < 0.05);
}

#[test]
fn climb_and_specific_excess_power() {
    let pts: Vec<Pt> = (0..100).map(|i| pt(i * 250, [100.0 * i as f32 * 0.25, 1000.0 + 25.0 * i as f32 * 0.25, 0.0], [100.0, 25.0, 0.0])).collect();
    let m = maneuver(&pts);
    assert!((m.climb_rate_10s_mps.max.unwrap() - 25.0).abs() < 0.01);
    assert!((m.specific_excess_power_5s_mps.p50.unwrap() - 25.0).abs() < 0.1);
}

#[test]
fn abrupt_change_is_excluded_and_reported() {
    let mut pts = level(40, 300.0);
    for p in pts.iter_mut().skip(20) {
        p.vel = [50.0, 0.0, 0.0];
        p.speed = 50.0;
    }
    let m = maneuver(&pts);
    assert!(m.abrupt_windows > 0);
    assert_eq!(m.abrupt_examples.len(), 1);
    assert!(m.load_factor_g.max.unwrap() < 1.01, "{:?}", m.load_factor_g.max);
}

#[test]
fn jumps_saturation_and_low_speed_are_not_measured() {
    let mut pts = level(40, 200.0);
    pts[10].jump = true;
    pts[20].saturated = true;
    let m = maneuver(&pts);
    let all = maneuver(&level(40, 200.0));
    assert!(m.load_factor_g.n < all.load_factor_g.n);
    assert_eq!(maneuver(&level(40, 10.0)).load_factor_g.n, 0);
    assert_eq!(maneuver(&[]).load_factor_g.n, 0);
    assert_eq!(maneuver(&[]).load_factor_cross_check_g, None);
}

#[test]
fn ground_transitions_are_debounced() {
    let mut pts = level(60, 80.0);
    for p in pts.iter_mut().take(20) {
        p.ground_flag = true;
    }
    pts[30].ground_flag = true; // one-sample flicker
    let t = transitions(&pts);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].kind, "liftoff_candidate");
    assert_eq!(t[0].timestamp_ms, 20 * 250);
}

#[test]
fn engine_startup_is_not_an_event_but_loss_is() {
    let mut pts = level(30, 100.0);
    for (i, p) in pts.iter_mut().enumerate() {
        p.engine = if i < 5 { i as u8 } else if i < 20 { 7 } else { 8 };
    }
    let t = transitions(&pts);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].detail.as_deref(), Some("7 -> 8"));
}

#[test]
fn gaps_are_found() {
    let mut pts = level(10, 100.0);
    for p in pts.iter_mut().skip(5) {
        p.t += 3000;
    }
    let g = gaps(&pts, 1000);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].duration_ms, 3250);
    assert_eq!(g[0].from_ms, 1000);
}

#[test]
fn robust_z_flags_extremes_only() {
    let z = robust_z(&[1.0, 1.1, 0.9, 1.05, 0.95, 9.0]).unwrap();
    assert!(z[5] > 3.5);
    assert!(z[0].abs() < 1.0);
    assert!(robust_z(&[1.0, 1.0, 1.0, 1.0]).is_none());
    assert!(robust_z(&[1.0, 2.0]).is_none());
}
