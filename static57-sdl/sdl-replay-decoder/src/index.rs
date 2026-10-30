//! `INDEX.md` and `LATEST.md`: where to find SDL's reports.
//!
//! Built by reading every report folder under a root, so replays processed
//! earlier, and by older decoder versions, stay listed. "Latest" means the
//! replay header's recording time. File modification times are never used.

use crate::research::{DATA_FILE, REPORT_FILE};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const INDEX_FILE: &str = "INDEX.md";
pub const LATEST_FILE: &str = "LATEST.md";
pub const VALIDATION_FILE: &str = "validation-data.json";

#[derive(Debug, Clone, PartialEq)]
pub struct ReportEntry {
    /// Report folder, relative to the root, with forward slashes.
    pub folder: String,
    pub file_name: String,
    /// Replay header time, ISO 8601, so text order is time order.
    pub recorded_utc: Option<String>,
    pub game_version: Option<String>,
    pub aircraft: Vec<String>,
    pub mission: Option<String>,
    /// `independent`, `not independent`, `none` or `unreadable`.
    pub validation: &'static str,
    pub decoder_version: Option<String>,
}

fn text(v: &serde_json::Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(String::from)
}

/// Read one dataset. Every field is optional, so older schemas still list.
pub fn entry_from(folder: &str, data: &serde_json::Value, validation: &'static str) -> ReportEntry {
    let r = &data["replay"];
    ReportEntry {
        folder: folder.to_string(),
        file_name: text(&r["file_name"]).unwrap_or_else(|| folder.to_string()),
        recorded_utc: text(&r["start_time_utc"]),
        game_version: text(&r["game_version"]),
        aircraft: data["author"]["vehicles"].as_array().map(|a| a.iter().filter_map(text).collect()).unwrap_or_default(),
        mission: text(&r["mission"]),
        validation,
        decoder_version: text(&data["decoder"]["version"]),
    }
}

fn validation_state(dir: &Path) -> &'static str {
    let path = dir.join(VALIDATION_FILE);
    if !path.exists() {
        return "none";
    }
    let parsed = fs::read(&path).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    match parsed.map(|v| v["independent_validation"].as_bool()) {
        Some(Some(true)) => "independent",
        Some(_) => "not independent",
        None => "unreadable",
    }
}

fn read_entry(dir: &Path, folder: &str) -> Option<ReportEntry> {
    let data: serde_json::Value = serde_json::from_slice(&fs::read(dir.join(DATA_FILE)).ok()?).ok()?;
    Some(entry_from(folder, &data, validation_state(dir)))
}

/// Every report under `root`, newest recording first. Reports with no
/// recording time sort last.
pub fn scan(root: &Path) -> io::Result<Vec<ReportEntry>> {
    let mut out = Vec::new();
    if let Some(e) = read_entry(root, ".") {
        out.push(e);
    }
    for dir in fs::read_dir(root)? {
        let dir = dir?.path();
        if dir.is_dir() {
            let name = dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            out.extend(read_entry(&dir, &name));
        }
    }
    out.sort_by(|a, b| b.recorded_utc.cmp(&a.recorded_utc).then_with(|| a.file_name.cmp(&b.file_name)));
    Ok(out)
}

fn link(e: &ReportEntry) -> String {
    if e.folder == "." { REPORT_FILE.to_string() } else { format!("{}/{REPORT_FILE}", e.folder) }
}

fn or_unknown(v: &Option<String>) -> &str {
    v.as_deref().unwrap_or("unavailable")
}

fn aircraft(e: &ReportEntry) -> String {
    if e.aircraft.is_empty() { "unavailable".to_string() } else { e.aircraft.join(", ") }
}

pub fn render_index(entries: &[ReportEntry]) -> String {
    let mut m = String::from("# SDL Research Index\n\n");
    m.push_str(&format!("{} reports, newest recording first. Dates are the replay header's recording time (UTC). See [LATEST.md](LATEST.md).\n\n", entries.len()));
    m.push_str("| Recorded (UTC) | Replay | Aircraft | Map | Game version | 8111 validation | Report |\n|---|---|---|---|---|---|---|\n");
    for e in entries {
        m.push_str(&format!("| {} | {} | {} | {} | {} | {} | [report]({}) |\n", or_unknown(&e.recorded_utc), e.file_name, aircraft(e), or_unknown(&e.mission), or_unknown(&e.game_version), e.validation, link(e)));
    }
    m
}

/// `None` when no report has a recording time.
pub fn render_latest(entries: &[ReportEntry]) -> Option<String> {
    let e = entries.iter().find(|e| e.recorded_utc.is_some())?;
    let mut m = String::from("# Latest SDL Replay\n\n");
    m.push_str("Chosen by the recording time in the replay header.\n\n");
    m.push_str("| Field | Value |\n|---|---|\n");
    m.push_str(&format!("| Replay | {} |\n", e.file_name));
    m.push_str(&format!("| Recorded (UTC) | {} |\n", or_unknown(&e.recorded_utc)));
    m.push_str(&format!("| Aircraft | {} |\n", aircraft(e)));
    m.push_str(&format!("| Map | {} |\n", or_unknown(&e.mission)));
    m.push_str(&format!("| Game version | {} |\n", or_unknown(&e.game_version)));
    m.push_str(&format!("| 8111 validation | {} |\n\n", e.validation));
    m.push_str(&format!("Report: [{0}]({0})\n\nAll reports: [INDEX.md](INDEX.md)\n", link(e)));
    Some(m)
}

pub struct IndexFiles {
    pub index: PathBuf,
    pub latest: Option<PathBuf>,
    pub reports: usize,
}

/// Write the index files in `root`. Report folders are only read.
pub fn write(root: &Path) -> io::Result<IndexFiles> {
    let entries = scan(root)?;
    let index = root.join(INDEX_FILE);
    fs::write(&index, render_index(&entries))?;
    let latest = render_latest(&entries).map(|m| {
        let p = root.join(LATEST_FILE);
        fs::write(&p, m).map(|_| p)
    });
    Ok(IndexFiles { index, latest: latest.transpose()?, reports: entries.len() })
}

/// Index folder for a report written to `out_dir`: its parent, or the folder
/// itself when `out_dir` is a bare name, so a custom `--out` never fills the
/// working directory.
pub fn root_for(out_dir: &Path) -> PathBuf {
    match out_dir.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => out_dir.to_path_buf(),
    }
}

/// Full path for printing. The file need not exist.
pub fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}
