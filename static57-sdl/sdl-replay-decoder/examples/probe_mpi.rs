//! Research probe: MPI message histogram over all replays, and payload dumps.
//! Usage: probe_mpi <replay dir> [hex message id] [max dumps per replay]
use sdl_replay_decoder::evidence::hex;
use sdl_replay_decoder::manifest::{analyze, Depth, Options};
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dump = args.get(2).map(|s| u16::from_str_radix(s.trim_start_matches("0x"), 16).unwrap());
    let max: usize = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(5);
    let mut files: Vec<_> = std::fs::read_dir(&args[1]).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "wrpl")).collect();
    files.sort();
    let mut hist: BTreeMap<u16, (u64, u64, usize, usize)> = BTreeMap::new();
    for (fi, f) in files.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        let mut shown = 0;
        analyze("x", &bytes, Options { depth: Depth::Packets, ..Default::default() }, |rec, pl| {
            if rec.type_id != Some(4) || pl.len() < 4 { return Ok(()); }
            let obj = u16::from_le_bytes([pl[0], pl[1]]);
            let mid = u16::from_le_bytes([pl[2], pl[3]]);
            let e = hist.entry(mid).or_insert((0, 0, usize::MAX, 0));
            e.0 += 1;
            e.1 |= 1 << fi;
            e.2 = e.2.min(pl.len());
            e.3 = e.3.max(pl.len());
            if Some(mid) == dump && shown < max {
                shown += 1;
                println!("f{fi} t={} pkt={} obj={:#06x} len={} {}", rec.timestamp_ms.unwrap(), rec.index, obj, pl.len(), hex(&pl[4..pl.len().min(240)]));
            }
            Ok(())
        }).unwrap();
    }
    if dump.is_none() {
        let mut v: Vec<_> = hist.into_iter().collect();
        v.sort_by(|a, b| b.1.0.cmp(&a.1.0));
        for (m, (n, fs, lo, hi)) in v.iter().filter(|(m, _)| m & 0xF000 >= 0xD000 && m & 0x0F00 <= 0x0100 && *m >= 0xF100).take(200) {
            println!("{m:#06x} n={n} replays={} len={lo}..{hi}", fs.count_ones());
        }
    }
}
