mod common;

use common::*;
use sdl_replay_decoder::export::{export_packets, sanitized_stem};
use sdl_replay_decoder::manifest::{Depth, Options};
use std::path::Path;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sdl-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn exports_manifest_and_jsonl() {
    let dir = temp_dir("export");
    let input = dir.join("fixture.wrpl");
    let file = replay(MAGIC_18C1C, &zstd(&sample_stream()), true);
    std::fs::write(&input, &file).unwrap();
    let out = dir.join("out");
    let opts = Options { depth: Depth::Packets, include_payload_hex: true, ..Default::default() };
    let ex = export_packets(&input, &file, &out, opts).unwrap();

    // The replay is untouched.
    assert_eq!(std::fs::read(&input).unwrap(), file);

    let m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&ex.manifest_path).unwrap()).unwrap();
    assert_eq!(m["decoder"]["schema"], "static57.sdl.replay-manifest/0.1");
    assert_eq!(m["status"], "ok");
    assert_eq!(m["input"]["file_name"], "fixture.wrpl");
    assert_eq!(m["input"]["size"], file.len());
    assert_eq!(m["format"]["format_magic_hex"], "0x00018C1C");
    assert_eq!(m["session"]["session_id_hex"], "0x4AD212E800E51D58");
    assert_eq!(m["session"]["session_id"]["value"], SESSION_ID);
    assert_eq!(m["session"]["session_id"]["evidence"], "REPORTED");
    assert_eq!(m["session"]["game_version"]["evidence"], "EXPERIMENTAL");
    assert_eq!(m["stream"]["codec_detected"], "zstd");
    assert_eq!(m["packets"]["records"], 7);
    assert_eq!(m["packets"]["types"]["4"]["kind"], "mpi");
    assert_eq!(m["header_raw_hex"].as_str().unwrap().len(), 1234 * 2);
    assert_eq!(m["header_fields"].as_array().unwrap().len(), 41);

    let lines: Vec<serde_json::Value> = std::fs::read_to_string(&ex.packets_path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 7);
    assert_eq!(lines[0]["kind"], "replay_header_info");
    assert_eq!(lines[0]["timestamp_ms"], 1000);
    assert_eq!(lines[0]["timestamp_source"], "explicit");
    assert_eq!(lines[1]["timestamp_source"], "inherited");
    assert_eq!(lines[5]["payload_hex"], "68656c6c6f");
    assert_eq!(lines[6]["kind"], "end_marker");
    for (i, l) in lines.iter().enumerate() {
        assert_eq!(l["index"], i);
        assert_eq!(l["decode_state"], "raw");
        assert_eq!(l["payload_sha256"].as_str().unwrap().len(), 64);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stems_are_sanitized() {
    assert_eq!(sanitized_stem(Path::new("C:/x/#2026.10.23 15.47.41.wrpl")), "2026.10.23_15.47.41");
    assert_eq!(sanitized_stem(Path::new("../../etc.wrpl")), "etc");
    assert_eq!(sanitized_stem(Path::new("###.wrpl")), "replay");
}
