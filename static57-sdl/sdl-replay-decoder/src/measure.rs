//! Pure measurement functions over a time-ordered aircraft track.
//!
//! All maneuver figures are DERIVED from decoded velocity or position over
//! fixed baselines, which attenuates peaks shorter than the baseline.

use crate::tracks::FlightSample;
use serde::Serialize;

pub const G: f64 = 9.80665;
/// Maneuver baseline.
pub const MANEUVER_BASELINE_MS: u32 = 1_000;
/// Sustained-climb baseline.
pub const CLIMB_BASELINE_MS: u32 = 10_000;
/// Specific-excess-power baseline.
pub const PS_BASELINE_MS: u32 = 5_000;
/// Below this speed direction is too coarse for maneuver figures.
pub const MIN_MANEUVER_SPEED_MPS: f64 = 30.0;
/// Speed change beyond this is a collision, break-up or impact, not flight
/// dynamics. Such windows are reported as events and excluded from statistics.
pub const ABRUPT_ACCEL_MPS2: f64 = 60.0;
/// Engine state byte reported as destroyed.
pub const ENGINE_DESTROYED: u8 = 8;
/// Engine state byte reported as running.
pub const ENGINE_RUNNING: u8 = 7;
/// Baselines may stretch by this fraction to find a sample.
const BASELINE_SLACK: f64 = 0.15;

/// Compact flight sample kept for measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pt {
    pub t: u32,
    pub packet: u64,
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub speed: f32,
    pub ground_flag: bool,
    pub author: bool,
    pub jump: bool,
    pub saturated: bool,
    /// First engine state byte, 255 when the record has no engine.
    pub engine: u8,
}

impl Pt {
    pub fn from_sample(s: &FlightSample) -> Pt {
        let v = &s.velocity;
        Pt {
            t: s.timestamp_ms,
            packet: s.packet_index,
            pos: [s.position.x_m as f32, s.position.y_m as f32, s.position.z_m as f32],
            vel: [v.vx_mps as f32, v.vy_mps as f32, v.vz_mps as f32],
            speed: v.speed_mps as f32,
            ground_flag: s.raw.ground_flag,
            author: s.is_author,
            jump: s.flags.contains(&"position_jump"),
            saturated: v.saturated,
            engine: s.raw.engine_states.first().copied().unwrap_or(255),
        }
    }

}

/// Where a value was measured.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct At {
    pub timestamp_ms: u32,
    pub packet_index: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Stat {
    pub n: u64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub mean: Option<f64>,
    pub p50: Option<f64>,
    pub p99: Option<f64>,
    pub min_at: Option<At>,
    pub max_at: Option<At>,
}

/// Round to three decimals so JSON and Markdown carry identical values.
pub fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0 + 0.0
}

pub fn stat(mut v: Vec<(f64, At)>) -> Stat {
    v.retain(|x| x.0.is_finite());
    if v.is_empty() {
        return Stat::default();
    }
    v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let n = v.len();
    let q = |p: f64| round3(v[((n - 1) as f64 * p).round() as usize].0);
    Stat {
        n: n as u64,
        min: Some(round3(v[0].0)),
        max: Some(round3(v[n - 1].0)),
        mean: Some(round3(v.iter().map(|x| x.0).sum::<f64>() / n as f64)),
        p50: Some(q(0.5)),
        p99: Some(q(0.99)),
        min_at: Some(v[0].1),
        max_at: Some(v[n - 1].1),
    }
}

fn at(p: &Pt) -> At {
    At { timestamp_ms: p.t, packet_index: p.packet }
}

/// Field statistics over a track.
pub fn field_stat(pts: &[Pt], f: impl Fn(&Pt) -> f64) -> Stat {
    stat(pts.iter().map(|p| (f(p), at(p))).collect())
}

pub fn altitude(p: &Pt) -> f64 {
    p.pos[1] as f64
}

/// Indices `(i, j)` where `j` is the first sample at least `baseline` after
/// `i`, within slack, with no jump in between.
fn windows(pts: &[Pt], baseline: u32) -> Vec<(usize, usize)> {
    let max = baseline + (baseline as f64 * BASELINE_SLACK) as u32;
    let mut out = Vec::new();
    let mut j = 0;
    for i in 0..pts.len() {
        j = j.max(i);
        while j < pts.len() && pts[j].t.saturating_sub(pts[i].t) < baseline {
            j += 1;
        }
        if j >= pts.len() {
            break;
        }
        if pts[j].t - pts[i].t <= max && !pts[i..=j].iter().any(|p| p.jump) {
            out.push((i, j));
        }
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Maneuver {
    pub baseline_s: f64,
    /// Magnitude of (acceleration + g up) / g.
    pub load_factor_g: Stat,
    pub turn_rate_deg_s: Stat,
    pub longitudinal_accel_mps2: Stat,
    /// Mean climb rate over 10 s: max is the best sustained climb.
    pub climb_rate_10s_mps: Stat,
    /// `dh/dt + (V/g) dV/dt` over 5 s.
    pub specific_excess_power_5s_mps: Stat,
    /// Windows excluded as collision, break-up or impact candidates.
    pub abrupt_windows: u64,
    pub abrupt_examples: Vec<At>,
    /// Median abs difference between load factor from velocity and from
    /// position second differences.
    pub load_factor_cross_check_g: Option<f64>,
}

fn load_factor(a: [f64; 3]) -> f64 {
    (a[0].powi(2) + (a[1] + G).powi(2) + a[2].powi(2)).sqrt() / G
}

pub fn maneuver(pts: &[Pt]) -> Maneuver {
    let usable = |p: &Pt| p.speed as f64 >= MIN_MANEUVER_SPEED_MPS && !p.saturated;
    let (mut lf, mut turn, mut accel, mut cross) = (vec![], vec![], vec![], vec![]);
    let (mut abrupt_windows, mut abrupt_examples) = (0u64, Vec::new());
    for (i, j) in windows(pts, MANEUVER_BASELINE_MS) {
        let (a, b) = (&pts[i], &pts[j]);
        if !usable(a) || !usable(b) {
            continue;
        }
        let dt = (b.t - a.t) as f64 / 1000.0;
        if a.engine == ENGINE_DESTROYED || b.engine == ENGINE_DESTROYED {
            continue;
        }
        if ((b.speed - a.speed) as f64 / dt).abs() > ABRUPT_ACCEL_MPS2 {
            abrupt_windows += 1;
            if abrupt_examples.last().is_none_or(|e: &At| b.t > e.timestamp_ms + 2_000) && abrupt_examples.len() < 5 {
                abrupt_examples.push(at(b));
            }
            continue;
        }
        let acc = [0, 1, 2].map(|k| (b.vel[k] - a.vel[k]) as f64 / dt);
        let n = load_factor(acc);
        lf.push((n, at(b)));
        accel.push(((b.speed - a.speed) as f64 / dt, at(b)));
        // Angle between velocity vectors: well defined in vertical flight,
        // unlike a horizontal track angle.
        let dot: f64 = (0..3).map(|k| a.vel[k] as f64 * b.vel[k] as f64).sum();
        let cos = dot / (a.speed as f64 * b.speed as f64);
        turn.push((cos.clamp(-1.0, 1.0).acos().to_degrees() / dt, at(b)));
        // Midpoint for the position second difference.
        if let Some(m) = pts[i..=j].iter().find(|p| p.t.abs_diff(a.t + (b.t - a.t) / 2) <= 30) {
            let h = dt / 2.0;
            let acc2 = [0, 1, 2].map(|k| (b.pos[k] as f64 - 2.0 * m.pos[k] as f64 + a.pos[k] as f64) / (h * h));
            cross.push((load_factor(acc2) - n).abs());
        }
    }
    let climb = windows(pts, CLIMB_BASELINE_MS)
        .into_iter()
        .map(|(i, j)| ((altitude(&pts[j]) - altitude(&pts[i])) / ((pts[j].t - pts[i].t) as f64 / 1000.0), at(&pts[j])))
        .collect();
    let energy = |p: &Pt| altitude(p) + (p.speed as f64).powi(2) / (2.0 * G);
    let ps = windows(pts, PS_BASELINE_MS)
        .into_iter()
        .filter(|&(i, j)| usable(&pts[i]) && usable(&pts[j]))
        .map(|(i, j)| ((energy(&pts[j]) - energy(&pts[i])) / ((pts[j].t - pts[i].t) as f64 / 1000.0), at(&pts[j])))
        .collect();
    cross.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Maneuver {
        baseline_s: MANEUVER_BASELINE_MS as f64 / 1000.0,
        load_factor_g: stat(lf),
        turn_rate_deg_s: stat(turn),
        longitudinal_accel_mps2: stat(accel),
        climb_rate_10s_mps: stat(climb),
        specific_excess_power_5s_mps: stat(ps),
        abrupt_windows,
        abrupt_examples,
        load_factor_cross_check_g: cross.get(cross.len() / 2).map(|v| round3(*v)),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Transition {
    /// `liftoff_candidate`, `touchdown_candidate`, `engine_state_change`.
    pub kind: &'static str,
    pub timestamp_ms: u32,
    pub packet_index: u64,
    pub altitude_m: f64,
    pub speed_mps: f64,
    pub detail: Option<String>,
}

/// A ground-flag change must persist this many samples (2 s at 4 Hz).
pub const TRANSITION_DEBOUNCE: usize = 8;

/// Debounced flight-state transitions.
pub fn transitions(pts: &[Pt]) -> Vec<Transition> {
    let mut out = Vec::new();
    let Some(first) = pts.first() else { return out };
    let mut state = first.ground_flag;
    let mut engine = first.engine;
    let mk = |kind, p: &Pt, detail| Transition {
        kind,
        timestamp_ms: p.t,
        packet_index: p.packet,
        altitude_m: round3(altitude(p)),
        speed_mps: round3(p.speed as f64),
        detail,
    };
    for i in 1..pts.len() {
        let p = &pts[i];
        if p.ground_flag != state {
            let end = (i + TRANSITION_DEBOUNCE).min(pts.len());
            if end - i == TRANSITION_DEBOUNCE && pts[i..end].iter().all(|q| q.ground_flag == p.ground_flag) {
                state = p.ground_flag;
                out.push(mk(if state { "touchdown_candidate" } else { "liftoff_candidate" }, p, None));
            }
        }
        if p.engine != engine {
            // Start-up steps through states 0..7; only leaving the running
            // state, or reaching the reported destroyed state, is an event.
            if p.engine != 255 && (engine == ENGINE_RUNNING || p.engine == ENGINE_DESTROYED) {
                out.push(mk("engine_state_change", p, Some(format!("{engine} -> {}", p.engine))));
            }
            engine = p.engine;
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Gap {
    pub from_ms: u32,
    pub to_ms: u32,
    pub duration_ms: u32,
}

/// Intervals between consecutive samples longer than `min_ms`.
pub fn gaps(pts: &[Pt], min_ms: u32) -> Vec<Gap> {
    pts.windows(2)
        .filter(|w| w[1].t.saturating_sub(w[0].t) > min_ms)
        .map(|w| Gap { from_ms: w[0].t, to_ms: w[1].t, duration_ms: w[1].t - w[0].t })
        .collect()
}

/// Robust z-score (median/MAD); `None` when the spread is zero.
pub fn robust_z(values: &[f64]) -> Option<Vec<f64>> {
    if values.len() < 4 {
        return None;
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let m = median(&mut values.to_vec());
    let mad = median(&mut values.iter().map(|v| (v - m).abs()).collect());
    (mad > 0.0).then(|| values.iter().map(|v| 0.6745 * (v - m) / mad).collect())
}
