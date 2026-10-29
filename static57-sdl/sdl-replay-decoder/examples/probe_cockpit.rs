//! Research probe: MPI 0xD04A cockpit parameters. CSV: file,t,pkt,obj,n,id:value...
use sdl_replay_decoder::bitstream::BitReader;
use sdl_replay_decoder::manifest::{analyze, Depth, Options};
use std::collections::BTreeMap;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let p = std::path::PathBuf::from(&args[1]);
    let mut files: Vec<_> = std::fs::read_dir(&p).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.extension().is_some_and(|x| x == "wrpl")).collect();
    files.sort();
    let mut w = std::io::BufWriter::new(std::fs::File::create(&args[2]).unwrap());
    for (fi, f) in files.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        let mut msgs: BTreeMap<u16, u64> = BTreeMap::new();
        let (mut n, mut bad) = (0, 0);
        analyze("x", &bytes, Options { depth: Depth::Packets, ..Default::default() }, |rec, pl| {
            if rec.type_id != Some(4) || pl.len() < 4 { return Ok(()); }
            let obj = u16::from_le_bytes([pl[0], pl[1]]);
            let mid = u16::from_le_bytes([pl[2], pl[3]]);
            *msgs.entry(mid).or_default() += 1;
            if mid != 0xD04A { return Ok(()); }
            n += 1;
            let mut r = BitReader::new(&pl[4..]);
            let ok = (|| -> Option<Vec<(u16, f32)>> {
                let bits = r.compressed("len").ok()?;
                r.align();
                if bits > r.remaining() { return None; }
                let count = r.u16_le("count").ok()?;
                if 16 + count as u64 * 48 > bits { return None; }
                let mut v = Vec::new();
                for _ in 0..count { v.push((r.u16_le("id").ok()?, r.f32_le("val").ok()?)); }
                Some(v)
            })();
            match ok {
                Some(v) => {
                    write!(w, "{fi},{},{},{:#06x},{}", rec.timestamp_ms.unwrap(), rec.index, obj, v.len()).unwrap();
                    for (id, x) in v { write!(w, ",{id}:{x}").unwrap(); }
                    writeln!(w).unwrap();
                }
                None => bad += 1,
            }
            Ok(())
        }).unwrap();
        let mut top: Vec<_> = msgs.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1));
        println!("{} d04a={n} bad={bad} top_msgs={:?}", f.file_name().unwrap().to_string_lossy(), top.iter().take(6).map(|(m, c)| format!("{m:#06x}:{c}")).collect::<Vec<_>>());
    }
}
