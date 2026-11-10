//! JSON and JSON Lines writers.

use crate::manifest::{self, Manifest, Options};
use crate::packets::PacketRecord;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(&mut w, value)?;
    w.write_all(b"\n")?;
    w.flush()
}

/// Output folder name derived from the replay file name, restricted to safe
/// characters.
pub fn sanitized_stem(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let s: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();
    let s = s.trim_matches(|c| c == '_' || c == '.').to_string();
    if s.is_empty() { "replay".into() } else { s }
}

fn write_jsonl<T: serde::Serialize>(path: &Path, items: &[T]) -> io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    for it in items {
        serde_json::to_writer(&mut w, it)?;
        w.write_all(b"\n")?;
    }
    w.flush()
}

pub struct FlightExport {
    pub decode: crate::flight::FlightDecode,
    pub files: Vec<PathBuf>,
}

/// Decode ownship flight data and weapon events and write `manifest.json`,
/// `flight_summary.json`, `ownship.jsonl`, `cockpit.jsonl` and
/// `weapons.jsonl` to `out_dir`.
pub fn export_flight(input: &Path, file: &[u8], out_dir: &Path) -> io::Result<FlightExport> {
    fs::create_dir_all(out_dir)?;
    let input_canon = fs::canonicalize(input).ok();
    let names = ["manifest.json", "flight_summary.json", "ownship.jsonl", "cockpit.jsonl", "weapons.jsonl"];
    let files: Vec<PathBuf> = names.iter().map(|n| out_dir.join(n)).collect();
    for p in &files {
        if p.exists() && fs::canonicalize(p).ok() == input_canon {
            return Err(io::Error::other("refusing to overwrite the input replay"));
        }
    }
    let name = input.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let d = crate::flight::decode(&name, file)?;
    write_json(&files[0], &d.manifest)?;
    write_json(&files[1], &d.summary)?;
    write_jsonl(&files[2], &d.ownship)?;
    write_jsonl(&files[3], &d.cockpit)?;
    write_jsonl(&files[4], &d.weapons)?;
    Ok(FlightExport { decode: d, files })
}

pub struct PacketExport {
    pub manifest: Manifest,
    pub manifest_path: PathBuf,
    pub packets_path: PathBuf,
}

/// Run packet extraction and write `manifest.json` and `packets.jsonl` to
/// `out_dir`. The replay itself is only read.
pub fn export_packets(input: &Path, file: &[u8], out_dir: &Path, opts: Options) -> io::Result<PacketExport> {
    fs::create_dir_all(out_dir)?;
    let manifest_path = out_dir.join("manifest.json");
    let packets_path = out_dir.join("packets.jsonl");
    let input_canon = fs::canonicalize(input).ok();
    for p in [&manifest_path, &packets_path] {
        if p.exists() && fs::canonicalize(p).ok() == input_canon {
            return Err(io::Error::other("refusing to overwrite the input replay"));
        }
    }
    let mut w = BufWriter::with_capacity(1 << 20, File::create(&packets_path)?);
    let name = input.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let manifest = manifest::analyze(&name, file, opts, |rec: &PacketRecord, _payload| {
        serde_json::to_writer(&mut w, rec)?;
        w.write_all(b"\n")
    })?;
    w.flush()?;
    write_json(&manifest_path, &manifest)?;
    Ok(PacketExport { manifest, manifest_path, packets_path })
}
