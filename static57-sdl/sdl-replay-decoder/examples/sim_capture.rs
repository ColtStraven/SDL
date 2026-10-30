//! Write a synthetic 8111 capture from a genuine replay, with a known clock
//! relation, for testing the validator. Output is marked synthetic and is
//! never evidence about the replay.
//! Usage: sim_capture <replay.wrpl> <out dir> [offset_s] [drift_ppm] [wind_mps]
use sdl_replay_decoder::lab::extract;
use sdl_replay_decoder::manifest::Options;
use sdl_replay_decoder::simcapture::{write_capture, SimConfig};
use sdl_replay_decoder::tracks::TrackConfig;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1]).unwrap();
    let x = extract("x", &bytes, Options::default(), TrackConfig::default(), |_| {}).unwrap();
    let num = |i: usize, d: f64| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let cfg = SimConfig { offset_s: num(3, 1234.5), drift_ppm: num(4, 0.0), wind_mps: num(5, 0.0), ..Default::default() };
    let n = write_capture(&x.author, &cfg, std::path::Path::new(&a[2])).unwrap();
    println!("{} author samples, {n} records", x.author.len());
}
