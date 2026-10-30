//! Comparison of replay measurements with a telemetry capture.
//!
//! Each quantity keeps its own identity: indicated airspeed, true airspeed
//! and ground speed are compared only with themselves, or as a stated
//! difference. Figures are DERIVED cross-instrument comparisons, and a
//! capture marked synthetic is EXPERIMENTAL whatever the numbers say.

use crate::cockpit::id;
use crate::evidence::Evidence;
use crate::flight::{angle_diff_deg, isa_density_ratio};
use crate::lab::Extraction;
use crate::sync::{interpolate, synchronize, Interpolated, SyncResult};
use crate::systems::{load_factor, pearson, line_fit, AuthorSample, COMPASS_MINUS_HEADING_DEG};
use crate::telemetry::{Capture, MapInfo, Observation};
use crate::tracks::Dist;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub const VALIDATION_SCHEMA: &str = "static57.sdl.telemetry-validation/0.1";
/// Exact international foot.
pub const METRES_PER_FOOT: f64 = 0.3048;
const KMH: f64 = 3.6;
/// A capture and a replay are associated when their UTC windows overlap
/// within this margin.
const UTC_MARGIN_S: f64 = 300.0;
const MIN_FIELD_PAIRS: usize = 200;
const MIN_FIELD_R: f64 = 0.95;
const MAX_FIELD_ROWS: usize = 80;

#[derive(Debug, Clone, Serialize)]
pub struct Provenance {
    pub replay_file: String,
    pub replay_sha256: String,
    pub game_version: Option<String>,
    pub replay_start_utc: Option<String>,
    pub capture_dir: String,
    pub capture_sha256: String,
    pub capture_bytes: u64,
    pub capture_started_utc: String,
    pub capture_synthetic: bool,
    pub recorder_host: String,
    pub recorder_computer: Option<String>,
    pub decoder_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Association {
    /// `confirmed`, `probable` or `unconfirmed`.
    pub class: &'static str,
    pub utc_overlap: Option<bool>,
    pub replay_vehicles: Vec<String>,
    pub capture_vehicles: Vec<String>,
    pub vehicle_match: Option<bool>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelComparison {
    pub name: &'static str,
    pub replay_source: &'static str,
    pub telemetry_source: String,
    pub unit: &'static str,
    pub n: u64,
    /// Telemetry readings of this field that had aligned replay data.
    pub coverage: Option<f64>,
    /// Median of replay minus telemetry.
    pub bias: Option<f64>,
    pub abs_error: Dist,
    pub rmse: Option<f64>,
    pub pearson_r: Option<f64>,
    /// Median error that the clock uncertainty alone could cause.
    pub sync_sensitivity: Option<f64>,
    pub evidence: Evidence,
    pub note: Option<String>,
}

/// How a cockpit altitude reading relates to replay altitude.
#[derive(Debug, Clone, Serialize)]
pub struct ScaleFit {
    pub field: String,
    pub n: u64,
    pub pearson_r: Option<f64>,
    pub metres_per_unit: Option<f64>,
    /// Median `|Y - value|`.
    pub error_as_metres_m: Option<f64>,
    /// Median `|Y - value / 3.28|`, the game's foot.
    pub error_as_game_foot_m: Option<f64>,
    /// Median `|Y - value * 0.3048|`, the exact foot.
    pub error_as_exact_foot_m: Option<f64>,
    pub nearest: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct AxisHypothesis {
    pub name: String,
    pub rms_map_fraction: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapFit {
    pub n: u64,
    pub map: MapInfo,
    pub hypotheses: Vec<AxisHypothesis>,
    pub best: String,
    pub best_rms_m: f64,
    /// RMS of the next hypothesis over the best.
    pub separation: Option<f64>,
    /// Free least-squares scale of map x and y, per world metre.
    pub free_scale_x: Option<f64>,
    pub free_scale_y: Option<f64>,
    pub expected_scale_x: f64,
    pub expected_scale_y: f64,
    pub direction_error_deg: Dist,
    pub accepted: bool,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldMatch {
    pub replay_field: String,
    pub telemetry_field: String,
    pub n: u64,
    pub pearson_r: f64,
    /// `telemetry = scale * replay + intercept`.
    pub scale: f64,
    pub intercept: f64,
    pub residual_rms: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Coverage {
    pub capture_span_s: f64,
    pub valid_state_samples: u64,
    pub valid_indicator_samples: u64,
    /// Valid state readings falling inside aligned replay data.
    pub aligned_samples: u64,
    pub aligned_fraction: Option<f64>,
    pub aligned_span_s: f64,
    pub telemetry_gaps_over_1s: u64,
    pub replay_gaps_over_1s: u64,
    pub largest_telemetry_gap_s: Option<f64>,
    pub map_position_jumps: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MarkMapped {
    pub note: String,
    pub capture_t_s: f64,
    pub replay_t_s: Option<f64>,
    pub altitude_m: Option<f64>,
    pub speed_mps: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelemetryValidation {
    pub schema: &'static str,
    /// True only for a real capture with a confirmed replay and an
    /// established alignment.
    pub independent_validation: bool,
    pub status: String,
    pub provenance: Provenance,
    pub association: Association,
    pub sync: SyncResult,
    pub coverage: Coverage,
    pub endpoints: BTreeMap<String, crate::telemetry::EndpointStats>,
    pub utc_vs_monotonic_ppm: Option<f64>,
    pub comparisons: Vec<ChannelComparison>,
    pub altimeter_scale: Vec<ScaleFit>,
    pub horizontal: Option<MapFit>,
    pub field_matches: Vec<FieldMatch>,
    pub telemetry_fields: Vec<String>,
    pub marks: Vec<MarkMapped>,
    pub data_gaps: Vec<String>,
    pub verified_discoveries: Vec<String>,
    pub unresolved: Vec<String>,
    pub strike_master_implications: Vec<String>,
}

struct Rows<'a> {
    samples: &'a [AuthorSample],
    sync: &'a SyncResult,
    sigma: f64,
    evidence: Evidence,
}

impl Rows<'_> {
    fn at(&self, t: f64) -> Option<Interpolated> {
        interpolate(self.samples, self.sync.to_replay(t)?)
    }

    /// Compare `field` with a replay quantity, optionally as an angle.
    #[allow(clippy::too_many_arguments)]
    fn compare(&self, name: &'static str, replay_source: &'static str, obs: &[Observation], field: &str, unit: &'static str, tel: impl Fn(f64) -> f64, rep: impl Fn(&Interpolated) -> Option<f64>, angular: bool, note: Option<&str>) -> Option<ChannelComparison> {
        let readings = obs.iter().filter(|o| o.get(field).is_some()).count();
        if readings == 0 {
            return None;
        }
        let mut errs = Vec::new();
        let mut pairs = Vec::new();
        let mut rates = Vec::new();
        for o in obs {
            let Some(v) = o.get(field) else { continue };
            let Some(i) = self.at(o.t_s) else { continue };
            let Some(r) = rep(&i) else { continue };
            let t = tel(v);
            errs.push(if angular { angle_diff_deg(r, t) } else { r - t });
            pairs.push((r, t));
            if let (Some(a), Some(b)) = (self.at(o.t_s - 0.5), self.at(o.t_s + 0.5)) {
                if let (Some(x), Some(y)) = (rep(&a), rep(&b)) {
                    rates.push(if angular { angle_diff_deg(y, x) } else { y - x }.abs());
                }
            }
        }
        let n = errs.len();
        let rmse = (n > 0).then(|| (errs.iter().map(|e| e * e).sum::<f64>() / n as f64).sqrt());
        let mut sorted = errs.clone();
        sorted.sort_by(f64::total_cmp);
        rates.sort_by(f64::total_cmp);
        Some(ChannelComparison {
            name,
            replay_source,
            telemetry_source: field.to_string(),
            unit,
            n: n as u64,
            coverage: Some(n as f64 / readings as f64),
            bias: sorted.get(n / 2).copied(),
            abs_error: Dist::of(errs.iter().map(|e| e.abs()).collect()),
            rmse,
            pearson_r: if angular { None } else { pearson(&pairs) },
            sync_sensitivity: rates.get(rates.len() / 2).map(|r| r * self.sigma),
            evidence: self.evidence,
            note: note.map(String::from),
        })
    }
}

/// Which sign of a telemetry angle matches the replay.
fn sign_of(obs: &[Observation], field: &str, rows: &Rows, rep: impl Fn(&Interpolated) -> f64) -> f64 {
    let p: Vec<(f64, f64)> = obs.iter().step_by(5).filter_map(|o| Some((rep(&rows.at(o.t_s)?), o.get(field)?))).collect();
    if pearson(&p).unwrap_or(1.0) < 0.0 { -1.0 } else { 1.0 }
}

fn comparisons(c: &Capture, rows: &Rows) -> Vec<ChannelComparison> {
    let mut v = Vec::new();
    let (st, ind) = (&c.state, &c.indicators);
    let cockpit_at = |i: &Interpolated, k: u16| rows.samples[i.index].cockpit(k);
    let not_sat = |i: &Interpolated| (!i.saturated).then_some(i.speed_mps);
    v.extend(rows.compare("altitude", "position Y", st, "H, m", "m", |x| x, |i| Some(i.position[1]), false, None));
    v.extend(rows.compare("vertical_speed", "velocity Y", st, "Vy, m/s", "m/s", |x| x, |i| Some(i.velocity[1]), false, None));
    v.extend(rows.compare("indicated_airspeed", "cockpit id 0", st, "IAS, km/h", "m/s", |x| x / KMH, |i| cockpit_at(i, id::AIRSPEED), false, Some("Replay IAS against 8111 IAS.")));
    v.extend(rows.compare("indicated_airspeed_estimate", "world speed through ISA density", st, "IAS, km/h", "m/s", |x| x / KMH, |i| Some(not_sat(i)? * isa_density_ratio(i.position[1]).sqrt()), false, Some("An estimate from ground speed. It assumes no wind and ISA.")));
    v.extend(rows.compare("ground_speed_minus_tas", "world speed", st, "TAS, km/h", "m/s", |x| x / KMH, not_sat, false, Some("Replay speed is world-frame ground speed, not TAS. The difference includes wind, so it is not a decoding error.")));
    v.extend(rows.compare("mach_estimate", "world speed over ISA sound speed", st, "M", "Mach", |x| x, |i| {
        let t = 288.15 - 0.0065 * i.position[1].clamp(0.0, 11_000.0);
        Some(not_sat(i)? / (20.0468 * t.sqrt()))
    }, false, Some("Uses ISA temperature and ground speed.")));
    let s = sign_of(ind, "aviahorizon_pitch", rows, |i| i.pitch_deg);
    v.extend(rows.compare("pitch", "orientation pitch", ind, "aviahorizon_pitch", "deg", |x| s * x, |i| Some(i.pitch_deg), false, (s < 0.0).then_some("8111 sign is opposite to the replay.")));
    let s = sign_of(ind, "aviahorizon_roll", rows, |i| i.bank_deg);
    v.extend(rows.compare("roll", "orientation bank", ind, "aviahorizon_roll", "deg", |x| s * x, |i| Some(i.bank_deg), true, (s < 0.0).then_some("8111 sign is opposite to the replay.")));
    v.extend(rows.compare("heading", "heading + 90", ind, "compass", "deg", |x| x, |i| Some(i.heading_deg + COMPASS_MINUS_HEADING_DEG), true, None));
    v.extend(rows.compare("angle_of_attack_expectation", "pitch minus flight-path angle", st, "AoA, deg", "deg", |x| x, |i| {
        (i.speed_mps > 30.0).then(|| i.pitch_deg - (i.velocity[1] / i.speed_mps).clamp(-1.0, 1.0).asin().to_degrees())
    }, false, Some("Equals AoA only in wings-level flight. It is an expectation, not an identity.")));
    let ny: Vec<Option<f64>> = (0..rows.samples.len()).map(|i| (i > 0 && i + 1 < rows.samples.len()).then(|| load_factor(&rows.samples[i - 1], &rows.samples[i], &rows.samples[i + 1])).flatten()).collect();
    v.extend(rows.compare("load_factor", "second difference of position", st, "Ny", "g", |x| x, |i| ny[i.index], false, Some("Replay value comes from 4 Hz positions, so it smooths short peaks.")));
    v
}

/// Compare altimeter-like fields with replay altitude under each unit.
fn altimeter_scale(c: &Capture, rows: &Rows) -> Vec<ScaleFit> {
    let keys: BTreeSet<&String> = c.indicators.iter().flat_map(|o| o.fields.keys()).filter(|k| k.starts_with("altitude")).collect();
    let mut out = Vec::new();
    for k in keys {
        let p: Vec<(f64, f64)> = c.indicators.iter().filter_map(|o| Some((o.get(k)?, rows.at(o.t_s)?.position[1]))).collect();
        if p.len() < 20 {
            continue;
        }
        let med = |f: &dyn Fn(f64) -> f64| {
            let mut e: Vec<f64> = p.iter().map(|x| (x.1 - f(x.0)).abs()).collect();
            e.sort_by(f64::total_cmp);
            e[e.len() / 2]
        };
        let (m, g, e) = (med(&|x| x), med(&|x| x * id::METRES_PER_GAME_FOOT), med(&|x| x * METRES_PER_FOOT));
        let nearest = if m <= g && m <= e { "metre" } else if g <= e { "game_foot_1_over_3.28" } else { "exact_foot_0.3048" };
        out.push(ScaleFit { field: k.clone(), n: p.len() as u64, pearson_r: pearson(&p), metres_per_unit: line_fit(&p).map(|f| f.0), error_as_metres_m: Some(m), error_as_game_foot_m: Some(g), error_as_exact_foot_m: Some(e), nearest });
    }
    out
}

fn map_fit(c: &Capture, rows: &Rows) -> Option<MapFit> {
    let info = c.map_info.clone()?;
    let pts: Vec<(f64, f64, f64, f64, Interpolated, Option<f64>)> = c.map_obj.iter().filter_map(|m| {
        let i = rows.at(m.t_s)?;
        let dir = m.dx.zip(m.dy).map(|(dx, dy)| dx.atan2(-dy).to_degrees());
        Some((m.x, m.y, i.position[0], i.position[2], i, dir))
    }).collect();
    if pts.len() < 50 {
        return None;
    }
    let (min, max) = (info.map_min, info.map_max);
    let (w, h) = (max[0] - min[0], max[1] - min[1]);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let mut hyp = Vec::new();
    for swap in [false, true] {
        for fx in [false, true] {
            for fy in [false, true] {
                let mut sum = 0.0;
                for p in &pts {
                    let (a, b) = if swap { (p.3, p.2) } else { (p.2, p.3) };
                    let u = if fx { (max[0] - a) / w } else { (a - min[0]) / w };
                    let v = if fy { (max[1] - b) / h } else { (b - min[1]) / h };
                    sum += (u - p.0).powi(2) + (v - p.1).powi(2);
                }
                let name = format!("map_x = {}{}, map_y = {}{}", if fx { "-" } else { "+" }, if swap { "Z" } else { "X" }, if fy { "-" } else { "+" }, if swap { "X" } else { "Z" });
                hyp.push(AxisHypothesis { name, rms_map_fraction: (sum / pts.len() as f64 / 2.0).sqrt() });
            }
        }
    }
    hyp.sort_by(|a, b| a.rms_map_fraction.total_cmp(&b.rms_map_fraction));
    let best = hyp[0].rms_map_fraction;
    let separation = (best > 0.0).then(|| hyp[1].rms_map_fraction / best);
    let spread = |f: &dyn Fn(&(f64, f64, f64, f64, Interpolated, Option<f64>)) -> f64| {
        let v: Vec<f64> = pts.iter().map(f).collect();
        v.iter().copied().fold(f64::NEG_INFINITY, f64::max) - v.iter().copied().fold(f64::INFINITY, f64::min)
    };
    let swap = hyp[0].name.contains("Z,");
    let px: Vec<(f64, f64)> = pts.iter().map(|p| (if swap { p.3 } else { p.2 }, p.0)).collect();
    let py: Vec<(f64, f64)> = pts.iter().map(|p| (if swap { p.2 } else { p.3 }, p.1)).collect();
    let dir: Vec<f64> = pts.iter().filter_map(|p| Some(angle_diff_deg(p.5?, p.4.heading_deg + COMPASS_MINUS_HEADING_DEG).abs())).collect();
    let moved = spread(&|p| p.2) > 500.0 && spread(&|p| p.3) > 500.0;
    let accepted = moved && best * w.max(h) <= 500.0 && separation.is_none_or(|s| s >= 5.0);
    Some(MapFit {
        n: pts.len() as u64,
        best: hyp[0].name.clone(),
        best_rms_m: best * w.max(h),
        separation,
        free_scale_x: line_fit(&px).map(|f| f.0),
        free_scale_y: line_fit(&py).map(|f| f.0),
        expected_scale_x: 1.0 / w,
        expected_scale_y: 1.0 / h,
        direction_error_deg: Dist::of(dir),
        hypotheses: hyp,
        map: info,
        accepted,
        evidence: rows.evidence,
    })
}

/// Every replay field against every numeric telemetry field. A strong linear
/// relation is reported with its scale, and meaning is decided by review.
fn field_matches(c: &Capture, rows: &Rows) -> Vec<FieldMatch> {
    let mut replay: BTreeMap<String, Vec<(usize, f64)>> = BTreeMap::new();
    for (i, s) in rows.samples.iter().enumerate() {
        let mut put = |k: String, v: f64| replay.entry(k).or_default().push((i, v));
        for n in 0..14 {
            put(format!("nibble_{n}"), s.nibble(n) as f64);
        }
        put("gear_bit".into(), s.gear_bit as u8 as f64);
        if let Some(e) = s.engine_state {
            put("engine_state".into(), e as f64);
        }
        if let Some(t) = s.throttle() {
            put("throttle_boost".into(), t);
        }
        for (k, v) in &s.cockpit {
            put(format!("cockpit_{k}"), *v as f64);
        }
    }
    // Nearest telemetry reading of each replay sample, per field.
    let mut tel: BTreeMap<String, BTreeMap<usize, f64>> = BTreeMap::new();
    for (prefix, obs) in [("state", &c.state), ("indicators", &c.indicators)] {
        for o in obs.iter() {
            let Some(t) = rows.sync.to_replay(o.t_s) else { continue };
            let t_ms = t * 1000.0;
            let i = rows.samples.partition_point(|s| (s.t_ms as f64) < t_ms);
            let Some((idx, s)) = [i.checked_sub(1), Some(i)].into_iter().flatten().filter_map(|j| rows.samples.get(j).map(|s| (j, s))).min_by(|a, b| (a.1.t_ms as f64 - t_ms).abs().total_cmp(&(b.1.t_ms as f64 - t_ms).abs())) else { continue };
            if (s.t_ms as f64 - t_ms).abs() > 60.0 {
                continue;
            }
            for (k, v) in &o.fields {
                tel.entry(format!("{prefix}:{k}")).or_default().insert(idx, *v);
            }
        }
    }
    let mut out = Vec::new();
    for (rk, rv) in &replay {
        for (tk, tv) in &tel {
            let p: Vec<(f64, f64)> = rv.iter().filter_map(|(i, x)| Some((*x, *tv.get(i)?))).collect();
            if p.len() < MIN_FIELD_PAIRS {
                continue;
            }
            let Some(r) = pearson(&p) else { continue };
            if r.abs() < MIN_FIELD_R {
                continue;
            }
            let Some((m, b)) = line_fit(&p) else { continue };
            let rms = (p.iter().map(|x| (x.1 - m * x.0 - b).powi(2)).sum::<f64>() / p.len() as f64).sqrt();
            out.push(FieldMatch { replay_field: rk.clone(), telemetry_field: tk.clone(), n: p.len() as u64, pearson_r: r, scale: m, intercept: b, residual_rms: rms });
        }
    }
    out.sort_by(|a, b| b.pearson_r.abs().total_cmp(&a.pearson_r.abs()));
    out.truncate(MAX_FIELD_ROWS);
    out
}

fn associate(c: &Capture, x: &ReplayView, sync: &SyncResult) -> Association {
    let replay_vehicles: Vec<String> = x.author.iter().filter_map(|s| s.vehicle.clone()).collect::<BTreeSet<_>>().into_iter().collect();
    let capture_vehicles: Vec<String> = c.indicators.iter().filter_map(|o| o.vehicle.clone()).filter(|v| !v.is_empty()).collect::<BTreeSet<_>>().into_iter().collect();
    let vehicle_match = (!replay_vehicles.is_empty() && !capture_vehicles.is_empty()).then(|| capture_vehicles.iter().any(|v| replay_vehicles.contains(v)));
    let mut notes = Vec::new();
    let utc_overlap = if c.meta.synthetic {
        notes.push("Synthetic capture: its UTC stamps have no meaning".to_string());
        None
    } else {
        x.start_unix_s.map(|r0| {
            let r1 = r0 + x.span_s;
            let c0 = c.meta.started_utc_ms as f64 / 1000.0;
            let c1 = c0 + c.meta.duration_s.unwrap_or(0.0);
            c0 <= r1 + UTC_MARGIN_S && r0 <= c1 + UTC_MARGIN_S
        })
    };
    let aligned = sync.quality == "established";
    let class = match (utc_overlap, vehicle_match, aligned) {
        (Some(true), Some(true), true) => "confirmed",
        (Some(false), _, _) | (_, Some(false), _) => "unconfirmed",
        (_, _, true) => "probable",
        _ => "unconfirmed",
    };
    if utc_overlap == Some(false) {
        notes.push("Capture and replay UTC windows do not overlap".into());
    }
    if vehicle_match == Some(false) {
        notes.push("Capture and replay vehicles differ".into());
    }
    Association { class, utc_overlap, replay_vehicles, capture_vehicles, vehicle_match, notes }
}

fn coverage(c: &Capture, rows: &Rows) -> Coverage {
    let aligned: Vec<f64> = c.state.iter().filter(|o| rows.at(o.t_s).is_some()).map(|o| o.t_s).collect();
    let gaps: Vec<f64> = c.state.windows(2).map(|w| w[1].t_s - w[0].t_s).collect();
    let span = c.state.last().zip(c.state.first()).map_or(0.0, |(a, b)| a.t_s - b.t_s);
    let jumps = c.map_obj.windows(2).filter(|w| {
        let (dt, d) = (w[1].t_s - w[0].t_s, (w[1].x - w[0].x).hypot(w[1].y - w[0].y));
        dt > 0.0 && d / dt > 0.05
    }).count() as u64;
    Coverage {
        capture_span_s: span,
        valid_state_samples: c.state.len() as u64,
        valid_indicator_samples: c.indicators.len() as u64,
        aligned_samples: aligned.len() as u64,
        aligned_fraction: (!c.state.is_empty()).then(|| aligned.len() as f64 / c.state.len() as f64),
        aligned_span_s: aligned.last().zip(aligned.first()).map_or(0.0, |(a, b)| a - b),
        telemetry_gaps_over_1s: gaps.iter().filter(|g| **g > 1.0).count() as u64,
        replay_gaps_over_1s: rows.samples.windows(2).filter(|w| w[1].t_ms - w[0].t_ms > 1000).count() as u64,
        largest_telemetry_gap_s: gaps.iter().copied().max_by(f64::total_cmp),
        map_position_jumps: jumps,
    }
}

fn strike_master_notes(v: &[ChannelComparison], h: Option<&MapFit>) -> Vec<String> {
    let has = |n: &str| v.iter().any(|c| c.name == n && c.n > 0);
    let mut out = Vec::new();
    if has("altitude") && has("vertical_speed") {
        out.push("HUD altitude and vertical-speed scales can be compared with these figures. Proposal only".to_string());
    }
    if has("indicated_airspeed") {
        out.push("IAS, TAS and ground speed have separate error figures here, so HUD and mission timing can keep them separate".into());
    }
    if h.is_some_and(|m| m.accepted) {
        out.push("The fitted map transform allows navigation checks in map coordinates".into());
    }
    out.push("Nothing here changes Strike Master or its databases. Each item needs owner review".into());
    out
}

/// What the validator needs from a replay.
pub struct ReplayView<'a> {
    pub file_name: &'a str,
    pub sha256: String,
    pub game_version: Option<String>,
    pub start_utc: Option<String>,
    pub start_unix_s: Option<f64>,
    pub span_s: f64,
    pub author: &'a [AuthorSample],
    pub kills_s: Vec<f64>,
}

impl<'a> ReplayView<'a> {
    pub fn of(x: &'a Extraction, file_name: &'a str) -> Self {
        let s = x.manifest.session.as_ref();
        ReplayView {
            file_name,
            sha256: x.manifest.input.sha256.clone(),
            game_version: s.map(|s| s.game_version.value.clone()),
            start_utc: s.map(|s| s.start_time_utc.value.clone()),
            start_unix_s: s.map(|s| s.start_time_unix.value as f64),
            span_s: x.manifest.packets.as_ref().and_then(|p| p.clock.span_ms).unwrap_or(0) as f64 / 1000.0,
            author: &x.author,
            kills_s: x.kills.iter().map(|k| k.timestamp_ms as f64 / 1000.0).collect(),
        }
    }
}

/// Align a capture with a replay and compare everything comparable.
pub fn validate(capture: &Capture, x: &ReplayView) -> TelemetryValidation {
    let sync = synchronize(capture, x.author, &x.kills_s);
    let evidence = if capture.meta.synthetic { Evidence::Experimental } else { Evidence::Derived };
    let rows = Rows { samples: x.author, sync: &sync, sigma: sync.uncertainty_s.unwrap_or(0.0), evidence };
    let association = associate(capture, x, &sync);
    let usable = sync.usable() && association.class != "unconfirmed" || (capture.meta.synthetic && sync.usable());
    let (comparisons_, altimeter, horizontal, matches) = if usable { (comparisons(capture, &rows), altimeter_scale(capture, &rows), map_fit(capture, &rows), field_matches(capture, &rows)) } else { (Vec::new(), Vec::new(), None, Vec::new()) };
    let cov = coverage(capture, &rows);
    let independent = !capture.meta.synthetic && association.class == "confirmed" && sync.quality == "established" && !sync.discontinuity;

    let mut gaps = Vec::new();
    if capture.state.is_empty() {
        gaps.push("No valid /state readings".to_string());
    }
    if capture.indicators.is_empty() {
        gaps.push("No valid /indicators readings".to_string());
    }
    if capture.map_info.is_none() {
        gaps.push("No /map_info.json".to_string());
    }
    if capture.map_obj.is_empty() {
        gaps.push("No player object in /map_obj.json".to_string());
    }
    if x.author.is_empty() {
        gaps.push("The replay has no author samples".to_string());
    }
    for (n, s) in &capture.endpoints {
        if s.errors > 0 {
            gaps.push(format!("{n}: {} of {} requests failed, first error: {}", s.errors, s.requests, s.first_error.as_deref().unwrap_or("-")));
        }
    }
    if !usable {
        gaps.push("Comparisons withheld: alignment or replay association is not good enough".to_string());
    }

    let mut discoveries = Vec::new();
    let mut unresolved = Vec::new();
    if capture.meta.synthetic {
        unresolved.push("This capture is synthetic. It tests the software and validates nothing about the replay".to_string());
    } else if independent {
        for c in comparisons_.iter().filter(|c| c.n > 0) {
            if let (Some(p50), Some(p99)) = (c.abs_error.p50, c.abs_error.p99) {
                discoveries.push(format!("{}: median abs error {p50:.3} {}, p99 {p99:.3}, n {}", c.name, c.unit, c.n));
            }
        }
    }
    if let Some(m) = &horizontal {
        if m.accepted && independent {
            discoveries.push(format!("Map transform: {}, RMS {:.1} m", m.best, m.best_rms_m));
        } else if !m.accepted {
            unresolved.push(format!("Map transform not accepted: best {}, RMS {:.1} m", m.best, m.best_rms_m));
        }
    } else {
        unresolved.push("Map transform not fitted: it needs /map_info.json and at least 50 aligned map positions".to_string());
    }
    if matches.is_empty() {
        unresolved.push("No replay field matched a telemetry field at |r| >= 0.95".to_string());
    }
    unresolved.extend(crate::systems::CANDIDATES.iter().filter(|c| c.evidence == Evidence::Unknown).map(|c| format!("{}: {}", c.field, c.basis)));

    let mut telemetry_fields: Vec<String> = capture.state.iter().flat_map(|o| o.fields.keys()).map(|k| format!("state:{k}")).chain(capture.indicators.iter().flat_map(|o| o.fields.keys()).map(|k| format!("indicators:{k}"))).collect::<BTreeSet<_>>().into_iter().collect();
    telemetry_fields.truncate(300);
    let marks = capture.marks.iter().map(|m| {
        let t = sync.to_replay(m.t_s);
        let i = t.and_then(|t| interpolate(x.author, t));
        MarkMapped { note: m.note.clone(), capture_t_s: m.t_s, replay_t_s: t, altitude_m: i.map(|i| i.position[1]), speed_mps: i.map(|i| i.speed_mps) }
    }).collect();
    let status = if capture.meta.synthetic {
        "SYNTHETIC: software test only, not independent validation".to_string()
    } else if independent {
        "Independent validation: real capture, confirmed replay, established alignment".to_string()
    } else {
        format!("Not independent validation: association {}, alignment {}", association.class, sync.quality)
    };
    let notes = strike_master_notes(&comparisons_, horizontal.as_ref());
    TelemetryValidation {
        schema: VALIDATION_SCHEMA,
        independent_validation: independent,
        status,
        provenance: Provenance {
            replay_file: x.file_name.to_string(),
            replay_sha256: x.sha256.clone(),
            game_version: x.game_version.clone(),
            replay_start_utc: x.start_utc.clone(),
            capture_dir: capture.dir.display().to_string(),
            capture_sha256: capture.sha256.clone(),
            capture_bytes: capture.bytes,
            capture_started_utc: capture.meta.started_utc.clone(),
            capture_synthetic: capture.meta.synthetic,
            recorder_host: capture.meta.host.clone(),
            recorder_computer: capture.meta.computer.clone(),
            decoder_version: env!("CARGO_PKG_VERSION").to_string(),
        },
        association,
        coverage: cov,
        endpoints: capture.endpoints.clone(),
        utc_vs_monotonic_ppm: capture.utc_vs_monotonic_ppm,
        comparisons: comparisons_,
        altimeter_scale: altimeter,
        horizontal,
        field_matches: matches,
        telemetry_fields,
        marks,
        data_gaps: gaps,
        verified_discoveries: discoveries,
        unresolved,
        strike_master_implications: notes,
        sync,
    }
}
