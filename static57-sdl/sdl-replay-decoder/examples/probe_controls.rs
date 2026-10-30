//! Research probe: author aircraft records paired with the nearest cockpit
//! message. JSONL: one sample per line, for offline correlation.
use sdl_replay_decoder::aircraft::{decode_sync, EntryBody};
use sdl_replay_decoder::cockpit;
use sdl_replay_decoder::flight::{decode_orientation, decode_velocity};
use sdl_replay_decoder::lab::extract;
use sdl_replay_decoder::manifest::{analyze, Depth, Options};
use sdl_replay_decoder::tracks::TrackConfig;
use std::collections::BTreeMap;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files: Vec<_> = std::fs::read_dir(&args[1]).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "wrpl")).collect();
    files.sort();
    let mut w = std::io::BufWriter::new(std::fs::File::create(&args[2]).unwrap());
    for (fi, f) in files.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        let x = extract("x", &bytes, Options::default(), TrackConfig::default(), |_| {}).unwrap();
        let authors = x.flight.author_uids.clone();
        if authors.is_empty() {
            continue;
        }
        let mut lines: Vec<(u32, serde_json::Value)> = vec![];
        let mut cockpit: BTreeMap<u32, Vec<(u16, f32)>> = BTreeMap::new();
        analyze("x", &bytes, Options { depth: Depth::Packets, ..Default::default() }, |rec, pl| {
            let t = rec.timestamp_ms.unwrap_or(0);
            match rec.type_id {
                Some(2) => {
                    for e in decode_sync(pl).entries {
                        let EntryBody::State(s) = e.body else { continue };
                        if !authors.contains(&e.uid) {
                            continue;
                        }
                        let o = decode_orientation(s.euler_raw);
                        let v = decode_velocity(s.packed_velocity_raw);
                        let vehicle = x.registry.resolve(e.uid, t).and_then(|u| u.vehicle.clone());
                        let eng: Vec<_> = s.engines.iter().map(|g| serde_json::json!([g.state, g.boost_raw, g.health_raw, g.radiators[0], g.radiators[1]])).collect();
                        let wp: Vec<_> = s.weapons.iter().map(|g| serde_json::json!([g.a, g.values])).collect();
                        lines.push((t, serde_json::json!({"f":fi,"v":vehicle,"u":e.uid,"t":t,"c":s.controls,"e":eng,"k":s.unk_u32,"b":s.unk_bit,"g":s.ground_flag,"p":s.position,"h":o.heading_deg,"th":o.pitch_deg,"r":o.bank_deg,"sp":v.speed_mps,"vy":v.vy_mps,"w":wp,"tp":s.tail_pair})));
                    }
                }
                Some(4) => {
                    if let Some(c) = cockpit::decode(t, pl) {
                        cockpit.insert(t, c.params);
                    }
                }
                _ => {}
            }
            Ok(())
        })
        .unwrap();
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        for (t, mut v) in lines {
            let near = cockpit.range(t.saturating_sub(125)..=t + 125).min_by_key(|e| e.0.abs_diff(t));
            if let Some((_, p)) = near {
                let m: BTreeMap<String, f32> = p.iter().map(|(i, x)| (i.to_string(), *x)).collect();
                v["q"] = serde_json::json!(m);
            }
            writeln!(w, "{v}").unwrap();
        }
        println!("{fi} {name}");
    }
}
