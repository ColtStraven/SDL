//! Independent capture of War Thunder's local telemetry server (port 8111).
//!
//! The recorder is read-only: it issues plain HTTP GET requests for a fixed
//! list of paths and stores each response body unmodified, one JSON line per
//! request. It uses the standard library only, needs no other program, and
//! has no connection to Strike Master.
//!
//! Field names are REPORTED by community documentation, so the loader keeps
//! every numeric field it finds. Nothing depends on a field being present.
//!
//! Time: every request carries session-monotonic start and end times, which
//! are the measurement clock, and a UTC stamp, used only to associate a
//! capture with a replay.

use crate::evidence::sha256_hex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const CAPTURE_SCHEMA: &str = "static57.sdl.telemetry-capture/0.1";
pub const SESSION_FILE: &str = "session.json";
pub const CAPTURE_FILE: &str = "capture.jsonl";
pub const DEFAULT_PORT: u16 = 8111;
/// Polling stops this long after a failed request, so a closed game does not
/// fill the log with errors.
pub const ERROR_BACKOFF_S: f64 = 1.0;
const REQUEST_TIMEOUT: Duration = Duration::from_millis(1_000);
const MAX_RESPONSE: usize = 8 << 20;

/// One polled resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EndpointConfig {
    pub name: String,
    pub path: String,
    pub hz: f64,
}

/// The default capture set.
pub fn default_endpoints() -> Vec<EndpointConfig> {
    let e = |name: &str, path: &str, hz| EndpointConfig { name: name.into(), path: path.into(), hz };
    vec![
        e("state", "/state", 10.0),
        e("indicators", "/indicators", 10.0),
        e("map_obj", "/map_obj.json", 2.0),
        e("map_info", "/map_info.json", 0.2),
        e("hudmsg", "/hudmsg", 1.0),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub schema: String,
    pub decoder_version: String,
    /// Written by a simulator, not by War Thunder. Never independent evidence.
    pub synthetic: bool,
    pub host: String,
    pub port: u16,
    pub computer: Option<String>,
    pub started_utc_ms: u64,
    pub started_utc: String,
    pub ended_utc_ms: Option<u64>,
    pub duration_s: Option<f64>,
    pub stop_reason: Option<String>,
    pub endpoints: Vec<EndpointConfig>,
}

/// One line of `capture.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CaptureRecord {
    pub seq: u64,
    /// Endpoint name, or `mark` for a manual mark.
    pub endpoint: String,
    /// Seconds since session start, monotonic, when the request was sent.
    pub t_start_s: f64,
    /// Seconds since session start, monotonic, when the response had arrived.
    pub t_end_s: f64,
    pub utc_ms: u64,
    pub status: Option<u16>,
    /// HTTP 200 and a body that parses as JSON.
    pub ok: bool,
    /// The response's own `valid` field, if it has one.
    pub valid: Option<bool>,
    pub bytes: u64,
    pub error: Option<String>,
    pub note: Option<String>,
    /// Raw response body, unmodified.
    pub body: Option<String>,
}

pub fn utc_now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// `YYYYMMDD_HHMMSS` for capture folder names.
pub fn utc_stamp(utc_ms: u64) -> String {
    let (y, mo, d, h, mi, s) = utc_civil(utc_ms);
    format!("{y:04}{mo:02}{d:02}_{h:02}{mi:02}{s:02}")
}

fn utc_iso(utc_ms: u64) -> String {
    let (y, mo, d, h, mi, s) = utc_civil(utc_ms);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn utc_civil(utc_ms: u64) -> (i64, u32, u32, u32, u32, u32) {
    let secs = (utc_ms / 1000) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m as u32, d as u32, (rem / 3600) as u32, (rem % 3600 / 60) as u32, (rem % 60) as u32)
}

/// Split an HTTP response into status and body. Accepts `Content-Length`,
/// chunked encoding, and a body that simply runs to the end of the stream.
pub fn parse_http(raw: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("no end of HTTP headers")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let mut lines = head.split("\r\n");
    let status = lines.next().and_then(|l| l.split_whitespace().nth(1)).and_then(|s| s.parse().ok()).ok_or("bad HTTP status line")?;
    let (mut length, mut chunked) = (None, false);
    for l in lines {
        let Some((k, v)) = l.split_once(':') else { continue };
        if k.eq_ignore_ascii_case("content-length") {
            length = v.trim().parse::<usize>().ok();
        } else if k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        }
    }
    let body = &raw[split + 4..];
    if chunked {
        return dechunk(body).map(|b| (status, b));
    }
    match length {
        Some(n) if n > body.len() => Err(format!("truncated body: {} of {n} bytes", body.len())),
        Some(n) => Ok((status, body[..n].to_vec())),
        None => Ok((status, body.to_vec())),
    }
}

fn dechunk(mut b: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let e = b.windows(2).position(|w| w == b"\r\n").ok_or("truncated chunk size")?;
        let size = String::from_utf8_lossy(&b[..e]);
        let n = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16).map_err(|_| "bad chunk size")?;
        if n == 0 {
            return Ok(out);
        }
        let start = e + 2;
        let chunk = b.get(start..start + n).ok_or("truncated chunk")?;
        out.extend_from_slice(chunk);
        b = b.get(start + n + 2..).ok_or("truncated chunk")?;
    }
}

/// One read-only HTTP GET.
pub fn http_get(addr: &SocketAddr, host: &str, path: &str) -> Result<(u16, Vec<u8>), String> {
    let mut s = TcpStream::connect_timeout(addr, REQUEST_TIMEOUT).map_err(|e| format!("connect: {e}"))?;
    s.set_read_timeout(Some(REQUEST_TIMEOUT)).map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(REQUEST_TIMEOUT)).map_err(|e| e.to_string())?;
    let _ = s.set_nodelay(true);
    write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: */*\r\nConnection: close\r\n\r\n").map_err(|e| format!("send: {e}"))?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() > MAX_RESPONSE {
                    return Err("response too large".into());
                }
            }
            // A server that holds the connection open has already sent what
            // the length header promised, or the parse below reports it.
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) && !raw.is_empty() => break,
            Err(e) => return Err(format!("read: {e}")),
        }
    }
    parse_http(&raw)
}

/// Commands accepted while recording.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Mark(String),
    Stop,
}

#[derive(Debug, Clone)]
pub struct RecordConfig {
    pub out_dir: PathBuf,
    pub host: String,
    pub port: u16,
    pub duration_s: Option<f64>,
    pub endpoints: Vec<EndpointConfig>,
}

/// Progress passed to the status callback about once per second.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub elapsed_s: f64,
    pub requests: u64,
    pub ok: u64,
    pub valid_state: u64,
    pub errors: u64,
    pub marks: u64,
}

fn body_flags(body: &str) -> (bool, Option<bool>) {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) => (true, v.get("valid").and_then(|x| x.as_bool())),
        Err(_) => (false, None),
    }
}

#[derive(Default)]
struct Counters {
    requests: AtomicU64,
    ok: AtomicU64,
    valid_state: AtomicU64,
    errors: AtomicU64,
}

fn poll(endpoint: EndpointConfig, addr: SocketAddr, host: String, t0: Instant, stop: Arc<AtomicBool>, tx: Sender<CaptureRecord>, c: Arc<Counters>) {
    let period = 1.0 / endpoint.hz.max(0.01);
    let is_hud = endpoint.name == "hudmsg";
    let (mut last_evt, mut last_dmg) = (0u64, 0u64);
    let mut next = 0.0f64;
    while !stop.load(Ordering::Relaxed) {
        let now = t0.elapsed().as_secs_f64();
        if now < next {
            std::thread::sleep(Duration::from_secs_f64((next - now).min(0.05)));
            continue;
        }
        let path = if is_hud { format!("{}?lastEvt={last_evt}&lastDmg={last_dmg}", endpoint.path) } else { endpoint.path.clone() };
        let start = t0.elapsed().as_secs_f64();
        let utc_ms = utc_now_ms();
        let result = http_get(&addr, &host, &path);
        let end = t0.elapsed().as_secs_f64();
        let mut r = CaptureRecord { seq: 0, endpoint: endpoint.name.clone(), t_start_s: start, t_end_s: end, utc_ms, status: None, ok: false, valid: None, bytes: 0, error: None, note: None, body: None };
        match result {
            Ok((status, body)) => {
                let text = String::from_utf8_lossy(&body).into_owned();
                let (json, valid) = body_flags(&text);
                r.status = Some(status);
                r.ok = status == 200 && json;
                r.valid = valid;
                r.bytes = body.len() as u64;
                if !r.ok {
                    r.error = Some(if status == 200 { "body is not JSON".into() } else { format!("HTTP {status}") });
                }
                if is_hud && r.ok {
                    let (e, d) = hud_cursor(&text);
                    last_evt = last_evt.max(e);
                    last_dmg = last_dmg.max(d);
                }
                r.body = Some(text);
            }
            Err(e) => r.error = Some(e),
        }
        c.requests.fetch_add(1, Ordering::Relaxed);
        c.ok.fetch_add(r.ok as u64, Ordering::Relaxed);
        c.errors.fetch_add(!r.ok as u64, Ordering::Relaxed);
        c.valid_state.fetch_add((endpoint.name == "state" && r.valid == Some(true)) as u64, Ordering::Relaxed);
        let failed = !r.ok;
        if tx.send(r).is_err() {
            return;
        }
        next = if failed { end + ERROR_BACKOFF_S } else { (next + period).max(end) };
    }
}

fn hud_cursor(body: &str) -> (u64, u64) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else { return (0, 0) };
    let max_id = |k: &str| v.get(k).and_then(|a| a.as_array()).map_or(0, |a| a.iter().filter_map(|e| e.get("id").and_then(|i| i.as_u64())).max().unwrap_or(0));
    (max_id("events"), max_id("damage"))
}

/// Record until `Command::Stop`, the duration, or the command channel closing
/// with no duration set. Every record is flushed to disk as it arrives, so
/// killing the process loses at most the record being written.
pub fn record(cfg: &RecordConfig, commands: Receiver<Command>, mut status: impl FnMut(&Status)) -> io::Result<SessionMeta> {
    fs::create_dir_all(&cfg.out_dir)?;
    let capture_path = cfg.out_dir.join(CAPTURE_FILE);
    if capture_path.exists() {
        return Err(io::Error::other(format!("{} already exists", capture_path.display())));
    }
    let addr = (cfg.host.as_str(), cfg.port).to_socket_addrs()?.next().ok_or_else(|| io::Error::other("host did not resolve"))?;
    let started_utc_ms = utc_now_ms();
    let mut meta = SessionMeta {
        schema: CAPTURE_SCHEMA.to_string(),
        decoder_version: env!("CARGO_PKG_VERSION").to_string(),
        synthetic: false,
        host: cfg.host.clone(),
        port: cfg.port,
        computer: std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok(),
        started_utc_ms,
        started_utc: utc_iso(started_utc_ms),
        ended_utc_ms: None,
        duration_s: None,
        stop_reason: None,
        endpoints: cfg.endpoints.clone(),
    };
    // Written now and again at the end, so a killed recorder leaves metadata.
    crate::export::write_json(&cfg.out_dir.join(SESSION_FILE), &meta)?;

    let t0 = Instant::now();
    let stop = Arc::new(AtomicBool::new(false));
    let counters = Arc::new(Counters::default());
    let (tx, rx) = mpsc::channel::<CaptureRecord>();
    let mut workers = Vec::new();
    for e in &cfg.endpoints {
        let (e, host, stop, tx, c) = (e.clone(), cfg.host.clone(), stop.clone(), tx.clone(), counters.clone());
        workers.push(std::thread::spawn(move || poll(e, addr, host, t0, stop, tx, c)));
    }

    let mut w = BufWriter::new(File::create(&capture_path)?);
    let mut seq = 0u64;
    let mut marks = 0u64;
    let mut next_status = 1.0;
    let mut commands_open = true;
    let reason = loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(mut r) => {
                seq += 1;
                r.seq = seq;
                serde_json::to_writer(&mut w, &r)?;
                w.write_all(b"\n")?;
                w.flush()?;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break "no endpoints",
        }
        let now = t0.elapsed().as_secs_f64();
        while commands_open {
            match commands.try_recv() {
                Ok(Command::Mark(note)) => {
                    seq += 1;
                    marks += 1;
                    let r = CaptureRecord { seq, endpoint: "mark".into(), t_start_s: now, t_end_s: now, utc_ms: utc_now_ms(), status: None, ok: true, valid: None, bytes: 0, error: None, note: Some(note), body: None };
                    serde_json::to_writer(&mut w, &r)?;
                    w.write_all(b"\n")?;
                    w.flush()?;
                }
                Ok(Command::Stop) => {
                    stop.store(true, Ordering::Relaxed);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => commands_open = false,
            }
        }
        if stop.load(Ordering::Relaxed) {
            break "stop command";
        }
        if cfg.duration_s.is_some_and(|d| now >= d) {
            stop.store(true, Ordering::Relaxed);
            break "duration reached";
        }
        if !commands_open && cfg.duration_s.is_none() {
            stop.store(true, Ordering::Relaxed);
            break "input closed";
        }
        if now >= next_status {
            next_status = now + 1.0;
            status(&Status {
                elapsed_s: now,
                requests: counters.requests.load(Ordering::Relaxed),
                ok: counters.ok.load(Ordering::Relaxed),
                valid_state: counters.valid_state.load(Ordering::Relaxed),
                errors: counters.errors.load(Ordering::Relaxed),
                marks,
            });
        }
    };
    drop(tx);
    for h in workers {
        let _ = h.join();
    }
    for mut r in rx.try_iter() {
        seq += 1;
        r.seq = seq;
        serde_json::to_writer(&mut w, &r)?;
        w.write_all(b"\n")?;
    }
    w.flush()?;
    let end = utc_now_ms();
    meta.ended_utc_ms = Some(end);
    meta.duration_s = Some(t0.elapsed().as_secs_f64());
    meta.stop_reason = Some(reason.to_string());
    crate::export::write_json(&cfg.out_dir.join(SESSION_FILE), &meta)?;
    Ok(meta)
}

/// One parsed `/state` or `/indicators` response.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// Midpoint of the request, seconds since session start.
    pub t_s: f64,
    /// Half the round trip: the bound on when the server sampled the value.
    pub half_rtt_s: f64,
    pub utc_ms: u64,
    pub fields: BTreeMap<String, f64>,
    /// `/indicators` `type`.
    pub vehicle: Option<String>,
}

impl Observation {
    pub fn get(&self, key: &str) -> Option<f64> {
        self.fields.get(key).copied()
    }
}

/// The player's `/map_obj.json` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct MapObservation {
    pub t_s: f64,
    pub x: f64,
    pub y: f64,
    pub dx: Option<f64>,
    pub dy: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MapInfo {
    pub map_min: [f64; 2],
    pub map_max: [f64; 2],
    pub grid_zero: Option<[f64; 2]>,
    pub grid_steps: Option<[f64; 2]>,
    pub map_generation: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HudEvent {
    /// When SDL first saw the message.
    pub t_s: f64,
    pub id: u64,
    pub msg: String,
    /// The message's own time, seconds, if the game supplies one.
    pub time_s: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Mark {
    pub t_s: f64,
    pub utc_ms: u64,
    pub note: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct EndpointStats {
    pub requests: u64,
    pub ok: u64,
    pub valid_true: u64,
    pub valid_false: u64,
    pub errors: u64,
    pub first_error: Option<String>,
    pub median_interval_s: Option<f64>,
    pub max_interval_s: Option<f64>,
    pub rtt_p50_s: Option<f64>,
    pub rtt_p99_s: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Capture {
    pub dir: PathBuf,
    pub meta: SessionMeta,
    pub sha256: String,
    pub bytes: u64,
    pub lines: u64,
    /// A final line cut off by a killed process.
    pub partial_lines: u64,
    pub state: Vec<Observation>,
    pub indicators: Vec<Observation>,
    pub map_obj: Vec<MapObservation>,
    pub map_info: Option<MapInfo>,
    pub hud: Vec<HudEvent>,
    pub marks: Vec<Mark>,
    pub endpoints: BTreeMap<String, EndpointStats>,
    /// Least-squares `d(utc)/d(monotonic)` minus 1, parts per million.
    pub utc_vs_monotonic_ppm: Option<f64>,
}

fn numeric(v: &serde_json::Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn pair(v: Option<&serde_json::Value>) -> Option<[f64; 2]> {
    let a = v?.as_array()?;
    Some([numeric(a.first()?)?, numeric(a.get(1)?)?])
}

fn observation(r: &CaptureRecord, v: &serde_json::Value) -> Option<Observation> {
    let o = v.as_object()?;
    let mut fields = BTreeMap::new();
    for (k, x) in o {
        if k != "valid" {
            if let Some(n) = x.as_f64() {
                fields.insert(k.clone(), n);
            }
        }
    }
    Some(Observation {
        t_s: 0.5 * (r.t_start_s + r.t_end_s),
        half_rtt_s: 0.5 * (r.t_end_s - r.t_start_s),
        utc_ms: r.utc_ms,
        fields,
        vehicle: o.get("type").and_then(|t| t.as_str()).map(String::from),
    })
}

fn player_object(t_s: f64, v: &serde_json::Value) -> Option<MapObservation> {
    let p = v.as_array()?.iter().find(|o| o.get("icon").and_then(|i| i.as_str()) == Some("Player"))?;
    Some(MapObservation { t_s, x: p.get("x")?.as_f64()?, y: p.get("y")?.as_f64()?, dx: p.get("dx").and_then(|x| x.as_f64()), dy: p.get("dy").and_then(|x| x.as_f64()) })
}

fn map_info(v: &serde_json::Value) -> Option<MapInfo> {
    Some(MapInfo {
        map_min: pair(v.get("map_min"))?,
        map_max: pair(v.get("map_max"))?,
        grid_zero: pair(v.get("grid_zero")),
        grid_steps: pair(v.get("grid_steps")),
        map_generation: v.get("map_generation").and_then(numeric),
    })
}

fn pct(v: &[f64], p: f64) -> Option<f64> {
    (!v.is_empty()).then(|| v[((v.len() - 1) as f64 * p) as usize])
}

/// Read a capture folder. The files are never modified.
pub fn load(dir: &Path) -> io::Result<Capture> {
    let meta: SessionMeta = serde_json::from_slice(&fs::read(dir.join(SESSION_FILE))?).map_err(io::Error::other)?;
    let path = dir.join(CAPTURE_FILE);
    let raw = fs::read(&path)?;
    let mut c = Capture {
        dir: dir.to_path_buf(),
        meta,
        sha256: sha256_hex(&raw),
        bytes: raw.len() as u64,
        lines: 0,
        partial_lines: 0,
        state: Vec::new(),
        indicators: Vec::new(),
        map_obj: Vec::new(),
        map_info: None,
        hud: Vec::new(),
        marks: Vec::new(),
        endpoints: BTreeMap::new(),
        utc_vs_monotonic_ppm: None,
    };
    let mut times: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut rtts: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut clock = Vec::new();
    let mut seen_hud = std::collections::BTreeSet::new();
    for line in BufReader::new(raw.as_slice()).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(r) = serde_json::from_str::<CaptureRecord>(&line) else {
            c.partial_lines += 1;
            continue;
        };
        c.lines += 1;
        if r.endpoint == "mark" {
            c.marks.push(Mark { t_s: r.t_start_s, utc_ms: r.utc_ms, note: r.note.unwrap_or_default() });
            continue;
        }
        let s = c.endpoints.entry(r.endpoint.clone()).or_default();
        s.requests += 1;
        s.ok += r.ok as u64;
        s.valid_true += (r.valid == Some(true)) as u64;
        s.valid_false += (r.valid == Some(false)) as u64;
        if !r.ok {
            s.errors += 1;
            if s.first_error.is_none() {
                s.first_error = r.error.clone();
            }
        }
        let t = 0.5 * (r.t_start_s + r.t_end_s);
        times.entry(r.endpoint.clone()).or_default().push(t);
        rtts.entry(r.endpoint.clone()).or_default().push(r.t_end_s - r.t_start_s);
        clock.push((r.t_start_s, r.utc_ms as f64 / 1000.0));
        if !r.ok {
            continue;
        }
        let Some(v) = r.body.as_deref().and_then(|b| serde_json::from_str::<serde_json::Value>(b).ok()) else { continue };
        match r.endpoint.as_str() {
            "state" if r.valid != Some(false) => c.state.extend(observation(&r, &v)),
            "indicators" if r.valid != Some(false) => c.indicators.extend(observation(&r, &v)),
            "map_obj" => c.map_obj.extend(player_object(t, &v)),
            "map_info" if r.valid != Some(false) => {
                if let Some(m) = map_info(&v) {
                    c.map_info = Some(m);
                }
            }
            "hudmsg" => {
                for key in ["damage", "events"] {
                    for e in v.get(key).and_then(|a| a.as_array()).into_iter().flatten() {
                        let Some(id) = e.get("id").and_then(|i| i.as_u64()) else { continue };
                        if seen_hud.insert((key, id)) {
                            c.hud.push(HudEvent { t_s: t, id, msg: e.get("msg").and_then(|m| m.as_str()).unwrap_or("").to_string(), time_s: e.get("time").and_then(numeric) });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for (k, mut t) in times {
        t.sort_by(f64::total_cmp);
        let mut d: Vec<f64> = t.windows(2).map(|w| w[1] - w[0]).collect();
        d.sort_by(f64::total_cmp);
        let s = c.endpoints.get_mut(&k).unwrap();
        s.median_interval_s = pct(&d, 0.5);
        s.max_interval_s = d.last().copied();
        if let Some(r) = rtts.get_mut(&k) {
            r.sort_by(f64::total_cmp);
            s.rtt_p50_s = pct(r, 0.5);
            s.rtt_p99_s = pct(r, 0.99);
        }
    }
    if clock.len() >= 10 {
        let n = clock.len() as f64;
        let (mx, my) = (clock.iter().map(|p| p.0).sum::<f64>() / n, clock.iter().map(|p| p.1).sum::<f64>() / n);
        let (mut sxy, mut sxx) = (0.0, 0.0);
        for (x, y) in &clock {
            sxy += (x - mx) * (y - my);
            sxx += (x - mx) * (x - mx);
        }
        if sxx > 100.0 {
            c.utc_vs_monotonic_ppm = Some((sxy / sxx - 1.0) * 1e6);
        }
    }
    c.state.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
    c.indicators.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
    c.map_obj.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
    Ok(c)
}
