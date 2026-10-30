//! Aligning a telemetry capture with a replay.
//!
//! Replay time is game session time, and capture time is host monotonic time.
//! Nothing assumes they start together. The relation estimated is
//!
//! ```text
//! t_replay = t_capture + offset + drift * (t_capture - reference)
//! ```
//!
//! Methods, each reported separately:
//!
//! - `cockpit_clock`: the cockpit clock appears in both records, so matching
//!   second ticks gives the offset directly.
//! - `motion_correlation`: normalized cross-correlation of altitude, vertical
//!   speed, pitch and roll, repeated over windows to measure drift.
//! - `gear_events`, `hud_events`: shared events, used only as cross-checks.
//!
//! A result is `established` only when a method passes its own tests and a
//! second, independent method agrees, or motion alone is strong over several
//! windows. Otherwise it stays `weak`, `conflict` or `failed`, and validation
//! figures carry that quality.

use crate::systems::{clock_seconds, line_fit, AuthorSample, MAX_INTERPOLATION_GAP_S};
use crate::telemetry::{Capture, Observation};
use serde::Serialize;

const COARSE_STEP_S: f64 = 1.0;
const MIN_OVERLAP_S: f64 = 60.0;
const WINDOW_S: f64 = 120.0;
const WINDOW_STEP_S: f64 = 60.0;
const MIN_WINDOW_SCORE: f64 = 0.9;
const MIN_SCORE: f64 = 0.95;
const MIN_MARGIN: f64 = 0.05;
/// Offsets within this many seconds of the best belong to the same peak.
const PEAK_EXCLUSION_S: f64 = 10.0;
const MIN_CLOCK_TICKS: usize = 20;
const MAX_CLOCK_SPREAD_S: f64 = 0.3;
/// A jump of a window's offset beyond this means the clocks did not run
/// together, for example the game was paused.
const DISCONTINUITY_S: f64 = 0.75;
/// Floor for any reported offset uncertainty: cockpit message latency in the
/// replay (median 36 ms) and the replay sample interval.
const UNCERTAINTY_FLOOR_S: f64 = 0.05;
/// Two methods agree within this, beyond their combined uncertainty.
const AGREEMENT_SLACK_S: f64 = 0.25;

/// Replay state interpolated at one instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interpolated {
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub speed_mps: f64,
    pub saturated: bool,
    pub heading_deg: f64,
    pub pitch_deg: f64,
    pub bank_deg: f64,
    /// Index of the sample at or before this time.
    pub index: usize,
}

fn lerp_angle(a: f64, b: f64, f: f64) -> f64 {
    let t = a + crate::flight::angle_diff_deg(b, a) * f;
    crate::flight::angle_diff_deg(t, 0.0)
}

/// Replay state at `t_s` seconds, between two samples of one uid. `samples`
/// must be in time order.
pub fn interpolate(samples: &[AuthorSample], t_s: f64) -> Option<Interpolated> {
    let t_ms = t_s * 1000.0;
    let i = samples.partition_point(|s| (s.t_ms as f64) <= t_ms);
    let (a, b) = (samples.get(i.checked_sub(1)?)?, samples.get(i)?);
    let span = (b.t_ms - a.t_ms) as f64;
    if span <= 0.0 || span > MAX_INTERPOLATION_GAP_S * 1000.0 || a.uid != b.uid {
        return None;
    }
    let f = (t_ms - a.t_ms as f64) / span;
    let mix = |x: f64, y: f64| x + (y - x) * f;
    Some(Interpolated {
        position: [0, 1, 2].map(|k| mix(a.position[k], b.position[k])),
        velocity: [0, 1, 2].map(|k| mix(a.velocity[k], b.velocity[k])),
        speed_mps: mix(a.speed_mps, b.speed_mps),
        saturated: a.saturated || b.saturated,
        heading_deg: lerp_angle(a.heading_deg, b.heading_deg, f),
        pitch_deg: mix(a.pitch_deg, b.pitch_deg),
        bank_deg: lerp_angle(a.bank_deg, b.bank_deg, f),
        index: i - 1,
    })
}

/// One aligned signal: capture points and the matching replay quantity.
struct Channel {
    name: &'static str,
    points: Vec<(f64, f64)>,
    replay: fn(&Interpolated) -> f64,
    min_std: f64,
}

fn points(obs: &[Observation], key: &str) -> Vec<(f64, f64)> {
    obs.iter().filter_map(|o| Some((o.t_s, o.get(key)?))).collect()
}

fn std_dev(v: &[f64]) -> f64 {
    let n = v.len() as f64;
    if v.is_empty() {
        return 0.0;
    }
    let m = v.iter().sum::<f64>() / n;
    (v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / n).sqrt()
}

fn channels(c: &Capture) -> Vec<Channel> {
    let mut v = vec![
        Channel { name: "altitude", points: points(&c.state, "H, m"), replay: |i| i.position[1], min_std: 5.0 },
        Channel { name: "vertical_speed", points: points(&c.state, "Vy, m/s"), replay: |i| i.velocity[1], min_std: 1.0 },
        Channel { name: "pitch", points: points(&c.indicators, "aviahorizon_pitch"), replay: |i| i.pitch_deg, min_std: 1.0 },
        Channel { name: "roll", points: points(&c.indicators, "aviahorizon_roll"), replay: |i| i.bank_deg, min_std: 3.0 },
    ];
    v.retain(|ch| ch.points.len() >= 30);
    v
}

/// Mean of `|ncc|` over channels with signal in `[lo, hi)` at `offset`.
fn score_at(channels: &[Channel], samples: &[AuthorSample], offset: f64, lo: f64, hi: f64) -> Option<f64> {
    let (mut total, mut used) = (0.0, 0);
    for ch in channels {
        let stride = (ch.points.len() / 4000).max(1);
        let pairs: Vec<(f64, f64)> = ch.points.iter().step_by(stride).filter(|p| p.0 >= lo && p.0 < hi).filter_map(|p| Some((p.1, (ch.replay)(&interpolate(samples, p.0 + offset)?)))).collect();
        if pairs.len() < 20 {
            continue;
        }
        if std_dev(&pairs.iter().map(|p| p.0).collect::<Vec<_>>()) < ch.min_std {
            continue;
        }
        if let Some(r) = crate::systems::pearson(&pairs) {
            total += r.abs();
            used += 1;
        }
    }
    (used > 0).then(|| total / used as f64)
}

fn binned(points: &[(f64, f64)], t0: f64, n: usize) -> Vec<f64> {
    let (mut sum, mut count) = (vec![0.0; n], vec![0u32; n]);
    for p in points {
        let i = ((p.0 - t0) / COARSE_STEP_S) as isize;
        if i >= 0 && (i as usize) < n {
            sum[i as usize] += p.1;
            count[i as usize] += 1;
        }
    }
    sum.iter().zip(&count).map(|(s, c)| if *c > 0 { s / *c as f64 } else { f64::NAN }).collect()
}

fn ncc_lag(a: &[f64], b: &[f64], lag: isize, min_n: usize) -> Option<f64> {
    let (mut n, mut sa, mut sb, mut saa, mut sbb, mut sab) = (0usize, 0.0, 0.0, 0.0, 0.0, 0.0);
    for (i, x) in a.iter().enumerate() {
        let j = i as isize + lag;
        if j < 0 || j as usize >= b.len() {
            continue;
        }
        let y = b[j as usize];
        if x.is_nan() || y.is_nan() {
            continue;
        }
        n += 1;
        sa += x;
        sb += y;
        saa += x * x;
        sbb += y * y;
        sab += x * y;
    }
    if n < min_n {
        return None;
    }
    let m = n as f64;
    let d = (saa - sa * sa / m) * (sbb - sb * sb / m);
    (d > 1e-9).then(|| (sab - sa * sb / m) / d.sqrt())
}

/// Full search for the correlation peak. Returns `(offset, score, margin)`.
fn coarse_peak(channels: &[Channel], samples: &[AuthorSample]) -> Option<(f64, f64, f64)> {
    let cap_lo = channels.iter().filter_map(|c| c.points.first().map(|p| p.0)).fold(f64::INFINITY, f64::min);
    let cap_hi = channels.iter().filter_map(|c| c.points.last().map(|p| p.0)).fold(f64::NEG_INFINITY, f64::max);
    let (rep_lo, rep_hi) = (samples.first()?.t_ms as f64 / 1000.0, samples.last()?.t_ms as f64 / 1000.0);
    let nc = ((cap_hi - cap_lo) / COARSE_STEP_S) as usize + 1;
    let nr = ((rep_hi - rep_lo) / COARSE_STEP_S) as usize + 1;
    let min_n = (MIN_OVERLAP_S / COARSE_STEP_S) as usize;
    if nc < min_n || nr < min_n {
        return None;
    }
    let mut pairs = Vec::new();
    for ch in channels {
        let a = binned(&ch.points, cap_lo, nc);
        let mut b = vec![f64::NAN; nr];
        for (i, slot) in b.iter_mut().enumerate() {
            if let Some(s) = interpolate(samples, rep_lo + i as f64 * COARSE_STEP_S) {
                *slot = (ch.replay)(&s);
            }
        }
        if std_dev(&a.iter().copied().filter(|x| !x.is_nan()).collect::<Vec<_>>()) >= ch.min_std {
            pairs.push((a, b));
        }
    }
    if pairs.is_empty() {
        return None;
    }
    let mut scores: Vec<(f64, f64)> = Vec::new();
    for lag in -(nc as isize)..(nr as isize) {
        let (mut total, mut used) = (0.0, 0);
        for (a, b) in &pairs {
            if let Some(r) = ncc_lag(a, b, lag, min_n) {
                total += r.abs();
                used += 1;
            }
        }
        if used == pairs.len() {
            let offset = rep_lo - cap_lo + lag as f64 * COARSE_STEP_S;
            scores.push((offset, total / used as f64));
        }
    }
    let best = scores.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1))?;
    let second = scores.iter().filter(|s| (s.0 - best.0).abs() > PEAK_EXCLUSION_S).map(|s| s.1).fold(0.0, f64::max);
    Some((best.0, best.1, best.1 - second))
}

fn refine(channels: &[Channel], samples: &[AuthorSample], around: f64, half: f64, lo: f64, hi: f64) -> Option<(f64, f64)> {
    let search = |c: f64, h: f64, step: f64| -> Option<(f64, f64)> {
        let n = (h / step).round() as i32;
        (-n..=n).filter_map(|i| {
            let o = c + i as f64 * step;
            Some((o, score_at(channels, samples, o, lo, hi)?))
        }).max_by(|a, b| a.1.total_cmp(&b.1))
    };
    let (c, _) = search(around, half, 0.05)?;
    let (o, s) = search(c, 0.05, 0.01)?;
    // Parabola through the top three points.
    let (a, b) = (score_at(channels, samples, o - 0.01, lo, hi), score_at(channels, samples, o + 0.01, lo, hi));
    if let (Some(a), Some(b)) = (a, b) {
        let d = a - 2.0 * s + b;
        if d < 0.0 {
            return Some((o + 0.5 * 0.01 * (a - b) / d, s));
        }
    }
    Some((o, s))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MethodResult {
    pub method: &'static str,
    /// `ok`, `unavailable`, `failed` or `cross_check`.
    pub status: &'static str,
    pub offset_s: Option<f64>,
    pub uncertainty_s: Option<f64>,
    pub drift_ppm: Option<f64>,
    pub n: u64,
    pub score: Option<f64>,
    pub margin: Option<f64>,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WindowOffset {
    pub t_s: f64,
    pub offset_s: f64,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncResult {
    /// `established`, `weak`, `conflict` or `failed`.
    pub quality: &'static str,
    pub method: Option<&'static str>,
    pub offset_s: Option<f64>,
    pub uncertainty_s: Option<f64>,
    pub drift_ppm: Option<f64>,
    /// Drift smaller than this cannot be told from zero in this flight: twice
    /// the offset uncertainty over the aligned span.
    pub drift_resolution_ppm: Option<f64>,
    pub reference_t_s: f64,
    pub discontinuity: bool,
    pub methods: Vec<MethodResult>,
    pub windows: Vec<WindowOffset>,
    pub limitations: Vec<String>,
}

impl SyncResult {
    /// Capture time to replay time. `None` unless an offset exists.
    pub fn to_replay(&self, t: f64) -> Option<f64> {
        let o = self.offset_s?;
        Some(t + o + self.drift_ppm.unwrap_or(0.0) * 1e-6 * (t - self.reference_t_s))
    }

    pub fn usable(&self) -> bool {
        matches!(self.quality, "established" | "weak") && !self.discontinuity
    }
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    v.sort_by(f64::total_cmp);
    (!v.is_empty()).then(|| v[v.len() / 2])
}

fn median_half_rtt(c: &Capture) -> f64 {
    median(c.state.iter().chain(&c.indicators).map(|o| o.half_rtt_s).collect()).unwrap_or(0.0)
}

fn poll_interval(obs: &[Observation]) -> f64 {
    median(obs.windows(2).map(|w| w[1].t_s - w[0].t_s).collect()).unwrap_or(0.1)
}

/// Offset and drift from a set of `(t, offset)` estimates. Returns
/// `(offset at reference, drift ppm or None, stderr, reference)`.
fn fit_offsets(p: &[(f64, f64)]) -> Option<(f64, Option<f64>, f64, f64)> {
    let n = p.len() as f64;
    if p.is_empty() {
        return None;
    }
    let reference = p.iter().map(|x| x.0).sum::<f64>() / n;
    let span = p.iter().map(|x| x.0).fold(f64::NEG_INFINITY, f64::max) - p.iter().map(|x| x.0).fold(f64::INFINITY, f64::min);
    let line = if p.len() >= 3 && span >= 300.0 { line_fit(p) } else { None };
    let mean = p.iter().map(|x| x.1).sum::<f64>() / n;
    let (offset, slope) = match line {
        Some((m, b)) => (m * reference + b, Some(m)),
        None => (mean, None),
    };
    let res: Vec<f64> = p.iter().map(|x| x.1 - offset - slope.unwrap_or(0.0) * (x.0 - reference)).collect();
    Some((offset, slope.map(|m| m * 1e6), std_dev(&res) / n.sqrt(), reference))
}

/// Offset from matching ticks of the cockpit clock.
pub fn clock_method(capture: &Capture, samples: &[AuthorSample]) -> (MethodResult, Option<Vec<(f64, f64)>>) {
    let mut r = MethodResult { method: "cockpit_clock", status: "unavailable", offset_s: None, uncertainty_s: None, drift_ppm: None, n: 0, score: None, margin: None, note: String::new() };
    let cap: Vec<(f64, f64)> = capture.indicators.iter().filter_map(|o| Some((o.t_s, clock_seconds(o.get("clock_hour")?, o.get("clock_min")?, o.get("clock_sec")?)?))).collect();
    let rep: Vec<(f64, f64)> = samples.iter().filter_map(|s| Some((s.t_ms as f64 / 1000.0, s.clock_s()?))).collect();
    if cap.is_empty() || rep.is_empty() {
        r.note = format!("clock samples: capture {}, replay {}", cap.len(), rep.len());
        return (r, None);
    }
    // A tick: the reading changes by one second between neighbours.
    let ticks = |v: &[(f64, f64)]| -> Vec<(f64, f64)> { v.windows(2).filter(|w| w[1].1 - w[0].1 == 1.0 && w[1].0 - w[0].0 < 1.0).map(|w| (0.5 * (w[0].0 + w[1].0), w[1].1)).collect() };
    let rep_ticks: std::collections::BTreeMap<i64, f64> = ticks(&rep).into_iter().map(|t| (t.1 as i64, t.0)).collect();
    let pairs: Vec<(f64, f64)> = ticks(&cap).into_iter().filter_map(|t| Some((t.0, rep_ticks.get(&(t.1 as i64))? - t.0))).collect();
    r.n = pairs.len() as u64;
    if pairs.len() < MIN_CLOCK_TICKS {
        r.status = "failed";
        r.note = format!("{} matching ticks, {MIN_CLOCK_TICKS} needed", pairs.len());
        return (r, None);
    }
    // A 12-hour dial repeats, and a game clock can jump: keep the main group.
    let mid = median(pairs.iter().map(|p| p.1).collect()).unwrap();
    let main: Vec<(f64, f64)> = pairs.iter().copied().filter(|p| (p.1 - mid).abs() <= 2.0).collect();
    let Some((offset, drift, stderr, _)) = fit_offsets(&main) else { return (r, None) };
    let spread = std_dev(&main.iter().map(|p| p.1 - offset).collect::<Vec<_>>());
    r.offset_s = Some(offset);
    r.drift_ppm = drift;
    r.score = Some(main.len() as f64 / pairs.len() as f64);
    r.uncertainty_s = Some(stderr.hypot(UNCERTAINTY_FLOOR_S));
    if spread > MAX_CLOCK_SPREAD_S + 0.5 * poll_interval(&capture.indicators) || main.len() * 10 < pairs.len() * 9 {
        r.status = "failed";
        r.note = format!("tick offsets spread {spread:.3} s; {} of {} in the main group", main.len(), pairs.len());
    } else {
        r.status = "ok";
        r.note = format!("{} ticks matched, spread {spread:.3} s", main.len());
    }
    (r, Some(main))
}

/// Offset and drift from motion.
pub fn motion_method(capture: &Capture, samples: &[AuthorSample]) -> (MethodResult, Vec<WindowOffset>, bool) {
    let mut r = MethodResult { method: "motion_correlation", status: "unavailable", offset_s: None, uncertainty_s: None, drift_ppm: None, n: 0, score: None, margin: None, note: String::new() };
    let ch = channels(capture);
    if ch.is_empty() {
        r.note = "no usable telemetry channels".into();
        return (r, Vec::new(), false);
    }
    let Some((coarse, _, margin)) = coarse_peak(&ch, samples) else {
        r.status = "failed";
        r.note = format!("no overlap of at least {MIN_OVERLAP_S} s with signal, or the flight was too steady");
        return (r, Vec::new(), false);
    };
    let (lo, hi) = (f64::NEG_INFINITY, f64::INFINITY);
    let Some((offset, score)) = refine(&ch, samples, coarse, 1.5, lo, hi) else {
        r.status = "failed";
        return (r, Vec::new(), false);
    };
    r.offset_s = Some(offset);
    r.score = Some(score);
    r.margin = Some(margin);

    let t0 = ch.iter().filter_map(|c| c.points.first().map(|p| p.0)).fold(f64::INFINITY, f64::min);
    let t1 = ch.iter().filter_map(|c| c.points.last().map(|p| p.0)).fold(f64::NEG_INFINITY, f64::max);
    let mut windows = Vec::new();
    let mut w = t0;
    while w + WINDOW_S <= t1 + 1.0 {
        if let Some((o, s)) = refine(&ch, samples, offset, 2.0, w, w + WINDOW_S) {
            if s >= MIN_WINDOW_SCORE {
                windows.push(WindowOffset { t_s: w + WINDOW_S / 2.0, offset_s: o, score: s });
            }
        }
        w += WINDOW_STEP_S;
    }
    let p: Vec<(f64, f64)> = windows.iter().map(|w| (w.t_s, w.offset_s)).collect();
    let mut discontinuity = false;
    let mut floor = UNCERTAINTY_FLOOR_S.hypot(median_half_rtt(capture)).hypot(poll_interval(&capture.state) / 12f64.sqrt());
    r.n = p.len() as u64;
    if let Some((o, drift, stderr, _)) = fit_offsets(&p) {
        if p.len() >= 3 {
            r.offset_s = Some(o);
            r.drift_ppm = drift;
            let m = drift.unwrap_or(0.0) * 1e-6;
            let refer = p.iter().map(|x| x.0).sum::<f64>() / p.len() as f64;
            discontinuity = p.iter().any(|x| (x.1 - o - m * (x.0 - refer)).abs() > DISCONTINUITY_S) || p.windows(3).any(|w| w[2].0 - w[0].0 <= 3.0 * WINDOW_STEP_S && (w[2].1 - w[0].1).abs() > DISCONTINUITY_S);
            floor = floor.hypot(stderr);
        } else {
            floor = floor.hypot(0.5);
        }
    } else {
        floor = floor.hypot(0.5);
    }
    r.uncertainty_s = Some(floor);
    let strong = score >= MIN_SCORE && margin >= MIN_MARGIN;
    r.status = if strong && !discontinuity { "ok" } else { "failed" };
    let names: Vec<&str> = ch.iter().map(|c| c.name).collect();
    r.note = format!("{} windows accepted; peak {score:.3}, margin {margin:.3}; channels {}{}", p.len(), names.join(", "), if discontinuity { "; offsets jump between windows" } else { "" });
    (r, windows, discontinuity)
}

/// Gear transitions in both records, mapped through a known offset.
fn gear_check(capture: &Capture, samples: &[AuthorSample], to_replay: &dyn Fn(f64) -> f64) -> MethodResult {
    let mut r = MethodResult { method: "gear_events", status: "unavailable", offset_s: None, uncertainty_s: None, drift_ppm: None, n: 0, score: None, margin: None, note: String::new() };
    let tel: Vec<(f64, f64)> = capture.indicators.iter().filter_map(|o| Some((o.t_s, o.get("gears")?))).collect();
    let mut tel_events: Vec<f64> = tel.windows(2).filter(|w| (w[0].1 < 0.5) != (w[1].1 < 0.5)).map(|w| to_replay(0.5 * (w[0].0 + w[1].0))).collect();
    let rep_events: Vec<f64> = samples.windows(2).filter(|w| w[0].gear_bit != w[1].gear_bit && w[0].uid == w[1].uid).map(|w| w[1].t_ms as f64 / 1000.0).collect();
    tel_events.retain(|t| rep_events.iter().any(|e| (e - t).abs() <= 10.0));
    let diffs: Vec<f64> = tel_events.iter().map(|t| rep_events.iter().map(|e| e - t).min_by(|a, b| a.abs().total_cmp(&b.abs())).unwrap()).collect();
    r.n = diffs.len() as u64;
    if let Some(m) = median(diffs) {
        r.status = "cross_check";
        r.offset_s = Some(m);
        r.note = "Median replay gear-bit change minus 8111 gears crossing 0.5, after alignment. The two may mark different moments of the gear cycle".into();
    } else {
        r.note = "no gear transition in both records".into();
    }
    r
}

/// HUD messages against replay kills, mapped through a known offset.
fn hud_check(capture: &Capture, kills_s: &[f64], to_replay: &dyn Fn(f64) -> f64) -> MethodResult {
    let mut r = MethodResult { method: "hud_events", status: "unavailable", offset_s: None, uncertainty_s: None, drift_ppm: None, n: 0, score: None, margin: None, note: String::new() };
    let diffs: Vec<f64> = capture.hud.iter().filter_map(|h| {
        let t = to_replay(h.t_s);
        kills_s.iter().map(|k| t - k).filter(|d| d.abs() <= 5.0).min_by(|a, b| a.abs().total_cmp(&b.abs()))
    }).collect();
    let timed: Vec<f64> = capture.hud.iter().filter_map(|h| {
        let t = h.time_s?;
        kills_s.iter().map(|k| t - k).filter(|d| d.abs() <= 5.0).min_by(|a, b| a.abs().total_cmp(&b.abs()))
    }).collect();
    r.n = diffs.len() as u64;
    if let Some(m) = median(diffs) {
        r.status = "cross_check";
        r.offset_s = Some(m);
        r.note = format!("Median time from a replay kill to the first sight of a HUD message, after alignment. It should be small and positive. {} of {} messages paired.{}", r.n, capture.hud.len(), median(timed).map_or(String::new(), |t| format!(" HUD time field minus replay kill time: median {t:.3} s.")));
    } else {
        r.note = format!("{} HUD messages, none within 5 s of a replay kill", capture.hud.len());
    }
    r
}

/// Estimate the capture-to-replay relation. `kills_s` are replay kill times.
pub fn synchronize(capture: &Capture, samples: &[AuthorSample], kills_s: &[f64]) -> SyncResult {
    let (clock, ticks) = clock_method(capture, samples);
    let (motion, windows, discontinuity) = motion_method(capture, samples);
    let mut limitations = Vec::new();
    let (clock_ok, motion_ok) = (clock.status == "ok", motion.status == "ok");
    let pick = if clock_ok { Some(clock.clone()) } else if motion_ok { Some(motion.clone()) } else { None };
    let mut quality = "failed";
    if clock_ok && motion_ok {
        let gap = (clock.offset_s.unwrap() - motion.offset_s.unwrap()).abs();
        let allowed = clock.uncertainty_s.unwrap().hypot(motion.uncertainty_s.unwrap()) * 3.0 + AGREEMENT_SLACK_S;
        if gap <= allowed {
            quality = "established";
        } else {
            quality = "conflict";
            limitations.push(format!("Clock and motion offsets differ by {gap:.3} s, more than the {allowed:.3} s allowed"));
        }
    } else if clock_ok {
        quality = "weak";
        limitations.push("Only the cockpit clock gave an offset. Motion did not confirm it".into());
    } else if motion_ok {
        if windows.len() >= 3 {
            quality = "established";
            limitations.push("Motion correlation only. The cockpit clock gave no confirmation".into());
        } else {
            quality = "weak";
            limitations.push("Motion correlation with fewer than 3 windows, so drift is not measured".into());
        }
    } else {
        limitations.push("No method passed its tests. Replay and capture are not aligned, so no comparison is valid".into());
    }
    if discontinuity {
        limitations.push("Offsets jump between windows. The clocks did not run together, possibly a game pause, so a single offset does not apply".into());
    }
    let offset_s = pick.as_ref().and_then(|m| m.offset_s);
    let uncertainty_s = pick.as_ref().and_then(|m| m.uncertainty_s);
    let drift_ppm = pick.as_ref().and_then(|m| m.drift_ppm);
    if offset_s.is_some() && drift_ppm.is_none() {
        limitations.push("Drift not measured: it needs at least 300 s of aligned data".into());
    }
    let reference_t_s = match (pick.as_ref().map(|m| m.method), &ticks) {
        (Some("cockpit_clock"), Some(t)) => t.iter().map(|x| x.0).sum::<f64>() / t.len() as f64,
        _ if !windows.is_empty() => windows.iter().map(|w| w.t_s).sum::<f64>() / windows.len() as f64,
        _ => 0.0,
    };
    let span = match (pick.as_ref().map(|m| m.method), &ticks) {
        (Some("cockpit_clock"), Some(t)) => t.iter().map(|x| x.0).fold(f64::NEG_INFINITY, f64::max) - t.iter().map(|x| x.0).fold(f64::INFINITY, f64::min),
        _ => windows.last().zip(windows.first()).map_or(0.0, |(a, b)| a.t_s - b.t_s),
    };
    let drift_resolution_ppm = uncertainty_s.filter(|_| span > 0.0).map(|u| 2.0 * u / span * 1e6);
    let mut methods = vec![clock, motion];
    if let Some(o) = offset_s {
        let (d, reference) = (drift_ppm.unwrap_or(0.0) * 1e-6, reference_t_s);
        let map = move |t: f64| t + o + d * (t - reference);
        methods.push(gear_check(capture, samples, &map));
        methods.push(hud_check(capture, kills_s, &map));
    }
    SyncResult { quality, method: pick.map(|m| m.method), offset_s, uncertainty_s, drift_ppm, drift_resolution_ppm, reference_t_s, discontinuity, methods, windows, limitations }
}
