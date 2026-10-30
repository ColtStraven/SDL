//! Research probe: independent check of decoded launchers. For each kill by a
//! non-gun weapon on an aircraft, take the weapon that ended nearest the
//! victim with no launcher constraint, and compare its decoded launcher with
//! the kill record's killer.
use sdl_replay_decoder::lab::extract;
use sdl_replay_decoder::manifest::Options;
use sdl_replay_decoder::tracks::TrackConfig;
use sdl_replay_decoder::weapons::is_gun_round;
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files: Vec<_> = std::fs::read_dir(&args[1]).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "wrpl")).collect();
    files.sort();
    let (mut tested, mut uid_ok, mut uid_bad, mut no_uid) = (0u64, 0u64, 0u64, 0u64);
    let (mut veh_ok, mut veh_bad) = (0u64, 0u64);
    let mut dist: Vec<f64> = vec![];
    let mut by_kind: BTreeMap<String, u64> = BTreeMap::new();
    let mut no_uid_tmpl: BTreeMap<String, u64> = BTreeMap::new();
    let mut examples = vec![];
    let mut split: BTreeMap<String, u64> = BTreeMap::new();
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let x = extract("x", &bytes, Options::default(), TrackConfig::default(), |_| {}).unwrap();
        for w in &x.weapons {
            *by_kind.entry(format!("{} {:?}", w.launcher.status, w.launcher.kind)).or_default() += 1;
            if w.launcher.status == "creation_record" {
                if w.launcher.kind == Some("aircraft") {
                    if let Some(d) = w.launcher.distance_m { dist.push(d); }
                }
                if w.launcher.uid.is_none() {
                    *no_uid_tmpl.entry(w.launcher.kind.unwrap_or("?").to_string()).or_default() += 1;
                }
            }
        }
        for k in &x.kills {
            let (Some(victim), Some(killer)) = (k.victim_uid, k.killer_uid) else { continue };
            if k.victim_surface || k.weapon.as_deref().is_none_or(is_gun_round) { continue; }
            let Some(v) = x.history.at(victim, k.timestamp_ms) else { continue };
            let best = x.weapons.iter().filter(|w| w.trajectory.last_ms <= k.timestamp_ms + 1_500 && w.trajectory.last_ms + 3_000 >= k.timestamp_ms).filter_map(|w| {
                let e = w.trajectory.last_position_m;
                let d = ((e[0] as f64 - v[0]).powi(2) + (e[1] as f64 - v[1]).powi(2) + (e[2] as f64 - v[2]).powi(2)).sqrt();
                (d <= 400.0).then_some((w, d))
            }).min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            let Some((w, _)) = best else { continue };
            tested += 1;
            match w.launcher.uid {
                None => no_uid += 1,
                Some(u) if u == killer && (w.launcher.kind != Some("aircraft")) == k.killer_surface => uid_ok += 1,
                Some(u) => {
                    uid_bad += 1;
                    let same_model = w.launcher.vehicle == k.killer_vehicle;
                    let same_name = w.weapon_name.as_deref().map(|n| n.trim_end_matches("_default")) == k.weapon.as_deref().map(|n| n.trim_end_matches("_default"));
                    *split.entry(format!("same launcher model {same_model}, weapon name matches kill {same_name}")).or_default() += 1;
                    if examples.len() < 12 {
                        examples.push(format!("{} killer {killer} surface {} ({:?}) launcher {u} {:?} ({:?}) weapon {:?}", f.file_name().unwrap().to_string_lossy(), k.killer_surface, k.killer_vehicle, w.launcher.kind, w.launcher.vehicle, k.weapon));
                    }
                }
            }
            match (&w.launcher.vehicle, &k.killer_vehicle) {
                (Some(a), Some(b)) if a == b => veh_ok += 1,
                (Some(_), Some(_)) => veh_bad += 1,
                _ => {}
            }
        }
    }
    dist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| dist[((dist.len() - 1) as f64 * p) as usize];
    println!("launcher status {by_kind:?}");
    println!("no uid by kind {no_uid_tmpl:?}");
    println!("launch point to launcher m: p50 {:.0} p90 {:.0} p99 {:.0} max {:.0} n {}", q(0.5), q(0.9), q(0.99), q(1.0), dist.len());
    println!("independent kill check: tested {tested}, uid agrees {uid_ok}, differs {uid_bad}, launcher has no uid {no_uid}; vehicle agrees {veh_ok}, differs {veh_bad}");
    println!("differences {split:?}");
    for e in examples { println!("  {e}"); }
}
