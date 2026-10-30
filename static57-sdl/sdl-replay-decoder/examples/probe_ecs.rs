//! Research probe: ECS creation templates; link weapons (0xF0DB) to entities.
use sdl_replay_decoder::ecs;
use sdl_replay_decoder::manifest::{analyze, Depth, Options};
use sdl_replay_decoder::shells;
use std::collections::{BTreeMap, HashMap, HashSet};

fn strings(b: &[u8], min: usize) -> Vec<String> {
    let mut out = vec![];
    let mut cur = vec![];
    for &x in b.iter().chain([0u8].iter()) {
        if (0x20..0x7f).contains(&x) {
            cur.push(x)
        } else {
            if cur.len() >= min {
                out.push(String::from_utf8_lossy(&cur).into_owned())
            }
            cur.clear()
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files: Vec<_> = std::fs::read_dir(&args[1]).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "wrpl")).collect();
    files.sort();
    let (mut tot, mut ok, mut bad) = (0u64, 0u64, 0u64);
    let mut tmpl: BTreeMap<String, u64> = BTreeMap::new();
    let (mut w_total, mut w_linked) = (0u64, 0u64);
    let mut w_tmpl: BTreeMap<String, u64> = BTreeMap::new();
    let mut errs: BTreeMap<String, u64> = BTreeMap::new();
    let mut shown = 0;
    for f in files.iter() {
        let bytes = std::fs::read(f).unwrap();
        let mut t = ecs::Templates::default();
        let mut ents: HashMap<u32, (String, Vec<String>)> = HashMap::new();
        let mut seen: HashSet<u32> = HashSet::new();
        let mut pending: Vec<u32> = vec![];
        analyze("x", &bytes, Options { depth: Depth::Packets, ..Default::default() }, |rec, pl| {
            if rec.type_id == Some(6) {
                if let Some(r) = ecs::decode_creation(pl, &mut t) {
                    tot += 1;
                    match r {
                        Ok(v) => {
                            ok += 1;
                            for e in v {
                                *tmpl.entry(e.template.clone()).or_default() += 1;
                                ents.insert(e.eid, (e.template, strings(&e.data, 4)));
                            }
                        }
                        Err(e) => {
                            bad += 1;
                            *errs.entry(e.what.to_string()).or_default() += 1;
                        }
                    }
                }
            } else if rec.type_id == Some(4) {
                if let Ok(Some(m)) = shells::decode(pl) {
                    for r in m.records {
                        if seen.insert(r.eid) {
                            pending.push(r.eid);
                        }
                    }
                }
            }
            Ok(())
        })
        .unwrap();
        for e in pending {
            w_total += 1;
            let e2 = ((e & 0xFF) << 22) | (e >> 8);
            if let Some((tn, s)) = ents.get(&e2) {
                w_linked += 1;
                *w_tmpl.entry(tn.clone()).or_default() += 1;
                if shown < 16 && (shown < 4 || tn != "rocket") {
                    shown += 1;
                    println!("{tn}: {:?}", &s[..s.len().min(10)]);
                }
            }
        }
    }
    println!("create packets {tot} ok {ok} bad {bad} {errs:?}; weapons {w_total} linked to creation {w_linked}");
    println!("weapon templates {w_tmpl:?}");
    let mut v: Vec<_> = tmpl.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for (s, n) in v.iter().take(40) {
        println!("{n} {s}");
    }
}
