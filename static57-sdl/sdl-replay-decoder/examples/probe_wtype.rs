//! Research probe: creation records of weapons, with kill-linked names.
//! CSV: file,weapon_id,template,name,launcher_vehicle,len,hex
use sdl_replay_decoder::ecs::{self, Templates};
use sdl_replay_decoder::evidence::hex;
use sdl_replay_decoder::lab::extract;
use sdl_replay_decoder::manifest::{analyze, Depth, Options};
use sdl_replay_decoder::tracks::TrackConfig;
use sdl_replay_decoder::weapons::{entity_key, is_store_template};
use std::collections::HashMap;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files: Vec<_> = std::fs::read_dir(&args[1]).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "wrpl")).collect();
    files.sort();
    let mut w = std::io::BufWriter::new(std::fs::File::create(&args[2]).unwrap());
    for (fi, f) in files.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        let x = extract("x", &bytes, Options::default(), TrackConfig::default(), |_| {}).unwrap();
        // Creation records in order, per entity key.
        let mut t = Templates::default();
        let mut made: HashMap<(u32, u32), Vec<(u32, Vec<u8>)>> = HashMap::new();
        analyze("x", &bytes, Options { depth: Depth::Packets, ..Default::default() }, |rec, pl| {
            if rec.type_id == Some(6) {
                if let Some(Ok(v)) = ecs::decode_creation(pl, &mut t) {
                    for e in v {
                        if is_store_template(&e.template) {
                            made.entry(entity_key(e.eid)).or_default().push((rec.timestamp_ms.unwrap_or(0), e.data));
                        }
                    }
                }
            }
            Ok(())
        })
        .unwrap();
        for e in &x.weapons {
            let id = e.weapon_id.trim_start_matches('w');
            let base = id.split('r').next().unwrap();
            let mut p = base.split('.');
            let key = (p.next().unwrap().parse().unwrap(), p.next().unwrap().parse().unwrap());
            let Some(list) = made.get(&key) else { continue };
            let first = e.trajectory.first_ms;
            let Some((_, d)) = list.iter().rfind(|(t, _)| *t <= first) else { continue };
            writeln!(w, "{fi},{},{},{},{},{},{}", e.weapon_id, e.template, e.weapon_name.as_deref().unwrap_or(""), e.launcher.vehicle.as_deref().unwrap_or(""), d.len(), hex(d)).unwrap();
        }
    }
}
