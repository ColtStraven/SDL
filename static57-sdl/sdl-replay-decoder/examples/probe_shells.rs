//! Research probe: 0xF0DB structural statistics and CSV of decoded records.
//! Usage: probe_shells <replay dir> <csv out>
use sdl_replay_decoder::manifest::{analyze, Depth, Options};
use sdl_replay_decoder::shells;
use std::collections::BTreeMap;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files: Vec<_> = std::fs::read_dir(&args[1]).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "wrpl")).collect();
    files.sort();
    let mut w = std::io::BufWriter::new(std::fs::File::create(&args[2]).unwrap());
    writeln!(w, "file,t,list,no_seeker,short,eid,gen,x,y,z,packed,index,dx,dy,dz,vx,vy,vz,rx,ry,rz,flagv,flags,timers,sfloat,sbits").unwrap();
    let (mut msgs, mut exact, mut inexact, mut failed, mut recs) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut errs: BTreeMap<String, u64> = BTreeMap::new();
    let mut lists: BTreeMap<u8, u64> = BTreeMap::new();
    for (fi, f) in files.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        analyze("x", &bytes, Options { depth: Depth::Packets, ..Default::default() }, |rec, pl| {
            if rec.type_id != Some(4) { return Ok(()); }
            match shells::decode(pl) {
                Ok(None) => {}
                Ok(Some(m)) => {
                    msgs += 1;
                    if m.exact { exact += 1 } else { inexact += 1 }
                    if !m.exact { return Ok(()); }
                    for r in &m.records {
                        recs += 1;
                        *lists.entry(r.list).or_default() += 1;
                        write!(w, "{fi},{},{},{},{},{},{},{},{},{},{}", rec.timestamp_ms.unwrap(), r.list, r.no_seeker as u8, r.short as u8, r.eid & 0x3F_FFFF, r.eid >> 22, r.position_m[0], r.position_m[1], r.position_m[2], r.position_packed as u8).unwrap();
                        match &r.dynamics {
                            Some(d) => {
                                let v = d.velocity_mps();
                                writeln!(w, ",{},{},{},{},{:.2},{:.2},{:.2},{},{},{},{:?},{},{:?},{:?},{:?}", d.index, d.direction[0], d.direction[1], d.direction[2], v[0], v[1], v[2], d.rates_raw[0], d.rates_raw[1], d.rates_raw[2], d.flag_value, d.flags, d.timers_s, d.seeker_float, d.seeker_bits).unwrap();
                            }
                            None => writeln!(w, ",,,,,,,,,,,,,,,").unwrap(),
                        }
                    }
                }
                Err(e) => {
                    msgs += 1;
                    failed += 1;
                    *errs.entry(e.what.to_string()).or_default() += 1;
                }
            }
            Ok(())
        }).unwrap();
    }
    println!("messages={msgs} exact={exact} inexact={inexact} failed={failed} records={recs} lists={lists:?}");
    println!("errors={errs:?}");
}
