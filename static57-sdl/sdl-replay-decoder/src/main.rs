use clap::{Parser, Subcommand};
use sdl_replay_decoder::diagnostics::Severity;
use sdl_replay_decoder::export;
use sdl_replay_decoder::manifest::{self, Depth, Manifest, Options};
use sdl_replay_decoder::strikemaster::Databases;
use sdl_replay_decoder::{index, lab, report, survey, telemetry, validate};
use sdl_replay_decoder::tracks::{Comparison, Dist, FlightReport, TrackConfig};
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
    /// Reconstruct aircraft tracks; write flight_tracks.jsonl and flight_summary.json.
    Flight {
        replay: PathBuf,
        /// Output directory [default: ./sdl-out/<replay name>].
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Reconstruct tracks for many replays; print validation statistics.
    FlightSurvey {
        /// Replay files or directories containing .wrpl files.
        paths: Vec<PathBuf>,
        /// Write all flight reports (without per-track summaries) as JSON.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Generate the research report and dataset for one replay.
    Research {
        replay: PathBuf,
        /// Output directory [default: ./research/<replay name>].
        #[arg(long)]
        out: Option<PathBuf>,
        /// Folder holding the Strike Master databases (read-only).
        #[arg(long)]
        db_dir: Option<PathBuf>,
    },
    /// Generate research reports for many replays plus a cross-replay summary.
    ResearchSurvey {
        /// Replay files or directories containing .wrpl files.
        paths: Vec<PathBuf>,
        /// Output directory [default: ./research].
        #[arg(long)]
        out: Option<PathBuf>,
        /// Folder holding the Strike Master databases (read-only).
        #[arg(long)]
        db_dir: Option<PathBuf>,
    },
    /// Record localhost:8111 telemetry. Run this on the computer that runs War Thunder.
    TelemetryRecord {
        /// Parent folder; each recording gets its own subfolder.
        #[arg(long, default_value = "telemetry")]
        out: PathBuf,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = telemetry::DEFAULT_PORT)]
        port: u16,
        /// Stop after this many seconds [default: until you type q].
        #[arg(long)]
        duration_s: Option<f64>,
    },
    /// Match a capture to its replay, align them, and write a validation report.
    TelemetryValidate {
        /// Capture folder written by telemetry-record.
        capture: PathBuf,
        /// A replay file, or a folder to search by capture time.
        replay: PathBuf,
        /// Output directory [default: ./research/<replay name>].
        #[arg(long)]
        out: Option<PathBuf>,
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

fn fmt_dist(d: &Dist) -> String {
    match (d.p50, d.p90, d.p99) {
        (Some(a), Some(b), Some(c)) => format!("p50 {a:.3} p90 {b:.3} p99 {c:.3} (n {})", d.n),
        _ => "n/a".into(),
    }
}

fn fmt_cmp(c: &Comparison) -> String {
    match c.pearson_r {
        Some(r) => format!("r {r:.5}, |err| {}", fmt_dist(&c.abs_error)),
        None if c.n > 0 => format!("|err| {}", fmt_dist(&c.abs_error)),
        None => "n/a".into(),
    }
}

fn print_flight(name: &str, r: &FlightReport) {
    let c = &r.coverage;
    println!("SDL flight reconstruction — {name} (game {}, {})", r.replay.game_version, r.replay.format_magic);
    println!(
        "  sync packets  {} ({} structurally decoded, {} failed)",
        c.sync_packets, c.structurally_decoded_packets, c.structural_failures
    );
    println!(
        "  records       {} entries: {} state, {} withheld, {} no-data",
        c.entries, c.state_records, c.withheld_entries, c.no_data_entries
    );
    let pct = |x: u64| if c.payload_bits > 0 { 100.0 * x as f64 / c.payload_bits as f64 } else { 0.0 };
    println!(
        "  bit coverage  physical {:.1}%  structural-only {:.1}%  trailer {:.1}%  unparsed {:.1}%",
        pct(c.physically_decoded_bits),
        pct(c.structural_only_bits),
        pct(c.trailer_bits),
        pct(c.unparsed_bits)
    );
    println!("  tracks        {} segments over {} uids, {} samples, author uid(s) {:?}", r.tracks, r.uids, r.samples, r.author_uids);
    let k = &r.kinematic_validation;
    println!("  kinematics    displacement residual m: {}", fmt_dist(&k.displacement_residual_m));
    println!("                displacement direction err deg: {}", fmt_dist(&k.displacement_direction_error_deg));
    println!("                author-only residual m: {}", fmt_dist(&k.author_displacement_residual_m));
    println!("                author-only direction err deg: {}", fmt_dist(&k.author_displacement_direction_error_deg));
    println!("                |heading - track| deg: {}", fmt_dist(&k.heading_minus_track_abs_deg));
    println!("                pitch - flight path deg: {}", fmt_dist(&k.pitch_minus_flight_path_deg));
    let v = &r.cockpit_validation;
    if v.matched > 0 {
        println!(
            "  cockpit       {} matched of {} author samples; altimeter id {:?} in {}, within 2 m {:.1}%",
            v.matched,
            v.author_samples,
            v.altimeter_param_id,
            v.altimeter_unit.unwrap_or("-"),
            v.altitude_within_2m_fraction.unwrap_or(0.0) * 100.0
        );
        println!("                altitude m: {}", fmt_cmp(&v.altitude_m));
        println!("                vertical speed m/s: {}", fmt_cmp(&v.vertical_speed_mps));
        println!("                roll deg: {}", fmt_cmp(&v.roll_deg));
        println!("                pitch deg: {}", fmt_cmp(&v.pitch_deg));
        println!("                IAS estimate (ground speed, ISA) m/s: {}", fmt_cmp(&v.ias_isa_mps));
    } else {
        println!("  cockpit       no matched cockpit messages ({} messages, {} author samples)", v.cockpit_messages, v.author_samples);
    }
}

fn load_db(dir: Option<PathBuf>) -> Result<Option<Databases>, String> {
    dir.map(|d| Databases::load(&d).map_err(|e| format!("cannot read databases in {}: {e}", d.display()))).transpose()
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

/// The replay whose time span overlaps the capture most. Time only proposes
/// a candidate: the validator still has to confirm it.
fn find_replay(cap: &telemetry::Capture, dir: &Path) -> Result<PathBuf, String> {
    let (c0, c1) = (cap.meta.started_utc_ms as f64 / 1000.0, cap.meta.started_utc_ms as f64 / 1000.0 + cap.meta.duration_s.unwrap_or(0.0));
    let mut best: Option<(f64, PathBuf)> = None;
    for f in collect(&[dir.to_path_buf()]) {
        let name = f.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let m = manifest::analyze(&name, &read(&f)?, Options { depth: Depth::Packets, ..Default::default() }, |_, _| Ok(())).map_err(|e| e.to_string())?;
        let Some(s) = &m.session else { continue };
        let r0 = s.start_time_unix.value as f64;
        let r1 = r0 + m.packets.as_ref().and_then(|p| p.clock.span_ms).unwrap_or(0) as f64 / 1000.0;
        let overlap = c1.min(r1) - c0.max(r0);
        if overlap > -300.0 && best.as_ref().is_none_or(|b| overlap > b.0) {
            best = Some((overlap, f));
        }
    }
    best.map(|b| b.1).ok_or_else(|| format!("no replay in {} overlaps this capture's time", dir.display()))
}

/// Refresh `INDEX.md` and `LATEST.md`, and print where they are.
fn print_index(root: &Path) -> Result<(), String> {
    let f = index::write(root).map_err(|e| format!("cannot write the research index: {e}"))?;
    println!("wrote {} ({} reports)", index::absolute(&f.index).display(), f.reports);
    if let Some(l) = f.latest {
        println!("wrote {}", index::absolute(&l).display());
    }
    Ok(())
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
        Command::Flight { replay, out } => {
            let file = read(&replay)?;
            let dir = out.unwrap_or_else(|| PathBuf::from("sdl-out").join(export::sanitized_stem(&replay)));
            let (ex, tracks_path, summary_path) =
                export::export_flight(&replay, &file, &dir, Options::default()).map_err(|e| e.to_string())?;
            print_flight(&ex.manifest.input.file_name, &ex.report);
            println!("wrote {}", tracks_path.display());
            println!("wrote {}", summary_path.display());
            Ok(exit_for(&ex.manifest))
        }
        Command::FlightSurvey { paths, out } => {
            let mut reports = Vec::new();
            let mut worst = ExitCode::SUCCESS;
            for f in collect(&paths) {
                let file = match read(&f) {
                    Ok(b) => b,
                    Err(e) => {
                        println!("{e}");
                        continue;
                    }
                };
                let name = f.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let ex = export::analyze_flight(&name, &file, Options::default(), TrackConfig::default(), |_| Ok(()))
                    .map_err(|e| e.to_string())?;
                print_flight(&name, &ex.report);
                if ex.manifest.status != "ok" || ex.report.coverage.structural_failures > 0 {
                    worst = ExitCode::from(1);
                }
                let mut r = ex.report;
                r.track_summaries.clear();
                reports.push(serde_json::json!({ "file_name": name, "report": r }));
            }
            if let Some(o) = out {
                export::write_json(&o, &reports).map_err(|e| e.to_string())?;
                eprintln!("wrote {}", o.display());
            }
            Ok(worst)
        }
        Command::Research { replay, out, db_dir } => {
            let db = load_db(db_dir)?;
            let file = read(&replay)?;
            let dir = out.unwrap_or_else(|| PathBuf::from("research").join(export::sanitized_stem(&replay)));
            let d = export::export_research(&replay, &file, &dir, db.as_ref()).map_err(|e| e.to_string())?;
            println!("{}: {} tracks, {} weapons, {} engineering findings", d.replay.file_name, d.tracks.len(), d.weapons.stats.weapons_tracked, d.engineering.findings.len());
            println!("wrote {}", index::absolute(&dir.join("research-report.md")).display());
            println!("wrote {}", index::absolute(&dir.join("research-data.json")).display());
            println!("wrote {}", index::absolute(&dir.join("weapon_tracks.jsonl")).display());
            print_index(&index::root_for(&dir))?;
            Ok(if d.replay.status == "ok" { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Command::ResearchSurvey { paths, out, db_dir } => {
            let db = load_db(db_dir)?;
            let root = out.unwrap_or_else(|| PathBuf::from("research"));
            std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
            let (mut all, mut dirs) = (Vec::new(), Vec::new());
            for f in collect(&paths) {
                let file = read(&f)?;
                let stem = export::sanitized_stem(&f);
                let d = export::export_research(&f, &file, &root.join(&stem), db.as_ref()).map_err(|e| e.to_string())?;
                println!("{}: {} tracks, {} weapons, {} engineering findings", d.replay.file_name, d.tracks.len(), d.weapons.stats.weapons_tracked, d.engineering.findings.len());
                println!("wrote {}", index::absolute(&root.join(&stem).join("research-report.md")).display());
                all.push(d);
                dirs.push(stem);
            }
            let c = survey::analyze(&all, &dirs);
            export::write_json(&root.join("research-summary.json"), &c).map_err(|e| e.to_string())?;
            std::fs::write(root.join("research-summary.md"), report::render_summary(&c)).map_err(|e| e.to_string())?;
            println!("wrote {}", index::absolute(&root.join("research-summary.md")).display());
            print_index(&root)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::TelemetryRecord { out, host, port, duration_s } => {
            let dir = out.join(telemetry::utc_stamp(telemetry::utc_now_ms()));
            let cfg = telemetry::RecordConfig { out_dir: dir.clone(), host, port, duration_s, endpoints: telemetry::default_endpoints() };
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for line in std::io::stdin().lines().map_while(Result::ok) {
                    let t = line.trim();
                    let cmd = if t.eq_ignore_ascii_case("q") { telemetry::Command::Stop } else { telemetry::Command::Mark(t.to_string()) };
                    if tx.send(cmd).is_err() {
                        break;
                    }
                }
            });
            println!("Recording {}:{} to {}", cfg.host, cfg.port, dir.display());
            println!("Press Enter for a MARK. Type q then Enter to stop. It is safe to start this before the game.");
            let mut last = 0.0;
            let meta = telemetry::record(&cfg, rx, |s| {
                if s.elapsed_s - last >= 5.0 {
                    last = s.elapsed_s;
                    println!("{:>7.0} s  flight readings {:>7}  errors {:>6}  marks {}", s.elapsed_s, s.valid_state, s.errors, s.marks);
                }
            })
            .map_err(|e| e.to_string())?;
            println!("Stopped ({}) after {:.1} s. Files are in {}", meta.stop_reason.as_deref().unwrap_or("-"), meta.duration_s.unwrap_or(0.0), dir.display());
            Ok(ExitCode::SUCCESS)
        }
        Command::TelemetryValidate { capture, replay, out } => {
            let cap = telemetry::load(&capture).map_err(|e| format!("cannot read capture {}: {e}", capture.display()))?;
            let path = if replay.is_dir() { find_replay(&cap, &replay)? } else { replay };
            let file = read(&path)?;
            let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let x = lab::extract(&name, &file, Options::default(), sdl_replay_decoder::tracks::TrackConfig::default(), |_| {}).map_err(|e| e.to_string())?;
            let v = validate::validate(&cap, &validate::ReplayView::of(&x, &name));
            let dir = out.unwrap_or_else(|| PathBuf::from("research").join(export::sanitized_stem(&path)));
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            export::write_json(&dir.join("validation-data.json"), &v).map_err(|e| e.to_string())?;
            std::fs::write(dir.join("validation-report.md"), report::render_validation(&v)).map_err(|e| e.to_string())?;
            println!("{}", v.status);
            println!("alignment {}: offset {:?} s, uncertainty {:?} s, drift {:?} ppm", v.sync.quality, v.sync.offset_s, v.sync.uncertainty_s, v.sync.drift_ppm);
            println!("wrote {}", index::absolute(&dir.join("validation-report.md")).display());
            print_index(&index::root_for(&dir))?;
            println!("Run research on this replay to add the validation to its engineering findings.");
            Ok(if v.independent_validation { ExitCode::SUCCESS } else { ExitCode::from(1) })
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
