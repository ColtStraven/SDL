//! Synthetic 8111 captures, generated from replay samples with a known clock
//! relation, noise and wind. They exist to test the capture, synchronization
//! and validation code before a real flight is recorded.
//!
//! Every capture written here sets `synthetic: true`. Reports from one are
//! marked as such and can never count as independent validation, because the
//! "telemetry" is the replay itself.

use crate::export::write_json;
use crate::cockpit::id::METRES_PER_GAME_FOOT;
use crate::systems::{load_factor, AuthorSample, BOOST_FULL_SCALE, COMPASS_MINUS_HEADING_DEG};
use crate::sync::interpolate;
use crate::telemetry::{default_endpoints, utc_now_ms, CaptureRecord, SessionMeta, CAPTURE_FILE, CAPTURE_SCHEMA, SESSION_FILE};
use serde_json::json;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct SimConfig {
    /// `t_replay = t_capture + offset + drift * (t_capture - 0)`.
    pub offset_s: f64,
    pub drift_ppm: f64,
    pub hz: f64,
    pub noise: f64,
    /// Added to true airspeed relative to replay ground speed, m/s.
    pub wind_mps: f64,
    pub include_clock: bool,
    pub map_half_extent_m: f64,
    /// Seconds of capture before the replay's first sample.
    pub lead_in_s: f64,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig { offset_s: 1234.5, drift_ppm: 0.0, hz: 10.0, noise: 0.02, wind_mps: 0.0, include_clock: true, map_half_extent_m: 65_536.0, lead_in_s: 5.0 }
    }
}

/// Deterministic noise in `[-1, 1)`.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }
}

/// Write a synthetic capture to `dir`. Refuses to overwrite one.
pub fn write_capture(samples: &[AuthorSample], cfg: &SimConfig, dir: &Path) -> io::Result<u64> {
    fs::create_dir_all(dir)?;
    let path = dir.join(CAPTURE_FILE);
    if path.exists() {
        return Err(io::Error::other(format!("{} already exists", path.display())));
    }
    let (Some(first), Some(last)) = (samples.first(), samples.last()) else { return Err(io::Error::other("no samples")) };
    let scale = 1.0 + cfg.drift_ppm * 1e-6;
    let t_cap = |t_replay: f64| (t_replay - cfg.offset_s) / scale;
    let (t0, t1) = (t_cap(first.t_ms as f64 / 1000.0) - cfg.lead_in_s, t_cap(last.t_ms as f64 / 1000.0));
    let utc0 = utc_now_ms();
    let meta = SessionMeta {
        schema: CAPTURE_SCHEMA.to_string(),
        decoder_version: env!("CARGO_PKG_VERSION").to_string(),
        synthetic: true,
        host: "synthetic".into(),
        port: 0,
        computer: None,
        started_utc_ms: utc0,
        started_utc: "synthetic".into(),
        ended_utc_ms: None,
        duration_s: Some(t1 - t0),
        stop_reason: Some("synthetic".into()),
        endpoints: default_endpoints(),
    };
    write_json(&dir.join(SESSION_FILE), &meta)?;
    let mut w = BufWriter::new(File::create(&path)?);
    let mut noise = Noise(0x9E37_79B9_7F4A_7C15);
    let half = cfg.map_half_extent_m;
    let span = 2.0 * half;
    let mut seq = 0u64;
    let mut emit = |w: &mut BufWriter<File>, endpoint: &str, t: f64, body: String, valid: Option<bool>| -> io::Result<()> {
        seq += 1;
        let r = CaptureRecord { seq, endpoint: endpoint.into(), t_start_s: t - 0.002, t_end_s: t + 0.002, utc_ms: utc0 + ((t - t0) * 1000.0) as u64, status: Some(200), ok: true, valid, bytes: body.len() as u64, error: None, note: None, body: Some(body) };
        serde_json::to_writer(&mut *w, &r)?;
        w.write_all(b"\n")
    };
    let info = json!({"grid_steps":[8192.0,8192.0],"grid_zero":[-half,half],"map_generation":1,"map_max":[half,half],"map_min":[-half,-half],"valid":true}).to_string();
    emit(&mut w, "map_info", t0, info, Some(true))?;
    let steps = ((t1 - t0) * cfg.hz) as u64;
    for k in 0..=steps {
        let t = t0 + k as f64 / cfg.hz;
        let tr = t * scale + cfg.offset_s;
        let Some(s) = interpolate(samples, tr) else {
            emit(&mut w, "state", t, "{\"valid\":false}".into(), Some(false))?;
            continue;
        };
        let near = &samples[s.index];
        // The nearer sample, so a clock tick falls midway between samples.
        let nearest = samples.get(s.index + 1).filter(|n| (n.t_ms as f64) - tr * 1000.0 < tr * 1000.0 - near.t_ms as f64).unwrap_or(near);
        let ny = samples.get(s.index + 1).filter(|_| s.index > 0).and_then(|n| load_factor(&samples[s.index - 1], near, n));
        let mut n = |a: f64| a * cfg.noise * noise.next();
        let tas = (s.speed_mps + cfg.wind_mps).max(0.0);
        let ias = tas * crate::flight::isa_density_ratio(s.position[1]).sqrt();
        let throttle = near.throttle().unwrap_or(near.nibble(11) as f64 / 15.0) * 100.0;
        let state = json!({"valid":true,"H, m":s.position[1] + n(5.0),"TAS, km/h":tas * 3.6 + n(5.0),"IAS, km/h":ias * 3.6 + n(5.0),"Vy, m/s":s.velocity[1] + n(5.0),"Ny":ny.unwrap_or(1.0) + n(5.0),"throttle 1, %":throttle,"gear, %":if near.gear_bit { 100.0 } else { 0.0 }});
        emit(&mut w, "state", t, state.to_string(), Some(true))?;
        let mut ind = json!({"valid":true,"type":near.vehicle.clone().unwrap_or_default(),"altitude_10k":s.position[1] / METRES_PER_GAME_FOOT,"aviahorizon_pitch":s.pitch_deg + n(5.0),"aviahorizon_roll":s.bank_deg + n(5.0),"compass":(s.heading_deg + COMPASS_MINUS_HEADING_DEG).rem_euclid(360.0),"gears":if near.gear_bit { 1.0 } else { 0.0 },"throttle":near.boost_raw.map_or(0.0, |b| b as f64 / BOOST_FULL_SCALE),"stick_ailerons":(near.nibble(2) as f64 - 8.0) / 7.0});
        if cfg.include_clock {
            if let Some(c) = nearest.clock_s() {
                ind["clock_hour"] = json!((c / 3600.0).floor() + (c % 3600.0) / 3600.0);
                ind["clock_min"] = json!(((c % 3600.0) / 60.0).floor());
                ind["clock_sec"] = json!(c % 60.0);
            }
        }
        emit(&mut w, "indicators", t, ind.to_string(), Some(true))?;
        if k % (cfg.hz / 2.0).max(1.0) as u64 == 0 {
            let mx = (s.position[0] + half) / span;
            let my = (half - s.position[2]) / span;
            let c = (s.heading_deg + COMPASS_MINUS_HEADING_DEG).to_radians();
            // Map y grows southward.
            let obj = json!([{"type":"aircraft","icon":"Player","x":mx,"y":my,"dx":c.sin(),"dy":-c.cos()}]);
            emit(&mut w, "map_obj", t, obj.to_string(), None)?;
        }
    }
    w.flush()?;
    Ok(seq)
}
