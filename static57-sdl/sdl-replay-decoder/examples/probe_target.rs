//! Research probe: seeker block sizes, target estimates vs aircraft and
//! kill victims; aircraft target and sensor records.
use sdl_replay_decoder::aircraft::{decode_sync, EntryBody};
use sdl_replay_decoder::lab::extract;
use sdl_replay_decoder::manifest::{analyze, Depth, Options};
use sdl_replay_decoder::seeker;
use sdl_replay_decoder::shells;
use sdl_replay_decoder::tracks::TrackConfig;
use sdl_replay_decoder::weapons::shell_key;
use std::collections::{BTreeMap, HashMap};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files: Vec<_> = std::fs::read_dir(&args[1]).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "wrpl")).collect();
    files.sort();
    let mut sizes: BTreeMap<u16, u64> = BTreeMap::new();
    let mut modes: BTreeMap<(u16, u8), u64> = BTreeMap::new();
    let (mut est_total, mut est_near_any, mut est_near_victim, mut est_victim_tested) = (0u64, 0u64, 0u64, 0u64);
    let mut d_any: Vec<f64> = vec![];
    let mut d_victim: Vec<f64> = vec![];
    let mut rng: Vec<f64> = vec![];
    let (mut los_ok, mut los_tested) = (0u64, 0u64);
    let mut los_err: Vec<f64> = vec![];
    let mut target_v1: BTreeMap<u8, u64> = BTreeMap::new();
    let mut contact_hi: BTreeMap<u32, u64> = BTreeMap::new();
    let mut tail_hi: BTreeMap<u32, u64> = BTreeMap::new();
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let x = extract("x", &bytes, Options::default(), TrackConfig::default(), |_| {}).unwrap();
        let mut victim: HashMap<(u32, u32), u16> = HashMap::new();
        for w in &x.weapons {
            if let Some(v) = w.outcome.victim_uid {
                let id = w.weapon_id.trim_start_matches('w');
                let mut p = id.split('r').next().unwrap().split('.');
                victim.insert((p.next().unwrap().parse().unwrap(), p.next().unwrap().parse().unwrap()), v);
            }
        }
        analyze("x", &bytes, Options { depth: Depth::Packets, ..Default::default() }, |rec, pl| {
            let t = rec.timestamp_ms.unwrap_or(0);
            if rec.type_id == Some(4) {
                if let Ok(Some(m)) = shells::decode(pl) {
                    for r in &m.records {
                        let Some(d) = &r.dynamics else { continue };
                        let Some(n) = d.seeker_bits else { continue };
                        *sizes.entry(n).or_default() += 1;
                        let o = seeker::parse(&d.seeker_block, n);
                        *modes.entry((n, o.mode)).or_default() += 1;
                        let p64 = r.position_m.map(|v| v as f64);
                        if let Some(e) = o.target_estimate_m {
                            let q = e.map(|v| v as f64);
                            est_total += 1;
                            if let Some((_, dd)) = x.history.nearest(t, q) {
                                d_any.push(dd);
                                est_near_any += (dd < 300.0) as u64;
                            }
                            if let Some(v) = victim.get(&shell_key(r.eid)).and_then(|u| x.history.at(*u, t)) {
                                est_victim_tested += 1;
                                let dd = ((q[0] - v[0]).powi(2) + (q[1] - v[1]).powi(2) + (q[2] - v[2]).powi(2)).sqrt();
                                d_victim.push(dd);
                                est_near_victim += (dd < 300.0) as u64;
                                if let Some(rr) = o.range_m {
                                    let true_r = ((p64[0] - v[0]).powi(2) + (p64[1] - v[1]).powi(2) + (p64[2] - v[2]).powi(2)).sqrt();
                                    rng.push((rr as f64 - true_r).abs());
                                }
                            }
                        }
                        if let (Some(l), Some(v)) = (o.line_of_sight, victim.get(&shell_key(r.eid)).and_then(|u| x.history.at(*u, t))) {
                            let dv = [v[0] - p64[0], v[1] - p64[1], v[2] - p64[2]];
                            let n = (dv[0] * dv[0] + dv[1] * dv[1] + dv[2] * dv[2]).sqrt();
                            if n > 50.0 {
                                los_tested += 1;
                                let cos = (l[0] as f64 * dv[0] + l[1] as f64 * dv[1] + l[2] as f64 * dv[2]) / n;
                                let a = cos.clamp(-1.0, 1.0).acos().to_degrees();
                                los_err.push(a);
                                los_ok += (a < 5.0) as u64;
                            }
                        }
                    }
                }
            } else if rec.type_id == Some(2) {
                for e in decode_sync(pl).entries {
                    if let EntryBody::State(s) = e.body {
                        for tg in &s.targets {
                            *target_v1.entry(tg.kind).or_default() += 1;
                            if let Some(v) = tg.tail {
                                *tail_hi.entry(v >> 16).or_default() += 1;
                            }
                        }
                        let _ = &s.contacts;
                        for c in &s.contacts {
                            *contact_hi.entry(c >> 16).or_default() += 1;
                        }
                    }
                }
            }
            Ok(())
        })
        .unwrap();
    }
    let top = |m: &BTreeMap<u32, u64>| { let mut v: Vec<_> = m.iter().map(|(k, n)| (format!("{k:#x}"), *n)).collect(); v.sort_by(|a, b| b.1.cmp(&a.1)); v.truncate(6); v };
    let mut sz: Vec<_> = sizes.into_iter().collect();
    sz.sort_by(|a, b| b.1.cmp(&a.1));
    println!("block sizes {:?}", &sz[..sz.len().min(14)]);
    let mut md: Vec<_> = modes.into_iter().collect();
    md.sort_by(|a, b| b.1.cmp(&a.1));
    println!("modes {:?}", &md[..md.len().min(14)]);
    let q = |v: &mut Vec<f64>, p: f64| { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); if v.is_empty() { f64::NAN } else { v[((v.len() - 1) as f64 * p) as usize] } };
    println!("estimates {est_total}; nearest aircraft p50 {:.0} p90 {:.0}; <300m {est_near_any}", q(&mut d_any, 0.5), q(&mut d_any, 0.9));
    println!("vs victim tested {est_victim_tested}; p50 {:.0} p90 {:.0}; <300m {est_near_victim}", q(&mut d_victim, 0.5), q(&mut d_victim, 0.9));
    println!("range field vs true range abs err p50 {:.0} p90 {:.0} n {}", q(&mut rng, 0.5), q(&mut rng, 0.9), rng.len());
    println!("LOS vs victim: <5deg {los_ok} of {los_tested}; p50 {:.1} p90 {:.1}", q(&mut los_err, 0.5), q(&mut los_err, 0.9));
    println!("target kinds {target_v1:?}");
    println!("target tail high16 {:?}", top(&tail_hi));
    println!("contacts high16 {:?}", top(&contact_hi));
}
