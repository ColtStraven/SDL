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

pub struct FlightExport {
    pub manifest: Manifest,
    pub report: crate::tracks::FlightReport,
}

/// Identify the replay for provenance fields without running the pipeline.
pub fn replay_ref(file: &[u8]) -> crate::tracks::ReplayRef {
    let mut d = crate::diagnostics::Diagnostics::default();
    let (magic, version) = match crate::container::parse_header(file, &mut d) {
        Ok(p) => (
            format!("0x{:08X}", p.header.format_magic),
            crate::container::game_version_string(p.header.game_version_raw),
        ),
        Err(_) => ("unknown".into(), "unknown".into()),
    };
    crate::tracks::ReplayRef { replay_sha256: crate::evidence::sha256_hex(file), format_magic: magic, game_version: version }
}

/// Run packet extraction plus flight reconstruction. `sink` receives every
/// flight sample in stream order.
pub fn analyze_flight(
    file_name: &str,
    file: &[u8],
    opts: Options,
    cfg: crate::tracks::TrackConfig,
    mut sink: impl FnMut(&crate::tracks::FlightSample) -> io::Result<()>,
) -> io::Result<FlightExport> {
    use crate::tracks::Reconstructor;
    let mut rec = Reconstructor::new(replay_ref(file), cfg);
    let mut sink_err: Option<io::Error> = None;
    let opts = Options { depth: manifest::Depth::Packets, ..opts };
    let manifest = manifest::analyze(file_name, file, opts, |p: &PacketRecord, payload| {
        if p.record != crate::packets::RecordKind::Packet {
            return Ok(());
        }
        let t = p.timestamp_ms.unwrap_or(0);
        match p.type_id {
            Some(2) => {
                let d = crate::aircraft::decode_sync(payload);
                rec.push_sync(p.index, t, payload.len() as u64 * 8, &d, |s| {
                    if sink_err.is_none() {
                        if let Err(e) = sink(s) {
                            sink_err = Some(e);
                        }
                    }
                });
            }
            Some(4) => {
                if let Some(c) = crate::cockpit::decode(t, payload) {
                    rec.push_cockpit(c);
                }
            }
            _ => {}
        }
        Ok(())
    })?;
    if let Some(e) = sink_err {
        return Err(e);
    }
    Ok(FlightExport { manifest, report: rec.finish() })
}

/// Write `flight_tracks.jsonl` and `flight_summary.json` to `out_dir`.
pub fn export_flight(input: &Path, file: &[u8], out_dir: &Path, opts: Options) -> io::Result<(FlightExport, PathBuf, PathBuf)> {
    fs::create_dir_all(out_dir)?;
    let tracks_path = out_dir.join("flight_tracks.jsonl");
    let summary_path = out_dir.join("flight_summary.json");
    let input_canon = fs::canonicalize(input).ok();
    for p in [&tracks_path, &summary_path] {
        if p.exists() && fs::canonicalize(p).ok() == input_canon {
            return Err(io::Error::other("refusing to overwrite the input replay"));
        }
    }
    let mut w = BufWriter::with_capacity(1 << 20, File::create(&tracks_path)?);
    let name = input.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ex = analyze_flight(&name, file, opts, crate::tracks::TrackConfig::default(), |s| {
        serde_json::to_writer(&mut w, s)?;
        w.write_all(b"\n")
    })?;
    w.flush()?;
    write_json(&summary_path, &ex.report)?;
    Ok((ex, tracks_path, summary_path))
}
