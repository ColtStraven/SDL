use clap::{Parser, Subcommand};
use sdl_replay_decoder::diagnostics::Severity;
use sdl_replay_decoder::export;
use sdl_replay_decoder::manifest::{self, Depth, Manifest, Options};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Static 57 SDL native War Thunder replay decoder (S57-003).
#[derive(Parser)]
#[command(name = "sdl-replay-decoder", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect header, sections and compressed stream; emit a JSON manifest.
    Inspect {
        replay: PathBuf,
        /// Write manifest.json into this directory.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Print the full manifest JSON to stdout instead of the summary.
        #[arg(long)]
        json: bool,
        /// Decompressed-output ceiling in MiB (decompression-bomb guard).
        #[arg(long, default_value_t = 4096)]
        max_output_mib: u64,
    },
    /// Decompress and frame every packet; write manifest.json and packets.jsonl.
    Packets {
        replay: PathBuf,
        /// Output directory [default: ./sdl-out/<replay name>].
        #[arg(long)]
        out: Option<PathBuf>,
        /// Include each packet payload as hex in packets.jsonl.
        #[arg(long)]
        payloads: bool,
        #[arg(long, default_value_t = 4096)]
        max_output_mib: u64,
    },
    /// Inspect and frame many replays; print a compatibility table.
    Survey {
        /// Replay files or directories containing .wrpl files.
        paths: Vec<PathBuf>,
        /// Write the survey as JSON to this file.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn exit_for(m: &Manifest) -> ExitCode {
    match m.status {
        "ok" => ExitCode::SUCCESS,
        "partial" => ExitCode::from(1),
        _ => ExitCode::from(2),
    }
}

fn print_summary(m: &Manifest) {
    println!("SDL replay inspection — {} v{}", m.decoder.name, m.decoder.version);
    println!("  file          {} ({} bytes)", m.input.file_name, m.input.size);
    println!("  sha256        {}", m.input.sha256);
    println!("  status        {}", m.status);
    if let Some(f) = &m.format {
        let profile = f.profile.as_ref().map(|p| format!("{} [{:?}]", p.name, p.evidence)).unwrap_or_else(|| "UNREGISTERED".into());
        println!("  format        magic {}  profile {}", f.format_magic_hex, profile);
    }
    if let Some(s) = &m.session {
        println!("  game version  {} [{:?}] (raw {})", s.game_version.value, s.game_version.evidence, s.game_version_raw_hex);
        println!("  session       {} part {}  recording {} [{:?}]", s.session_id_hex, s.replay_part_number.value, s.recording.kind, s.recording.evidence);
        println!("  start         {} (unix {})", s.start_time_utc.value, s.start_time_unix.value);
        println!("  mission       {}  level {}", s.mission_name.value, s.level_path.value);
        println!("  conditions    {} / {}", s.environment.value, s.weather.value);
    }
    if let Some(l) = &m.layout {
        let sec = |s: &Option<sdl_replay_decoder::container::Section>| {
            s.as_ref().map(|s| format!("@{} len {}", s.offset, s.length)).unwrap_or_else(|| "absent".into())
        };
        println!("  settings blk  {}", sec(&l.settings_blk));
        println!("  packet stream {}", sec(&l.packet_stream));
        println!("  footer blk    {}", sec(&l.footer_blk));
    }
    if let Some(st) = &m.stream {
        print!("  codec         {:?} (profile expects {:?})", st.codec_detected, st.codec_expected);
        if let Some(r) = &st.inflation {
            print!(
                " -> {} frame(s), {} of {} compressed bytes consumed, {} bytes decompressed",
                r.frames.len(),
                r.compressed_consumed,
                r.compressed_length,
                r.decompressed_length
            );
        }
        println!();
    }
    if let Some(p) = &m.packets {
        println!(
            "  packets       {} records: {} recognized, {} unknown-type, {} malformed, {} zero-size{}",
            p.records,
            p.recognized_packets,
            p.unknown_type_packets,
            p.malformed_records,
            p.zero_size_records,
            if p.truncated { ", TRUNCATED" } else { "" }
        );
        println!("  accounting    {} / {} decompressed bytes framed; all accounted: {}", p.bytes_framed, p.stream_bytes, p.all_bytes_accounted);
        let c = &p.clock;
        println!(
            "  clock         {:?}..{:?} ms (span {:?} ms), explicit {}, inherited {}, monotonic {}, largest gap {} ms",
            c.first_timestamp_ms, c.last_timestamp_ms, c.span_ms, c.explicit_count, c.inherited_count, c.monotonic, c.largest_forward_gap_ms
        );
        println!("  type  kind                 count      payload bytes   min..max");
        for t in p.types.values() {
            println!(
                "  {:>4}  {:<20} {:>9}  {:>14}   {}..{}",
                t.type_id,
                t.kind.unwrap_or("UNKNOWN"),
                t.count,
                t.payload_bytes,
                t.min_payload,
                t.max_payload
            );
        }
    }
    for d in &m.diagnostics.0 {
        let tag = match d.severity {
            Severity::Info => "info",
            Severity::Warning => "WARN",
            Severity::Error => "ERROR",
        };
        let at = d.offset.map(|o| format!(" @{o}")).unwrap_or_default();
        println!("  [{tag}] {} ({}{at}): {}", d.code, d.section, d.message);
    }
}

fn collect(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(p) {
                let mut v: Vec<_> = rd
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|f| f.extension().is_some_and(|x| x.eq_ignore_ascii_case("wrpl")))
                    .collect();
                v.sort();
                out.extend(v);
            }
        } else {
            out.push(p.clone());
        }
    }
    out
}

fn run() -> Result<ExitCode, String> {
    match Cli::parse().command {
        Command::Inspect { replay, out, json, max_output_mib } => {
            let file = read(&replay)?;
            let name = replay.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let opts = Options { depth: Depth::Inspect, max_output: max_output_mib << 20, ..Default::default() };
            let m = manifest::analyze(&name, &file, opts, |_, _| Ok(())).map_err(|e| e.to_string())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&m).unwrap());
            } else {
                print_summary(&m);
            }
            if let Some(dir) = out {
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let p = dir.join("manifest.json");
                export::write_json(&p, &m).map_err(|e| e.to_string())?;
                eprintln!("wrote {}", p.display());
            }
            Ok(exit_for(&m))
        }
        Command::Packets { replay, out, payloads, max_output_mib } => {
            let file = read(&replay)?;
            let dir = out.unwrap_or_else(|| PathBuf::from("sdl-out").join(export::sanitized_stem(&replay)));
            let opts = Options { depth: Depth::Packets, max_output: max_output_mib << 20, include_payload_hex: payloads };
            let ex = export::export_packets(&replay, &file, &dir, opts).map_err(|e| e.to_string())?;
            print_summary(&ex.manifest);
            println!("wrote {}", ex.manifest_path.display());
            println!("wrote {}", ex.packets_path.display());
            Ok(exit_for(&ex.manifest))
        }
        Command::Survey { paths, out } => {
            let files = collect(&paths);
            let mut rows = Vec::new();
            let mut worst = ExitCode::SUCCESS;
            println!("{:<34} {:>10} {:<10} {:<10} {:>6} {:>9} {:>4} {:>10} {:<6} status", "file", "bytes", "magic", "game", "codec", "packets", "unk", "span_s", "acct");
            for f in files {
                let file = match read(&f) {
                    Ok(b) => b,
                    Err(e) => {
                        println!("{e}");
                        continue;
                    }
                };
                let name = f.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let opts = Options { depth: Depth::Packets, ..Default::default() };
                let m = manifest::analyze(&name, &file, opts, |_, _| Ok(())).map_err(|e| e.to_string())?;
                let p = m.packets.as_ref();
                println!(
                    "{:<34} {:>10} {:<10} {:<10} {:>6} {:>9} {:>4} {:>10} {:<6} {}",
                    name,
                    m.input.size,
                    m.format.as_ref().map(|f| f.format_magic_hex.as_str()).unwrap_or("-"),
                    m.session.as_ref().map(|s| s.game_version.value.as_str()).unwrap_or("-"),
                    m.stream.as_ref().and_then(|s| s.codec_detected).map(|c| format!("{c:?}")).unwrap_or("-".into()),
                    p.map(|p| p.records).unwrap_or(0),
                    p.map(|p| p.unknown_type_packets).unwrap_or(0),
                    p.and_then(|p| p.clock.span_ms).map(|s| format!("{:.1}", s as f64 / 1000.0)).unwrap_or("-".into()),
                    p.map(|p| p.all_bytes_accounted.to_string()).unwrap_or("-".into()),
                    m.status
                );
                if m.status != "ok" {
                    worst = ExitCode::from(1);
                }
                rows.push(m);
            }
            if let Some(o) = out {
                export::write_json(&o, &rows).map_err(|e| e.to_string())?;
                eprintln!("wrote {}", o.display());
            }
            Ok(worst)
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
