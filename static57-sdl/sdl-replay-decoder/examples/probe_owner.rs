//! Research probe: launcher entity id in store creation records.
//! Checks it against unit entities of every template, and against kill records.
use sdl_replay_decoder::bitstream::BitReader;
use sdl_replay_decoder::ecs::{self, read_compact_eid, Templates};
use sdl_replay_decoder::lab::extract;
use sdl_replay_decoder::manifest::{analyze, Depth, Options};
use sdl_replay_decoder::tracks::TrackConfig;
use sdl_replay_decoder::units::prefixed_strings;
use sdl_replay_decoder::weapons::{entity_key, is_store_template};
use std::collections::{BTreeMap, HashMap};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files: Vec<_> = std::fs::read_dir(&args[1]).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "wrpl")).collect();
    files.sort();
    let mut owner_tmpl: BTreeMap<String, u64> = BTreeMap::new();
    let (mut stores, mut resolved, mut parse_fail) = (0u64, 0u64, 0u64);
    let (mut kill_ok, mut kill_bad) = (0u64, 0u64);
    let mut shown: BTreeMap<String, u32> = BTreeMap::new();
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let x = extract("x", &bytes, Options::default(), TrackConfig::default(), |_| {}).unwrap();
        let mut t = Templates::default();
        let mut ents: HashMap<u32, (String, Vec<u8>)> = HashMap::new();
        let mut store_owner: HashMap<(u32, u32), Vec<(u32, u32)>> = HashMap::new();
        analyze("x", &bytes, Options { depth: Depth::Packets, ..Default::default() }, |rec, pl| {
            if rec.type_id == Some(6) {
                if let Some(Ok(v)) = ecs::decode_creation(pl, &mut t) {
                    for e in v {
                        if is_store_template(&e.template) {
                            stores += 1;
                            let mut r = BitReader::new(&e.data[3.min(e.data.len())..]);
                            let ok = r.compressed("t").and_then(|_| read_compact_eid(&mut r));
                            match ok {
                                Ok(o) => {
                                    store_owner.entry(entity_key(e.eid)).or_default().push((rec.timestamp_ms.unwrap_or(0), o));
                                    match ents.get(&o) {
                                        Some((tn, d)) => {
                                            resolved += 1;
                                            let k = tn.split('+').next().unwrap().to_string();
                                            *owner_tmpl.entry(tn.clone()).or_default() += 1;
                                            let n = shown.entry(k).or_default();
                                            if *n < 2 {
                                                *n += 1;
                                                println!("{tn} len {} strs {:?}", d.len(), &prefixed_strings(d, 3)[..prefixed_strings(d, 3).len().min(6)]);
                                            }
                                        }
                                        None => *owner_tmpl.entry("(no entity)".into()).or_default() += 1,
                                    }
                                }
                                Err(_) => parse_fail += 1,
                            }
                        } else {
                            ents.insert(e.eid, (e.template, e.data));
                        }
                    }
                }
            }
            Ok(())
        })
        .unwrap();
        // Kill records as independent truth for the launcher.
        for w in x.weapons.iter().filter(|w| w.outcome.status == "kill_linked") {
            let id = w.weapon_id.trim_start_matches('w');
            let mut p = id.split('r').next().unwrap().split('.');
            let key = (p.next().unwrap().parse().unwrap(), p.next().unwrap().parse().unwrap());
            let Some(list) = store_owner.get(&key) else { continue };
            let Some((_, o)) = list.iter().rfind(|(t, _)| *t <= w.trajectory.first_ms) else { continue };
            let k = x.kills.iter().find(|k| Some(k.packet_index) == w.outcome.kill_packet_index).unwrap();
            let Some(killer) = k.killer_uid else { continue };
            match x.registry.iter().find(|u| u.eid == *o) {
                Some(u) if u.uid == killer => kill_ok += 1,
                Some(_) => kill_bad += 1,
                None => {}
            }
        }
    }
    println!("stores {stores} owner entity found {resolved} parse_fail {parse_fail}");
    let mut v: Vec<_> = owner_tmpl.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for (k, n) in v.iter().take(24) { println!("{n} {k}"); }
    println!("kill-record check: launcher uid == killer {kill_ok}, differs {kill_bad}");
}
