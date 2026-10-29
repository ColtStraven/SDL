//! Replay manifest: the structured inspection report for one `.wrpl` file.

use crate::compression::{self, Inflater, StreamReport};
use crate::container::{self, HeaderField, Layout, Recording};
use crate::datablock::{self, BlkSummary};
use crate::diagnostics::Diagnostics;
use crate::evidence::{sha256_hex, Evidence};
use crate::packets::{self, FrameOptions, FramingSummary, PacketRecord};
use crate::versions::{Codec, FormatProfile};
use serde::Serialize;
use std::io;

pub const MANIFEST_SCHEMA: &str = "static57.sdl.replay-manifest/0.1";
pub const DECODER_NAME: &str = "sdl-replay-decoder";
pub const DECODER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize)]
pub struct Value<T: Serialize> {
    pub value: T,
    pub evidence: Evidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

fn v<T: Serialize>(value: T, evidence: Evidence) -> Value<T> {
    Value { value, evidence, note: None }
}

#[derive(Debug, Clone, Serialize)]
pub struct InputInfo {
    /// File name only; directories are not recorded.
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FormatInfo {
    pub signature_hex: String,
    pub format_magic_hex: String,
    pub recognized: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<FormatProfile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub session_id: Value<u64>,
    pub session_id_hex: String,
    pub replay_part_number: Value<u8>,
    pub game_version_raw_hex: String,
    pub game_version: Value<String>,
    pub start_time_unix: Value<u32>,
    pub start_time_utc: Value<String>,
    pub level_path: Value<String>,
    pub mission_file: Value<String>,
    pub mission_name: Value<String>,
    pub location_name: Value<String>,
    pub environment: Value<String>,
    pub weather: Value<String>,
    pub recording: Recording,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamInfo {
    pub codec_detected: Option<Codec>,
    pub codec_expected: Option<Codec>,
    pub codec_matches_profile: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inflation: Option<StreamReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecoderInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub schema: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub decoder: DecoderInfo,
    /// `ok`: every stage completed without errors. `partial`: the report is
    /// usable but some stage failed or found anomalies flagged as errors.
    /// `rejected`: not a `.wrpl` file.
    pub status: &'static str,
    pub input: InputInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<FormatInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<Session>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings_blk: Option<BlkSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer_blk: Option<BlkSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<StreamInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packets: Option<FramingSummary>,
    /// Every header field with raw bytes, including unknown regions.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub header_fields: Vec<HeaderField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_raw_hex: Option<String>,
    pub diagnostics: Diagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// Header, sections, codec probe and full inflation (no framing).
    Inspect,
    /// Everything in `Inspect` plus packet framing.
    Packets,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub depth: Depth,
    pub max_output: u64,
    pub include_payload_hex: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { depth: Depth::Inspect, max_output: compression::DEFAULT_MAX_OUTPUT, include_payload_hex: false }
    }
}

/// Days-since-epoch to civil date (Howard Hinnant's algorithm).
fn utc_string(unix: u32) -> String {
    let secs = unix as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Run the pipeline on an in-memory replay. `sink` receives every packet
/// record when `opts.depth == Depth::Packets`.
pub fn analyze(
    file_name: &str,
    file: &[u8],
    opts: Options,
    mut sink: impl FnMut(&PacketRecord, &[u8]) -> io::Result<()>,
) -> io::Result<Manifest> {
    let mut diag = Diagnostics::default();
    let mut m = Manifest {
        decoder: DecoderInfo { name: DECODER_NAME, version: DECODER_VERSION, schema: MANIFEST_SCHEMA },
        status: "ok",
        input: InputInfo { file_name: file_name.to_string(), size: file.len() as u64, sha256: sha256_hex(file) },
        format: None,
        session: None,
        layout: None,
        settings_blk: None,
        footer_blk: None,
        stream: None,
        packets: None,
        header_fields: Vec::new(),
        header_raw_hex: None,
        diagnostics: Diagnostics::default(),
    };

    let parsed = match container::parse_header(file, &mut diag) {
        Ok(p) => p,
        Err(e) => {
            diag.error("not_a_replay", "header", Some(0), e.to_string());
            m.status = "rejected";
            m.diagnostics = diag;
            return Ok(m);
        }
    };
    let h = &parsed.header;
    let mut fields = parsed.fields;
    let profile = parsed.profile;
    m.format = Some(FormatInfo {
        signature_hex: format!("0x{:08X}", h.signature),
        format_magic_hex: format!("0x{:08X}", h.format_magic),
        recognized: profile.is_some(),
        profile: profile.cloned(),
    });
    m.header_raw_hex = Some(crate::evidence::hex(&file[..container::HEADER_SIZE]));

    let layout = container::layout(file, h, &mut diag);
    let slice = |s: &container::Section| &file[s.offset as usize..(s.offset + s.length) as usize];
    m.settings_blk = layout.settings_blk.as_ref().map(|s| datablock::summarize(slice(s)));
    m.footer_blk = layout.footer_blk.as_ref().map(|s| datablock::summarize(slice(s)));

    let mut stream = StreamInfo {
        codec_detected: None,
        codec_expected: profile.map(|p| p.expected_codec),
        codec_matches_profile: None,
        inflation: None,
    };
    if let Some(ps) = &layout.packet_stream {
        let data = slice(ps);
        stream.codec_detected = compression::probe(data);
        stream.codec_matches_profile = match (stream.codec_detected, stream.codec_expected) {
            (Some(a), Some(b)) => Some(a == b),
            _ => None,
        };
        match stream.codec_detected {
            Some(_) => {
                // A codec signature exactly at 1234 + settings_blk_size confirms the size field.
                container::promote(&mut fields, "settings_blk_size");
            }
            None => diag.error(
                "unknown_stream_codec",
                "packet_stream",
                Some(ps.offset),
                format!(
                    "no zstd or zlib signature at stream start (first bytes {})",
                    crate::evidence::hex(&data[..data.len().min(8)])
                ),
            ),
        }
        if stream.codec_matches_profile == Some(false) {
            diag.warn(
                "codec_profile_mismatch",
                "packet_stream",
                Some(ps.offset),
                format!("stream codec {:?} differs from profile's expected {:?}", stream.codec_detected, stream.codec_expected),
            );
        }

        if stream.codec_detected.is_some() {
            let mut inf = Inflater::new(data, opts.max_output);
            if opts.depth == Depth::Packets {
                let fo = FrameOptions { segment: h.replay_part_number as u32, include_payload_hex: opts.include_payload_hex };
                let summary = packets::frame_packets(&mut inf, fo, &mut sink)?;
                packet_diagnostics(&summary, &mut diag);
                m.packets = Some(summary);
            } else {
                // Drain to measure the stream exactly; errors are recorded by the inflater.
                let _ = io::copy(&mut inf, &mut io::sink());
            }
            let rep = inf.report();
            if let Some(e) = &rep.error {
                diag.error("decompression_failed", "packet_stream", Some(ps.offset), e.clone());
            }
            if rep.trailing_bytes > 0 {
                diag.warn(
                    "stream_trailing_bytes",
                    "packet_stream",
                    Some(ps.offset + rep.compressed_consumed),
                    format!("{} compressed bytes after the last complete frame were not decoded", rep.trailing_bytes),
                );
            }
            if rep.frames.len() > 1 {
                diag.info("multiple_frames", "packet_stream", Some(ps.offset), format!("{} compressed frames/members", rep.frames.len()));
            }
            let clean = rep.error.is_none() && rep.trailing_bytes == 0 && rep.frames.last().is_some_and(|f| f.complete);
            if clean && layout.footer_blk.is_some() {
                // The stream ended exactly where the footer begins.
                container::promote(&mut fields, "footer_blk_offset");
            }
            stream.inflation = Some(rep);
        }
    }
    m.stream = Some(stream);

    let field_ev = |name: &str| fields.iter().find(|f| f.name == name).map(|f| f.evidence).unwrap_or(Evidence::Unknown);
    let text = |s: &String, name: &str| v(s.clone(), field_ev(name));
    let plausible_time = (1_262_304_000..4_102_444_800).contains(&(h.start_time as i64));
    let mut start_time_utc = v(utc_string(h.start_time), Evidence::Derived);
    if !plausible_time {
        start_time_utc.note = Some("outside 2010..2100; interpretation of start_time is doubtful".into());
    }
    let mut game_version = v(container::game_version_string(h.game_version_raw), Evidence::Experimental);
    game_version.note = Some("four u16le words [build, patch, minor, major]; not documented by any source".into());
    m.session = Some(Session {
        session_id: v(h.session_id, field_ev("session_id")),
        session_id_hex: format!("0x{:016X}", h.session_id),
        replay_part_number: v(h.replay_part_number, field_ev("replay_part_number")),
        game_version_raw_hex: format!("0x{:016X}", h.game_version_raw),
        game_version,
        start_time_unix: v(h.start_time, field_ev("start_time")),
        start_time_utc,
        level_path: text(&h.level_path, "level_path"),
        mission_file: text(&h.mission_file, "mission_file"),
        mission_name: text(&h.mission_name, "mission_name"),
        location_name: text(&h.location_name, "location_name"),
        environment: text(&h.environment, "environment"),
        weather: text(&h.weather, "weather"),
        recording: container::classify_recording(h),
    });
    m.layout = Some(layout);
    m.header_fields = fields;
    m.status = if diag.has_errors() { "partial" } else { "ok" };
    m.diagnostics = diag;
    Ok(m)
}

fn packet_diagnostics(s: &FramingSummary, diag: &mut Diagnostics) {
    let sec = "decompressed_stream";
    if s.truncated {
        diag.error("packet_stream_truncated", sec, Some(s.stream_bytes - s.truncated_tail_bytes), "stream ends inside a packet; partial tail preserved as a `truncated` record");
    }
    if s.unknown_type_packets > 0 {
        diag.warn("unknown_packet_types", sec, None, format!("{} packets with unregistered type ids (preserved raw)", s.unknown_type_packets));
    }
    if s.malformed_records > 0 {
        diag.warn("malformed_packets", sec, None, format!("{} packet bodies too short for their type/timestamp header", s.malformed_records));
    }
    if s.zero_size_records > 0 {
        diag.info("zero_size_records", sec, None, format!("{} zero-size prefixes (reported as padding)", s.zero_size_records));
    }
    if s.end_marker_index.is_none() && !s.truncated {
        diag.warn("no_end_marker", sec, None, "no type-0 end marker packet found");
    }
    if s.records_after_end_marker > 0 {
        diag.warn("records_after_end_marker", sec, None, format!("{} records follow the end marker", s.records_after_end_marker));
    }
    if !s.clock.monotonic {
        diag.warn("non_monotonic_clock", sec, None, format!("{} backward timestamp steps (largest {} ms)", s.clock.backward_steps, s.clock.largest_backward_step_ms));
    }
    if s.clock.inherited_before_first_explicit > 0 {
        // Seen on real replays recorded from session start (first explicit time
        // a few ms): consistent with a running clock initialised to 0.
        diag.info(
            "inherited_before_explicit",
            sec,
            None,
            format!(
                "{} packets inherit time before any explicit timestamp; assigned 0 ms (`inherited_initial`, EXPERIMENTAL)",
                s.clock.inherited_before_first_explicit
            ),
        );
    }
    if s.type_high_bits_packets > 0 {
        diag.warn("type_high_bits", sec, None, format!("{} packets have type bits outside 0x1F set", s.type_high_bits_packets));
    }
}
