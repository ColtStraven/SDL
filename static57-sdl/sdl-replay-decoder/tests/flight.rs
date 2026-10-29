mod common;

use common::{pack_euler, pack_velocity};
use sdl_replay_decoder::flight::*;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn orientation_bit_layout() {
    let o = decode_orientation(0);
    assert_eq!((o.heading_deg, o.pitch_deg, o.bank_deg), (0.0, 0.0, 0.0));
    let o = decode_orientation(0x3FF << 19 | 0x1FF << 10 | 0x3FF);
    assert_eq!((o.heading_deg, o.pitch_deg, o.bank_deg), (180.0, 90.0, 180.0));
    let o = decode_orientation(1 << 31 | 1 << 30 | 1 << 29 | 1 << 19 | 1 << 10 | 1);
    assert!(close(o.heading_deg, -HEADING_QUANTUM_DEG, 1e-12));
    assert!(close(o.pitch_deg, -PITCH_QUANTUM_DEG, 1e-12));
    assert!(close(o.bank_deg, -BANK_QUANTUM_DEG, 1e-12));
}

#[test]
fn orientation_round_trip_within_quantum() {
    for &(h, p, b) in &[(37.2, 4.1, -61.0), (-179.0, -88.0, 179.5), (90.0, 0.0, 0.0), (-0.1, 45.0, -0.2)] {
        let o = decode_orientation(pack_euler(h, p, b));
        assert!(close(o.heading_deg, h, HEADING_QUANTUM_DEG / 2.0 + 1e-9), "{h} -> {}", o.heading_deg);
        assert!(close(o.pitch_deg, p, PITCH_QUANTUM_DEG / 2.0 + 1e-9));
        assert!(close(o.bank_deg, b, BANK_QUANTUM_DEG / 2.0 + 1e-9));
    }
}

#[test]
fn velocity_bit_layout() {
    // speed field 1023, rx = 1, ry = 0, all signs negative.
    let v = decode_velocity(1 << 31 | 1 << 30 | 1 << 29 | 0x3FF << 19 | 0x1FF << 9);
    assert!(v.saturated);
    assert_eq!(v.speed_mps, 500.0);
    assert!(close(v.vx_mps, -500.0, 1e-9));
    assert_eq!(v.vy_mps, 0.0);
    assert!(close(v.vz_mps, 0.0, 1e-9));
    assert!(!v.flag_bit18);
    let f = decode_velocity(1 << 18);
    assert!(f.flag_bit18);
    assert_eq!(f.speed_mps, 0.0);
}

#[test]
fn velocity_round_trip_and_unit_norm() {
    for &(vx, vy, vz) in &[(143.5f64, 0.0f64, -117.8f64), (-224.2, -0.1, 101.5), (58.3, 19.0, 319.8), (0.0, -150.0, 0.0)] {
        let v = decode_velocity(pack_velocity(vx, vy, vz));
        let s = (vx * vx + vy * vy + vz * vz).sqrt();
        assert!(close(v.speed_mps, s, SPEED_QUANTUM_MPS / 2.0 + 1e-9));
        let n = (v.vx_mps.powi(2) + v.vy_mps.powi(2) + v.vz_mps.powi(2)).sqrt();
        assert!(v.clamped || close(n, v.speed_mps, 1e-6 * v.speed_mps.max(1.0)));
        assert!(close(v.vx_mps, vx, s * 0.004 + 0.5), "vx {vx} -> {}", v.vx_mps);
        assert!(close(v.vy_mps, vy, s * 0.004 + 0.5));
        assert!(close(v.vz_mps, vz, s * 0.05 + 0.5), "vz {vz} -> {}", v.vz_mps);
        assert!(!v.saturated);
    }
}

#[test]
fn velocity_quantisation_overflow_is_clamped() {
    // rx = ry = 1 cannot be a unit vector.
    let v = decode_velocity(100 << 19 | 0x1FF << 9 | 0x1FF);
    assert!(v.clamped);
    assert_eq!(v.vz_mps, 0.0);
}

#[test]
fn heading_convention_matches_track() {
    // Nose along (cos h, 0, -sin h).
    assert!(close(track_deg(1.0, 0.0), 0.0, 1e-9));
    assert!(close(track_deg(0.0, -1.0), 90.0, 1e-9));
    assert!(close(track_deg(0.0, 1.0), -90.0, 1e-9));
    assert!(close(track_deg(-1.0, 0.0).abs(), 180.0, 1e-9));
}

#[test]
fn angle_diff_wraps() {
    assert_eq!(angle_diff_deg(179.0, -179.0), -2.0);
    assert_eq!(angle_diff_deg(-179.0, 179.0), 2.0);
    assert_eq!(angle_diff_deg(10.0, 10.0), 0.0);
    assert_eq!(angle_diff_deg(0.0, 180.0), 180.0);
}

#[test]
fn isa_density_ratio_reference_points() {
    assert!(close(isa_density_ratio(0.0), 1.0, 1e-12));
    assert!(close(isa_density_ratio(5000.0), 0.6012, 0.001));
    assert!(close(isa_density_ratio(11000.0), 0.2971, 0.001));
}
