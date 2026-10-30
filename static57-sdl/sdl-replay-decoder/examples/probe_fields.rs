//! Research probe: each control nibble value against chosen 8111 fields, after
//! alignment. Usage: probe_fields <capture dir> <replay.wrpl>
use sdl_replay_decoder::lab::extract;
use sdl_replay_decoder::manifest::Options;
use sdl_replay_decoder::sync::synchronize;
use sdl_replay_decoder::systems::{pearson, AuthorSample};
use sdl_replay_decoder::telemetry::{load, Observation};
use sdl_replay_decoder::tracks::TrackConfig;
use std::collections::BTreeMap;

fn nearest<'a>(obs: &'a [Observation], t: f64) -> Option<&'a Observation> {
    let i = obs.partition_point(|o| o.t_s < t);
    [i.checked_sub(1), Some(i)].into_iter().flatten().filter_map(|j| obs.get(j)).min_by(|a, b| (a.t_s - t).abs().total_cmp(&(b.t_s - t).abs())).filter(|o| (o.t_s - t).abs() <= 0.06)
}

fn table(name: &str, samples: &[AuthorSample], to_cap: &dyn Fn(f64) -> f64, obs: &[Observation], key: &str, f: &dyn Fn(&AuthorSample) -> Option<f64>) {
    let mut by: BTreeMap<i64, Vec<f64>> = BTreeMap::new();
    let mut pairs = Vec::new();
    for s in samples {
        let Some(o) = nearest(obs, to_cap(s.t_ms as f64 / 1000.0)) else { continue };
        if let (Some(v), Some(x)) = (f(s), o.get(key)) {
            by.entry((v * 1000.0).round() as i64).or_default().push(x);
            pairs.push((v, x));
        }
    }
    let rows: Vec<String> = by.iter_mut().map(|(k, v)| {
        v.sort_by(f64::total_cmp);
        format!("{}:{:.2}({})", *k as f64 / 1000.0, v[v.len() / 2], v.len())
    }).collect();
    let shown = if rows.len() > 18 { format!("{} distinct", rows.len()) } else { rows.join(" ") };
    println!("{name} vs {key}: r {:?} n {} | {shown}", pearson(&pairs).map(|r| (r * 1000.0).round() / 1000.0), pairs.len());
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = load(std::path::Path::new(&a[1])).unwrap();
    let bytes = std::fs::read(&a[2]).unwrap();
    let x = extract("x", &bytes, Options::default(), TrackConfig::default(), |_| {}).unwrap();
    let r = synchronize(&c, &x.author, &[]);
    let (o, d, t0) = (r.offset_s.unwrap(), r.drift_ppm.unwrap_or(0.0) * 1e-6, r.reference_t_s);
    let to_cap = move |t: f64| (t - o + d * t0) / (1.0 + d);
    let s = &x.author;
    let n = |i: usize| move |x: &AuthorSample| Some(x.nibble(i) as f64);
    for (name, i, st, key) in [
        ("n0", 0, true, "flaps, %"), ("n1", 1, true, "elevator, %"), ("n1", 1, false, "stick_elevator"), ("n2", 2, true, "aileron, %"), ("n2", 2, false, "stick_ailerons"),
        ("n3", 3, true, "rudder, %"), ("n3", 3, false, "pedals"), ("n4", 4, false, "blister1"), ("n11", 11, true, "throttle 1, %"),
    ] {
        table(name, s, &to_cap, if st { &c.state } else { &c.indicators }, key, &n(i));
    }
    table("boost", s, &to_cap, &c.state, "throttle 1, %", &|x| x.throttle());
    table("gear_bit", s, &to_cap, &c.state, "gear, %", &|x| Some(x.gear_bit as u8 as f64));
    table("engine_state", s, &to_cap, &c.state, "RPM 1", &|x| x.engine_state.map(|e| e as f64));
    for (id, key, st) in [(195u16, "Mfuel, kg", true), (240, "throttle 1, %", true), (240, "throttle", false), (23, "stick_elevator", false), (31, "stick_ailerons", false), (15, "pedals", false), (482, "Ny", true), (482, "g_meter", false), (81, "RPM 1", true)] {
        table(&format!("id{id}"), s, &to_cap, if st { &c.state } else { &c.indicators }, key, &|x| x.cockpit(id));
    }
    let ids: std::collections::BTreeSet<u16> = s.iter().flat_map(|x| x.cockpit.iter().map(|p| p.0)).collect();
    println!("cockpit ids in this replay: {ids:?}");
}
