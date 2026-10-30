//! Aircraft controls, engine and configuration fields of the recording
//! author, checked against the replay's own cockpit message and against
//! measured motion. Findings are in `docs/FINDINGS_M7_FLIGHT_SYSTEMS.md`.
//!
//! The seven control bytes hold 14 nibbles, high nibble first. Every label
//! here is a candidate until an independent source agrees: see
//! [`CANDIDATES`]. Which cockpit parameter ids exist, and their units, depend
//! on each aircraft's instruments, so nothing outside [`crate::cockpit::id`]
//! is treated as a known quantity.

use crate::aircraft::AircraftState;
use crate::cockpit::{id, CockpitParams};
use crate::evidence::Evidence;
use crate::flight::{angle_diff_deg, decode_orientation, decode_velocity};
use crate::tracks::Dist;
use serde::Serialize;
use std::collections::BTreeMap;

/// Scale of `Engine::boost_raw`: full scale reads 1.1.
pub const BOOST_FULL_SCALE: f64 = 32767.0;
pub const BOOST_MAX: f64 = 1.1;
/// Replay heading plus this angle is the cockpit compass.
pub const COMPASS_MINUS_HEADING_DEG: f64 = 90.0;
pub const GRAVITY_MPS2: f64 = 9.81;
/// Bracketing samples further apart than this are not interpolated.
pub const MAX_INTERPOLATION_GAP_S: f64 = 0.3;
/// Beyond this a load factor is a position jump, not flight.
pub const MAX_LOAD_FACTOR_G: f64 = 30.0;

/// A control field and what is known about it.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub field: &'static str,
    pub location: &'static str,
    pub label: &'static str,
    pub scale: &'static str,
    pub evidence: Evidence,
    pub basis: &'static str,
}

/// Every field studied. DERIVED entries were compared with a real 8111
/// capture on two aircraft (`saab_a32a_red_adam`, `saab_j21a_1`), so they are
/// measured, but not yet across the fleet. EXPERIMENTAL entries were checked
/// only against the replay's own cockpit message. UNKNOWN entries were not
/// exercised in the capture.
pub const CANDIDATES: &[Candidate] = &[
    Candidate { field: "gear_bit", location: "state bit 0 (ground_flag)", label: "landing gear not retracted", scale: "0 up, 1 otherwise", evidence: Evidence::Derived, basis: "r 0.995 and 0.994 against 8111 gear %. Agrees with cockpit id 228 in over 99.9% of about 19,900 samples. Not an on-ground flag" },
    Candidate { field: "throttle_boost", location: "engine boost_raw (int16)", label: "throttle above 100%", scale: "raw / 32767 * 1.1", evidence: Evidence::Derived, basis: "Reads 1.011, 1.022 and 1.100 where 8111 throttle reads 101, 102 and 110% (244 samples, one aircraft). Absent at or below 100%" },
    Candidate { field: "throttle_nibble", location: "controls nibble 11", label: "engine throttle, coarse", scale: "floor(throttle * 15), capped at 15", evidence: Evidence::Derived, basis: "r 0.964 and 0.995 against 8111 throttle. Matches floor(id 240 * 15) in 93.5% of samples" },
    Candidate { field: "flaps_nibble", location: "controls nibble 0", label: "flap position", scale: "floor(flaps * 15); seen at 0 and 4 (33%)", evidence: Evidence::Derived, basis: "r 0.991 and 0.965 against 8111 flaps %, with only two positions flown" },
    Candidate { field: "elevator_nibble", location: "controls nibble 1", label: "stick pitch input", scale: "signed, 8 neutral, about 7.5 steps per full travel", evidence: Evidence::Derived, basis: "Monotonic against 8111 stick_elevator apart from a cluster at value 5. r 0.919 on one aircraft, 0.554 on the other because of that cluster" },
    Candidate { field: "aileron_nibble", location: "controls nibble 2", label: "stick roll input", scale: "signed, 8 neutral, about 7 steps per full travel", evidence: Evidence::Derived, basis: "Monotonic against 8111 stick_ailerons apart from values 4 and 5. r 0.67 and 0.70. Opposes roll rate on 14 of 17 aircraft" },
    Candidate { field: "rudder_nibble", location: "controls nibble 3", label: "pedal input", scale: "signed, 8 neutral, about 7 steps per full travel", evidence: Evidence::Derived, basis: "r 0.715 against 8111 rudder % on one aircraft. Barely exercised on the other" },
    Candidate { field: "blister_nibble", location: "controls nibble 4", label: "follows 8111 blister1", scale: "0 or 15", evidence: Evidence::Derived, basis: "15 where blister1 is 1, 0 where it is 0 (r 0.971, 0.976). Set while parked. What blister1 represents is not established. The earlier wheel-brake guess is withdrawn" },
    Candidate { field: "engine_state", location: "engine state byte", label: "engine start sequence", scale: "0 stopped, 1-6 starting, 7 running, 8 stopped", evidence: Evidence::Derived, basis: "8111 RPM by state: 0 at 0, 12-18 at 1, about 29 at 2, 120 at 4, 190-210 at 5, full RPM at 7, 0 at 8" },
    Candidate { field: "fuel_cockpit", location: "cockpit id 195", label: "fuel mass", scale: "kg", evidence: Evidence::Derived, basis: "Equals 8111 fuel (scale 1.000) and tracks Mfuel kg (r 0.998-1.000, scale 0.998-1.011)" },
    Candidate { field: "sweep_nibble", location: "controls nibble 10", label: "variable-geometry position", scale: "0-15", evidence: Evidence::Experimental, basis: "Non-zero on su_22m4, f_111f and mirage_milan only, and rises with speed. Not in the capture" },
    Candidate { field: "nibble_5", location: "controls nibble 5", label: "unassigned", scale: "0 or 15", evidence: Evidence::Unknown, basis: "Mostly low speed, on 8 aircraft. Never set in the compared flights. Airbrake is a candidate" },
    Candidate { field: "nibble_8", location: "controls nibble 8", label: "unassigned", scale: "0 or 15", evidence: Evidence::Unknown, basis: "Below 115 m/s in 95% of samples, on 8 aircraft. Never set in the compared flights" },
];

/// One sample of the recording author, with the nearest cockpit message.
#[derive(Debug, Clone)]
pub struct AuthorSample {
    pub t_ms: u32,
    pub uid: u16,
    pub vehicle: Option<String>,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub speed_mps: f64,
    pub saturated: bool,
    pub heading_deg: f64,
    pub pitch_deg: f64,
    pub bank_deg: f64,
    pub gear_bit: bool,
    pub controls: [u8; 7],
    pub engines: u8,
    pub engine_state: Option<u8>,
    pub boost_raw: Option<i16>,
    pub cockpit: Vec<(u16, f32)>,
}

impl AuthorSample {
    pub fn from_state(t_ms: u32, uid: u16, s: &AircraftState) -> Self {
        let o = decode_orientation(s.euler_raw);
        let v = decode_velocity(s.packed_velocity_raw);
        AuthorSample {
            t_ms,
            uid,
            vehicle: None,
            position: s.position.map(|c| c as f64),
            velocity: [v.vx_mps, v.vy_mps, v.vz_mps],
            speed_mps: v.speed_mps,
            saturated: v.saturated,
            heading_deg: o.heading_deg,
            pitch_deg: o.pitch_deg,
            bank_deg: o.bank_deg,
            gear_bit: s.ground_flag,
            controls: s.controls,
            engines: s.engines.len() as u8,
            engine_state: s.engines.first().map(|e| e.state),
            boost_raw: s.engines.first().and_then(|e| e.boost_raw),
            cockpit: Vec::new(),
        }
    }

    pub fn cockpit(&self, i: u16) -> Option<f64> {
        self.cockpit.iter().find(|p| p.0 == i).map(|p| p.1 as f64)
    }

    /// Nibble 0..=13, high nibble of byte 0 first.
    pub fn nibble(&self, i: usize) -> u8 {
        let b = self.controls[i / 2];
        if i % 2 == 0 { b >> 4 } else { b & 15 }
    }

    pub fn throttle(&self) -> Option<f64> {
        self.boost_raw.map(|b| b as f64 / BOOST_FULL_SCALE * BOOST_MAX)
    }

    /// Cockpit clock in seconds of a 12-hour dial, if all three hands exist.
    pub fn clock_s(&self) -> Option<f64> {
        clock_seconds(self.cockpit(id::CLOCK_HOUR)?, self.cockpit(id::CLOCK_MIN)?, self.cockpit(id::CLOCK_SEC)?)
    }
}

/// `hour` is fractional on the dial; minutes and seconds are whole.
pub fn clock_seconds(hour: f64, min: f64, sec: f64) -> Option<f64> {
    (hour.is_finite() && min.is_finite() && sec.is_finite()).then(|| hour.floor().rem_euclid(12.0) * 3600.0 + min.round() * 60.0 + sec.round())
}

/// Attach the nearest cockpit message within `max_ms` to each sample.
pub fn attach_cockpit(samples: &mut [AuthorSample], cockpit: &[CockpitParams], max_ms: u32) {
    for s in samples.iter_mut() {
        let i = cockpit.partition_point(|c| c.timestamp_ms < s.t_ms);
        let best = [i.checked_sub(1), Some(i)].into_iter().flatten().filter_map(|j| cockpit.get(j)).min_by_key(|c| c.timestamp_ms.abs_diff(s.t_ms));
        if let Some(c) = best.filter(|c| c.timestamp_ms.abs_diff(s.t_ms) <= max_ms) {
            s.cockpit = c.params.clone();
        }
    }
}

/// Load factor along the aircraft's up axis, g, from the second difference of
/// position around `b`. `None` across a gap or an uneven step.
pub fn load_factor(a: &AuthorSample, b: &AuthorSample, c: &AuthorSample) -> Option<f64> {
    let (d0, d1) = (b.t_ms.checked_sub(a.t_ms)?, c.t_ms.checked_sub(b.t_ms)?);
    if d0 == 0 || d1 == 0 || d0 > 300 || d1 > 300 || a.uid != c.uid {
        return None;
    }
    let dt = (d0 + d1) as f64 / 2000.0;
    let mut acc = [0.0; 3];
    for k in 0..3 {
        acc[k] = (c.position[k] - 2.0 * b.position[k] + a.position[k]) / (dt * dt);
    }
    acc[1] += GRAVITY_MPS2;
    let (h, th, r) = (b.heading_deg.to_radians(), b.pitch_deg.to_radians(), b.bank_deg.to_radians());
    let f = [th.cos() * h.cos(), th.sin(), -th.cos() * h.sin()];
    let right = [h.sin(), 0.0, h.cos()];
    let cross = [f[1] * right[2] - f[2] * right[1], f[2] * right[0] - f[0] * right[2], f[0] * right[1] - f[1] * right[0]];
    let up = if cross[1] < 0.0 { cross.map(|x| -x) } else { cross };
    let n: f64 = (0..3).map(|k| acc[k] * (r.cos() * up[k] + r.sin() * right[k])).sum();
    let g = n / GRAVITY_MPS2;
    (g.abs() <= MAX_LOAD_FACTOR_G).then_some(g)
}

/// Pearson correlation, or `None` when either side does not vary.
pub fn pearson(pairs: &[(f64, f64)]) -> Option<f64> {
    if pairs.len() < 3 {
        return None;
    }
    let n = pairs.len() as f64;
    let (mx, my) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for (x, y) in pairs {
        sxx += (x - mx) * (x - mx);
        syy += (y - my) * (y - my);
        sxy += (x - mx) * (y - my);
    }
    (sxx > 1e-12 && syy > 1e-12).then(|| sxy / (sxx * syy).sqrt())
}

/// Least-squares `y = slope * x + intercept`.
pub fn line_fit(pairs: &[(f64, f64)]) -> Option<(f64, f64)> {
    if pairs.len() < 2 {
        return None;
    }
    let n = pairs.len() as f64;
    let (mx, my) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
    let (mut sxx, mut sxy) = (0.0, 0.0);
    for (x, y) in pairs {
        sxx += (x - mx) * (x - mx);
        sxy += (x - mx) * (y - my);
    }
    (sxx > 1e-12).then(|| (sxy / sxx, my - sxy / sxx * mx))
}

#[derive(Debug, Clone, Serialize)]
pub struct CompassCheck {
    pub n: u64,
    /// `compass - heading - 90`, degrees.
    pub median_error_deg: Option<f64>,
    pub abs_error_deg: Dist,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClockCheck {
    pub n: u64,
    pub span_s: f64,
    /// Clock seconds per replay second.
    pub rate: Option<f64>,
    pub residual_abs_s: Dist,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgreementCheck {
    pub n: u64,
    pub agree: u64,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoadFactorCheck {
    pub vehicle: String,
    pub n: u64,
    pub pearson_r: Option<f64>,
    pub abs_error_g: Dist,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, Serialize)]
pub struct FuelObservation {
    pub vehicle: String,
    pub start: f64,
    pub end: f64,
    pub duration_s: f64,
    /// Units per second. Negative while burning.
    pub rate_per_s: f64,
    pub refilled: bool,
    pub evidence: Evidence,
}

/// Controls and systems of the author, per replay.
#[derive(Debug, Clone, Serialize)]
pub struct SystemsReport {
    pub samples: u64,
    pub samples_with_cockpit: u64,
    pub engine_counts: BTreeMap<u8, u64>,
    pub engine_states: BTreeMap<u8, u64>,
    pub gear_bit_set: u64,
    pub gear_transitions: u64,
    pub gear_vs_cockpit: AgreementCheck,
    pub throttle_boost_samples: u64,
    /// Throttle from `boost_raw` minus cockpit id 240.
    pub throttle_abs_error: Dist,
    pub throttle_nibble_agree: AgreementCheck,
    pub compass: CompassCheck,
    pub clock: ClockCheck,
    pub load_factor: Vec<LoadFactorCheck>,
    pub fuel: Vec<FuelObservation>,
    pub nibble_nonzero: BTreeMap<String, u64>,
}

fn nibble_name(i: usize) -> String {
    format!("n{i}")
}

/// Compare the candidates with the cockpit message and with motion.
pub fn analyze(samples: &[AuthorSample]) -> SystemsReport {
    let mut r = SystemsReport {
        samples: samples.len() as u64,
        samples_with_cockpit: samples.iter().filter(|s| !s.cockpit.is_empty()).count() as u64,
        engine_counts: BTreeMap::new(),
        engine_states: BTreeMap::new(),
        gear_bit_set: 0,
        gear_transitions: 0,
        gear_vs_cockpit: AgreementCheck { n: 0, agree: 0, evidence: Evidence::Derived },
        throttle_boost_samples: 0,
        throttle_abs_error: Dist::of(Vec::new()),
        throttle_nibble_agree: AgreementCheck { n: 0, agree: 0, evidence: Evidence::Derived },
        compass: CompassCheck { n: 0, median_error_deg: None, abs_error_deg: Dist::of(Vec::new()), evidence: Evidence::Derived },
        clock: ClockCheck { n: 0, span_s: 0.0, rate: None, residual_abs_s: Dist::of(Vec::new()), evidence: Evidence::Derived },
        load_factor: Vec::new(),
        fuel: Vec::new(),
        nibble_nonzero: BTreeMap::new(),
    };
    let (mut throttle, mut compass, mut clock) = (Vec::new(), Vec::new(), Vec::new());
    for (i, s) in samples.iter().enumerate() {
        *r.engine_counts.entry(s.engines).or_default() += 1;
        if let Some(e) = s.engine_state {
            *r.engine_states.entry(e).or_default() += 1;
        }
        r.gear_bit_set += s.gear_bit as u64;
        if i > 0 && samples[i - 1].uid == s.uid && samples[i - 1].gear_bit != s.gear_bit {
            r.gear_transitions += 1;
        }
        for n in 0..14 {
            if s.nibble(n) != 0 && !matches!(n, 1..=3) {
                *r.nibble_nonzero.entry(nibble_name(n)).or_default() += 1;
            }
        }
        if let Some(g) = s.cockpit(id::GEAR) {
            r.gear_vs_cockpit.n += 1;
            r.gear_vs_cockpit.agree += ((g > 0.5) == s.gear_bit) as u64;
        }
        if let Some(c) = s.cockpit(id::THROTTLE) {
            if let Some(t) = s.throttle() {
                r.throttle_boost_samples += 1;
                throttle.push((t - c).abs());
            }
            r.throttle_nibble_agree.n += 1;
            r.throttle_nibble_agree.agree += ((c * 15.0).floor().clamp(0.0, 15.0) as u8 == s.nibble(11)) as u64;
        }
        if let Some(c) = s.cockpit(id::COMPASS).filter(|_| s.speed_mps >= 30.0) {
            compass.push(angle_diff_deg(c, s.heading_deg + COMPASS_MINUS_HEADING_DEG));
        }
        if let Some(c) = s.clock_s() {
            clock.push((s.t_ms as f64 / 1000.0, c));
        }
    }
    r.throttle_abs_error = Dist::of(throttle);
    r.compass.n = compass.len() as u64;
    compass.sort_by(f64::total_cmp);
    r.compass.median_error_deg = compass.get(compass.len() / 2).copied();
    r.compass.abs_error_deg = Dist::of(compass.iter().map(|x| x.abs()).collect());

    // Undo the 12-hour wrap, then fit.
    let mut wrap = 0.0;
    for i in 1..clock.len() {
        if clock[i].1 + wrap < clock[i - 1].1 - 21_600.0 {
            wrap += 43_200.0;
        }
        clock[i].1 += wrap;
    }
    r.clock.n = clock.len() as u64;
    r.clock.span_s = clock.last().zip(clock.first()).map_or(0.0, |(a, b)| a.0 - b.0);
    if r.clock.span_s >= 30.0 {
        if let Some((m, b)) = line_fit(&clock) {
            r.clock.rate = Some(m);
            r.clock.residual_abs_s = Dist::of(clock.iter().map(|p| (p.1 - m * p.0 - b).abs()).collect());
        }
    }

    // Load factor, only where this aircraft's id 482 behaves as a g-meter.
    let mut by_vehicle: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
    for w in samples.windows(3) {
        if w[1].speed_mps < 60.0 {
            continue;
        }
        if let (Some(n), Some(g), Some(v)) = (load_factor(&w[0], &w[1], &w[2]), w[1].cockpit(id::G_METER), &w[1].vehicle) {
            by_vehicle.entry(v.clone()).or_default().push((n, g));
        }
    }
    for (vehicle, p) in by_vehicle {
        let r_ = pearson(&p);
        r.load_factor.push(LoadFactorCheck {
            vehicle,
            n: p.len() as u64,
            pearson_r: r_,
            abs_error_g: Dist::of(p.iter().map(|x| (x.0 - x.1).abs()).collect()),
            evidence: Evidence::Derived,
        });
    }

    // Fuel per continuous life of a vehicle.
    let mut start: Option<(usize, f64)> = None;
    let close = |r: &mut SystemsReport, a: usize, b: usize, q0: f64, q1: f64, up: bool| {
        let dt = (samples[b].t_ms - samples[a].t_ms) as f64 / 1000.0;
        if dt >= 30.0 {
            r.fuel.push(FuelObservation { vehicle: samples[a].vehicle.clone().unwrap_or_default(), start: q0, end: q1, duration_s: dt, rate_per_s: (q1 - q0) / dt, refilled: up, evidence: Evidence::Experimental });
        }
    };
    let mut last: Option<(usize, f64)> = None;
    for (i, s) in samples.iter().enumerate() {
        let Some(q) = s.cockpit(id::FUEL) else { continue };
        if let (Some((a, q0)), Some((l, ql))) = (start, last) {
            // A rise of more than 1% or a new vehicle ends the life.
            if q > ql * 1.01 + 1.0 || samples[l].vehicle != s.vehicle {
                close(&mut r, a, l, q0, ql, q > ql * 1.01 + 1.0);
                start = Some((i, q));
            }
        } else {
            start = Some((i, q));
        }
        last = Some((i, q));
    }
    if let (Some((a, q0)), Some((l, ql))) = (start, last) {
        close(&mut r, a, l, q0, ql, false);
    }
    r
}
