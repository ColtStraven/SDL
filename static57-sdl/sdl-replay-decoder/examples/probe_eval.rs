//! Research probe: corpus totals, including target method evaluation.
use sdl_replay_decoder::lab::extract;
use sdl_replay_decoder::manifest::Options;
use sdl_replay_decoder::tracks::TrackConfig;
use std::collections::BTreeMap;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files: Vec<_> = std::fs::read_dir(&args[1]).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "wrpl")).collect();
    files.sort();
    let mut ev: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
    let (mut linked, mut named, mut kill_named, mut hc, mut ha, mut wl, mut hits, mut dmg) = (0, 0, 0, 0, 0, 0, 0, 0);
    let mut surface: BTreeMap<String, u64> = BTreeMap::new();
    let mut classes: BTreeMap<&str, u64> = BTreeMap::new();
    let mut points: BTreeMap<u8, (u64, u64)> = BTreeMap::new();
    let (mut rej, mut seek, mut killt, mut geo, mut exact, mut au, mut au_named, mut au_target, mut au_geo) = (0, 0, 0, 0, 0, 0, 0, 0, 0);
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let x = extract("x", &bytes, Options::default(), TrackConfig::default(), |_| {}).unwrap();
        for (k, v) in &x.weapon_stats.target_eval { let e = ev.entry(k).or_default(); e.0 += v.0; e.1 += v.1; }
        linked += x.weapon_stats.kills_linked_to_weapon; named += x.weapon_stats.weapons_named; kill_named += x.weapon_stats.named_by_kill_record;
        hc += x.weapon_stats.holdout_type_id_correct; ha += x.weapon_stats.holdout_type_id_attempted; wl += x.weapon_stats.weapons_with_launcher;
        hits += x.hits.len(); dmg += x.damage.len();
        let s = &x.weapon_stats;
        rej += s.rejected_impossible_links; seek += s.targets_seeker; killt += s.targets_kill_record; geo += s.geometry_available; exact += s.launchers_exact;
        for (k, v) in &s.outcome_classes { *classes.entry(k).or_default() += v; }
        for (k, v) in &s.designated_point_eval { let e = points.entry(*k).or_default(); e.0 += v.0; e.1 += v.1; }
        for w in x.weapons.iter().filter(|w| w.launcher.is_author) { au += 1; au_named += w.weapon_name.is_some() as u32; au_target += (w.target.status != "unavailable") as u32; au_geo += w.geometry.is_some() as u32; }
        for k in &x.kills { *surface.entry(format!("victim_surface={} killer_surface={}", k.victim_surface, k.killer_surface)).or_default() += 1; }
    }
    println!("launcher {wl} linked {linked} named {named} kill-named {kill_named} holdout {hc}/{ha} hits {hits} damage {dmg}");
    println!("target eval (claims, correct): {ev:?}");
    println!("{surface:?}");
    println!("exact launchers {exact}; rejected impossible links {rej}; targets seeker {seek} kill {killt}; geometry {geo}");
    println!("classes {classes:?}");
    println!("designated point eval {points:?}");
    println!("author {au} named {au_named} target {au_target} geometry {au_geo}");
}
