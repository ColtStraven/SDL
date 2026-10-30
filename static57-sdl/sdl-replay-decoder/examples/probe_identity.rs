//! Research probe: validates the unit registry against the flight sync and
//! the battle log, and prints kills with weapon names.
use sdl_replay_decoder::aircraft::{decode_sync, EntryBody};
use sdl_replay_decoder::combat::{self, CombatRecord};
use sdl_replay_decoder::ecs;
use sdl_replay_decoder::manifest::{analyze, Depth, Options};
use sdl_replay_decoder::units::Registry;
use std::collections::{BTreeMap, HashMap};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files: Vec<_> = std::fs::read_dir(&args[1]).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "wrpl")).collect();
    files.sort();
    let (mut units, mut with_name, mut spawn_ok, mut spawn_tested) = (0u64, 0u64, 0u64, 0u64);
    let (mut kill_ok, mut kill_bad, mut kill_none, mut dmg_ok, mut dmg_bad, mut dmg_none) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut kills, mut parsed_fail) = (0u64, 0u64);
    let mut weapons: BTreeMap<String, u64> = BTreeMap::new();
    let mut vehicles: BTreeMap<String, u64> = BTreeMap::new();
    let mut bad_examples = vec![];
    for (fi, file) in files.iter().enumerate() {
        let bytes = std::fs::read(file).unwrap();
        let mut t = ecs::Templates::default();
        let mut reg = Registry::default();
        let mut pending: HashMap<u16, (u32, [f32; 3])> = HashMap::new();
        let mut author: BTreeMap<String, u64> = BTreeMap::new();
        analyze("x", &bytes, Options { depth: Depth::Packets, ..Default::default() }, |rec, pl| {
            let ts = rec.timestamp_ms.unwrap_or(0);
            match rec.type_id {
                Some(2) => {
                    for e in decode_sync(pl).entries {
                        if let EntryBody::State(s) = e.body {
                            if let Some((ct, p)) = pending.remove(&e.uid) {
                                if ts.saturating_sub(ct) < 30_000 {
                                    spawn_tested += 1;
                                    let d = (((p[0] - s.position[0]).powi(2) + (p[1] - s.position[1]).powi(2) + (p[2] - s.position[2]).powi(2)) as f64).sqrt();
                                    if d < 1000.0 {
                                        spawn_ok += 1;
                                    }
                                }
                            }
                        }
                    }
                }
                Some(6) => {
                    if let Some(Ok(v)) = ecs::decode_creation(pl, &mut t) {
                        for e in v {
                            let before = reg.units;
                            reg.push(ts, &e);
                            if reg.units > before {
                                units += 1;
                                let u = reg.iter().filter(|u| u.created_ms == ts).last().unwrap().clone();
                                pending.insert(u.uid, (ts, u.spawn_position_m));
                                if let Some(n) = &u.vehicle {
                                    with_name += 1;
                                    *vehicles.entry(n.clone()).or_default() += 1;
                                }
                                if u.controlled_by_author {
                                    *author.entry(format!("{}@{}", u.vehicle.clone().unwrap_or_default(), u.uid)).or_default() += 1;
                                }
                            }
                        }
                    }
                }
                Some(4) => match combat::decode(ts, rec.index, pl) {
                    Some(CombatRecord::Kill(k)) => {
                        kills += 1;
                        if let Some(w) = &k.weapon {
                            *weapons.entry(w.clone()).or_default() += 1;
                        }
                        match (k.killer_uid.and_then(|u| reg.resolve(u, ts)), &k.killer_vehicle) {
                            (Some(u), Some(v)) if u.vehicle.as_deref() == Some(v.as_str()) => kill_ok += 1,
                            (Some(u), Some(v)) => {
                                kill_bad += 1;
                                if bad_examples.len() < 8 {
                                    bad_examples.push(format!("kill f{fi} uid {:?} log={v} reg={:?}", k.killer_uid, u.vehicle));
                                }
                            }
                            _ => kill_none += 1,
                        }
                    }
                    Some(CombatRecord::Damage(d)) => match (d.attacker_uid.and_then(|u| reg.resolve(u, ts)), &d.victim_vehicle) {
                        (Some(u), Some(v)) if u.vehicle.as_deref() == Some(v.as_str()) => dmg_ok += 1,
                        (Some(u), Some(v)) => {
                            dmg_bad += 1;
                            if bad_examples.len() < 8 {
                                bad_examples.push(format!("dmg f{fi} uid {:?} log={v} reg={:?}", d.victim_uid, u.vehicle));
                            }
                        }
                        _ => dmg_none += 1,
                    },
                    Some(CombatRecord::Hit(_)) => {}
                    None => {
                        if pl.len() >= 4 && matches!(u16::from_le_bytes([pl[2], pl[3]]), 0xF058 | 0xF056 | 0xF157) {
                            parsed_fail += 1;
                        }
                    }
                },
                _ => {}
            }
            Ok(())
        })
        .unwrap();
        println!("f{fi} author {author:?}");
    }
    println!("units {units} named {with_name}; spawn check {spawn_ok}/{spawn_tested} within 1 km");
    println!("kills {kills} parse_fail {parsed_fail}; killer vehicle match {kill_ok} mismatch {kill_bad} unresolved {kill_none}; damage match {dmg_ok} mismatch {dmg_bad} unresolved {dmg_none}");
    println!("examples {bad_examples:?}");
    let mut v: Vec<_> = weapons.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    println!("weapons {:?}", &v[..v.len().min(50)]);
    let mut v: Vec<_> = vehicles.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    println!("vehicles {:?}", &v[..v.len().min(30)]);
}
