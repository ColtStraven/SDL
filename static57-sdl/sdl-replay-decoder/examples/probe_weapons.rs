//! Research probe: weapon statistics and type-key to weapon-name cross-tab.
use sdl_replay_decoder::lab::extract;
use sdl_replay_decoder::manifest::Options;
use sdl_replay_decoder::tracks::TrackConfig;
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files: Vec<_> = std::fs::read_dir(&args[1]).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "wrpl")).collect();
    files.sort();
    let mut xtab: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut tmpl: BTreeMap<String, u64> = BTreeMap::new();
    let mut status: BTreeMap<String, u64> = BTreeMap::new();
    let mut dists = vec![];
    let mut victim_d = vec![];
    let mut named = vec![];
    let mut totals = serde_json::Map::new();
    for file in files.iter() {
        let bytes = std::fs::read(file).unwrap();
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        let x = extract(&name, &bytes, Options::default(), TrackConfig::default(), |_| {}).unwrap();
        for (k, v) in serde_json::to_value(&x.weapon_stats).unwrap().as_object().unwrap() {
            let cur = totals.get(k).and_then(|c: &serde_json::Value| c.as_u64()).unwrap_or(0);
            totals.insert(k.clone(), (cur + v.as_u64().unwrap()).into());
        }
        for w in &x.weapons {
            *tmpl.entry(w.template.clone()).or_default() += 1;
            *status.entry(format!("{} {}", w.launcher.status, w.outcome.status)).or_default() += 1;
            if let Some(d) = w.launcher.distance_m {
                dists.push(d);
            }
            if let Some(d) = w.outcome.distance_to_victim_m {
                victim_d.push(d);
            }
            let key = w.creation_prefix.clone().unwrap_or_else(|| "none".into());
            if let Some(n) = &w.weapon_name {
                *xtab.entry(format!("{}|{key}", w.template)).or_default().entry(n.clone()).or_default() += 1;
                if named.len() < 14 {
                    named.push(format!("{n} launcher={:?} author={} lalt={:?} tof={:?} range={:.0} maxv={:.0} d={:?}", w.launcher.vehicle, w.launcher.is_author, w.trajectory.launch_altitude_m, w.trajectory.time_of_flight_s, w.trajectory.ground_range_m, w.trajectory.max_speed_mps, w.outcome.distance_to_victim_m));
                }
            }
        }
    }
    println!("{}", serde_json::Value::Object(totals));
    println!("templates {tmpl:?}");
    println!("status {status:?}");
    let q = |v: &mut Vec<f64>, p: f64| { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); if v.is_empty() { 0.0 } else { v[((v.len() - 1) as f64 * p) as usize] } };
    println!("launcher dist p10 {:.0} p50 {:.0} p90 {:.0} n {}", q(&mut dists, 0.1), q(&mut dists, 0.5), q(&mut dists, 0.9), dists.len());
    println!("victim dist p50 {:.0} p90 {:.0} n {}", q(&mut victim_d, 0.5), q(&mut victim_d, 0.9), victim_d.len());
    for n in named { println!("{n}"); }
    let mut multi = 0;
    for (k, m) in &xtab {
        if m.len() > 1 { multi += 1; }
        println!("{k} -> {m:?}");
    }
    println!("keys {} multi-name {multi}", xtab.len());
}
