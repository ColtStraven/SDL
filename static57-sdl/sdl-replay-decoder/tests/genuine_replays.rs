//! Compatibility tests against genuine War Thunder replays.
//!
//! Ignored by default because replays are not stored in the repository.
//! Run with:
//!
//! ```text
//! SDL_REPLAY_DIR=<folder with .wrpl files> cargo test --release -- --ignored
//! ```
//!
//! Without `SDL_REPLAY_DIR`, the local War Thunder replay folder
//! (`%LOCALAPPDATA%/WarThunder/Replays`) is used. Files are only read.

use sdl_replay_decoder::manifest::{analyze, Depth, Options};
use std::path::PathBuf;

fn replay_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("SDL_REPLAY_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("WarThunder").join("Replays")))?;
    dir.is_dir().then_some(dir)
}

#[test]
#[ignore = "needs genuine .wrpl files (SDL_REPLAY_DIR)"]
fn genuine_replays_frame_completely() {
    let dir = replay_dir().expect("no replay directory: set SDL_REPLAY_DIR");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wrpl")))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no .wrpl files in {}", dir.display());

    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let opts = Options { depth: Depth::Packets, ..Default::default() };
        let m = analyze(&name, &bytes, opts, |_, _| Ok(())).unwrap();
        assert_eq!(m.status, "ok", "{name}: {:?}", m.diagnostics);
        assert!(m.format.as_ref().unwrap().recognized, "{name}: unregistered format");
        let st = m.stream.as_ref().unwrap();
        assert_eq!(st.codec_matches_profile, Some(true), "{name}");
        let inf = st.inflation.as_ref().unwrap();
        assert_eq!(inf.trailing_bytes, 0, "{name}");
        assert_eq!(inf.compressed_consumed, inf.compressed_length, "{name}");
        let p = m.packets.as_ref().unwrap();
        assert!(p.all_bytes_accounted && !p.truncated, "{name}");
        assert_eq!(p.malformed_records, 0, "{name}");
        assert!(p.clock.monotonic, "{name}");
        let end = p.end_marker_index.expect("end marker");
        assert_eq!(end + 1, p.records, "{name}: end marker is not the last record");
        eprintln!("{name}: {} packets, {} unknown-type, sha256 {}", p.records, p.unknown_type_packets, m.input.sha256);
    }
    eprintln!("{} genuine replays framed completely from {}", files.len(), dir.display());
}
