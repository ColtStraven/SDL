//! Research probe: structural decode statistics for type-2 payloads.
//! Usage: cargo run --release --example probe_aircraft -- <replay or dir> [csv_out]
use sdl_replay_decoder::aircraft::{decode_sync, EntryBody};
use sdl_replay_decoder::manifest::{analyze, Depth, Options};
use std::collections::BTreeMap;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let p = std::path::PathBuf::from(&args[1]);
    let mut files: Vec<_> = if p.is_dir() {
        std::fs::read_dir(&p).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "wrpl")).collect()
    } else { vec![p] };
    files.sort();
    let mut csv = args.get(2).map(|f| std::io::BufWriter::new(std::fs::File::create(f).unwrap()));
    if let Some(w) = csv.as_mut() { writeln!(w, "file,pkt,t,uid,ground,x,y,z,euler,vel,unk_u32,unk_bit,trail_x,trail_y,trail_z,hidden_a,hidden_b").unwrap(); }
    for (fi, f) in files.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        let (mut n, mut ok, mut states, mut withheld, mut nodata) = (0u64, 0u64, 0u64, 0u64, 0u64);
        let mut errs: BTreeMap<String, u64> = BTreeMap::new();
        let mut leftover: BTreeMap<u64, u64> = BTreeMap::new();
        let mut uids: BTreeMap<u16, u64> = BTreeMap::new();
        let mut trailer_match = 0u64;
        let opts = Options { depth: Depth::Packets, ..Default::default() };
        analyze("x", &bytes, opts, |rec, payload| {
            if rec.type_id != Some(2) { return Ok(()); }
            n += 1;
            let d = decode_sync(payload);
            if d.terminated && d.error.is_none() { ok += 1; } else {
                *errs.entry(d.error.as_ref().map(|e| e.what.to_string()).unwrap_or("unterminated".into())).or_default() += 1;
            }
            *leftover.entry((d.payload_bits - d.consumed_bits) / 8).or_default() += 1;
            for e in &d.entries {
                match &e.body {
                    EntryBody::NoData => nodata += 1,
                    EntryBody::Withheld => withheld += 1,
                    EntryBody::State(s) => {
                        states += 1;
                        *uids.entry(e.uid).or_default() += 1;
                        if let Some(t) = d.trailer { if t == s.position { trailer_match += 1; } }
                        if let Some(w) = csv.as_mut() {
                            let t = d.trailer.unwrap_or([f32::NAN; 3]);
                            let (a, b) = e.hidden_flags.unwrap();
                            writeln!(w, "{fi},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}", rec.index, rec.timestamp_ms.unwrap(), e.uid, s.ground_flag as u8,
                                s.position[0], s.position[1], s.position[2], s.euler_raw, s.packed_velocity_raw, s.unk_u32, s.unk_bit as u8, t[0], t[1], t[2], a as u8, b as u8).unwrap();
                        }
                    }
                }
            }
            Ok(())
        }).unwrap();
        let top_left: Vec<_> = { let mut v: Vec<_> = leftover.iter().collect(); v.sort_by(|a, b| b.1.cmp(a.1)); v.into_iter().take(4).collect() };
        println!("{} pkts={n} ok={ok} states={states} withheld={withheld} nodata={nodata} uids={} trailer_eq={trailer_match} leftover_bytes(top)={:?} errs={:?}",
            f.file_name().unwrap().to_string_lossy(), uids.len(), top_left, errs);
    }
}
