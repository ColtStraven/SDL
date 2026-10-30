//! Synthetic author flights for the telemetry tests. Motion is an arbitrary
//! but non-repeating sequence of turns, climbs and dives.
#![allow(dead_code)]
use sdl_replay_decoder::cockpit::id;
use sdl_replay_decoder::systems::AuthorSample;

/// `(session seconds, heading deg, pitch deg, bank deg)` along the flight.
fn attitude(t: f64) -> (f64, f64, f64) {
    let h = 30.0 * (t / 37.0).sin() + 0.8 * t;
    let p = 6.0 * (t / 11.0).sin() + 3.0 * (t / 3.1).sin();
    let b = 40.0 * (t / 7.0).sin() * (t / 23.0).cos();
    (h, p, b)
}

/// A 4 Hz flight of `seconds` starting at session time `start_s`.
pub fn flight(start_s: f64, seconds: f64, vehicle: &str) -> Vec<AuthorSample> {
    let mut out = Vec::new();
    let mut p = [1_000.0, 2_000.0, -3_000.0];
    let n = (seconds * 4.0) as u32;
    for i in 0..n {
        let t = i as f64 / 4.0;
        let (h, pitch, bank) = attitude(t);
        let wrapped = (h + 180.0).rem_euclid(360.0) - 180.0;
        let speed = 200.0 + 30.0 * (t / 19.0).sin();
        let (hr, th) = (h.to_radians(), pitch.to_radians());
        let v = [speed * th.cos() * hr.cos(), speed * th.sin(), -speed * th.cos() * hr.sin()];
        let session = start_s + t;
        let clock = 5.0 * 3600.0 + session;
        let throttle = 0.6 + 0.3 * (t / 13.0).sin();
        let gear = t < 20.0;
        let cockpit = vec![
            (id::AIRSPEED, (speed * 0.8) as f32),
            (id::COMPASS, (wrapped + 90.0).rem_euclid(360.0) as f32),
            (id::CLOCK_HOUR, (clock / 3600.0) as f32),
            (id::CLOCK_MIN, ((clock % 3600.0) / 60.0).floor() as f32),
            (id::CLOCK_SEC, (clock % 60.0).floor() as f32),
            (id::THROTTLE, throttle as f32),
            (id::GEAR, if gear { 1.0 } else { 0.0 }),
            (id::FUEL, (1_000.0 - 0.5 * t) as f32),
        ];
        out.push(AuthorSample {
            t_ms: (session * 1000.0) as u32,
            uid: 7,
            vehicle: Some(vehicle.to_string()),
            position: p,
            velocity: v,
            speed_mps: speed,
            saturated: false,
            heading_deg: wrapped,
            pitch_deg: pitch,
            bank_deg: bank,
            gear_bit: gear,
            controls: [0x08, 0x88, 0, 0, 0, (throttle * 15.0).floor() as u8, 0],
            engines: 1,
            engine_state: Some(7),
            boost_raw: Some((throttle / 1.1 * 32767.0) as i16),
            cockpit,
        });
        for k in 0..3 {
            p[k] += v[k] / 4.0;
        }
    }
    out
}

/// A level, constant flight: nothing to correlate.
pub fn steady(start_s: f64, seconds: f64) -> Vec<AuthorSample> {
    let mut v = flight(start_s, seconds, "steady");
    for (i, s) in v.iter_mut().enumerate() {
        s.position = [i as f64 * 50.0, 2_000.0, 0.0];
        s.velocity = [200.0, 0.0, 0.0];
        s.speed_mps = 200.0;
        s.heading_deg = 0.0;
        s.pitch_deg = 0.0;
        s.bank_deg = 0.0;
        s.cockpit.clear();
    }
    v
}
