//! `.wrpl` container: fixed header, section layout and bounds checks.
//!
//! Layout source: third-party documentation (WT_KNOWLEDGE_DOCS, wrpl-inspector,
//! WrplReplayParser). Every field is preserved as raw bytes alongside its
//! interpreted value. Fields marked `unk*` are kept opaque.

use crate::diagnostics::Diagnostics;
use crate::evidence::{hex, Evidence};
use crate::versions::{self, FormatProfile, WRPL_SIGNATURE};
use serde::Serialize;

pub const HEADER_SIZE: usize = 1234;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    U8,
    U16,
    U32,
    I32,
    U64,
    Bool,
    CStr,
    Opaque,
}

pub struct FieldSpec {
    pub name: &'static str,
    pub offset: usize,
    pub size: usize,
    pub kind: Kind,
}

const fn f(name: &'static str, offset: usize, size: usize, kind: Kind) -> FieldSpec {
    FieldSpec { name, offset, size, kind }
}

/// Header field table. Must tile `[0, HEADER_SIZE)` exactly (enforced by test).
pub const HEADER_FIELDS: &[FieldSpec] = &[
    f("signature", 0, 4, Kind::U32),
    f("format_magic", 4, 4, Kind::U32),
    f("level_path", 8, 128, Kind::CStr),
    f("mission_file", 136, 260, Kind::CStr),
    f("mission_name", 396, 128, Kind::CStr),
    f("environment", 524, 128, Kind::CStr),
    f("weather", 652, 32, Kind::CStr),
    f("footer_blk_offset", 684, 4, Kind::U32),
    f("difficulty_part_1", 688, 8, Kind::U64),
    f("difficulty_part_2", 696, 8, Kind::U64),
    f("unk0", 704, 20, Kind::Opaque),
    f("session_type", 724, 4, Kind::U32),
    f("player_count", 728, 4, Kind::U32),
    f("session_id", 732, 8, Kind::U64),
    f("replay_part_number", 740, 1, Kind::U8),
    f("unk1", 741, 1, Kind::Opaque),
    f("segment_length_sec", 742, 2, Kind::U16),
    f("skies_initial_random_seed", 744, 4, Kind::U32),
    f("settings_blk_size", 748, 2, Kind::U16),
    f("unk3", 750, 29, Kind::Opaque),
    f("is_world_war", 779, 1, Kind::Bool),
    f("location_name", 780, 128, Kind::CStr),
    f("start_time", 908, 4, Kind::U32),
    f("time_limit", 912, 4, Kind::U32),
    f("score_limit", 916, 4, Kind::U32),
    f("kill_limit", 920, 4, Kind::U32),
    f("game_type", 924, 4, Kind::U32),
    f("restore_type", 928, 4, Kind::U32),
    f("player_no", 932, 4, Kind::I32),
    f("unk4", 936, 4, Kind::Opaque),
    f("num_attempts", 940, 4, Kind::U32),
    f("max_attempts", 944, 4, Kind::U32),
    f("is_attempts", 948, 1, Kind::Bool),
    f("is_limited_ammo", 949, 1, Kind::Bool),
    f("is_limited_fuel", 950, 1, Kind::Bool),
    f("unk5", 951, 13, Kind::Opaque),
    f("game_mode", 964, 4, Kind::U32),
    f("chapter_name", 968, 128, Kind::CStr),
    f("battle_kill_streak", 1096, 128, Kind::CStr),
    f("snapshot_period_sec", 1224, 2, Kind::U16),
    f("game_version", 1226, 8, Kind::U64),
];

#[derive(Debug, Clone, Serialize)]
pub struct HeaderField {
    pub name: &'static str,
    pub offset: usize,
    pub size: usize,
    pub raw_hex: String,
    /// Interpreted value (number, bool or string). Null for opaque fields.
    pub value: serde_json::Value,
    pub evidence: Evidence,
    /// For strings: non-zero bytes found after the NUL terminator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub residue_hex: Option<String>,
}

fn le_u64(b: &[u8]) -> u64 {
    let mut v = [0u8; 8];
    v[..b.len()].copy_from_slice(b);
    u64::from_le_bytes(v)
}

fn interpret(spec: &FieldSpec, raw: &[u8]) -> (serde_json::Value, Option<String>) {
    use serde_json::Value;
    match spec.kind {
        Kind::U8 | Kind::U16 | Kind::U32 | Kind::U64 => (Value::from(le_u64(raw)), None),
        Kind::I32 => (Value::from(i32::from_le_bytes(raw.try_into().unwrap())), None),
        Kind::Bool => (Value::from(raw[0] != 0), None),
        Kind::Opaque => (Value::Null, None),
        Kind::CStr => {
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            let s = String::from_utf8_lossy(&raw[..end]).into_owned();
            let tail = &raw[end..];
            let residue = tail.iter().any(|&b| b != 0).then(|| hex(tail));
            (Value::from(s), residue)
        }
    }
}

/// Typed copies of the fields the decoder itself depends on.
#[derive(Debug, Clone, Serialize)]
pub struct Header {
    pub signature: u32,
    pub format_magic: u32,
    pub footer_blk_offset: u32,
    pub session_id: u64,
    pub replay_part_number: u8,
    pub segment_length_sec: u16,
    pub settings_blk_size: u16,
    pub start_time: u32,
    pub player_no: i32,
    pub game_version_raw: u64,
    pub level_path: String,
    pub mission_file: String,
    pub mission_name: String,
    pub location_name: String,
    pub environment: String,
    pub weather: String,
}

pub struct ParsedHeader {
    pub header: Header,
    pub fields: Vec<HeaderField>,
    pub profile: Option<&'static FormatProfile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub name: &'static str,
    pub offset: u64,
    pub length: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Layout {
    pub header: Section,
    pub settings_blk: Option<Section>,
    pub packet_stream: Option<Section>,
    pub footer_blk: Option<Section>,
}

/// Fatal container problems: nothing past these can be interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerError {
    TooShort { len: usize },
    BadSignature { found: u32 },
}

impl std::fmt::Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort { len } => write!(f, "file is {len} bytes; a .wrpl header needs {HEADER_SIZE}"),
            Self::BadSignature { found } => {
                write!(f, "signature 0x{found:08X} at offset 0 is not the .wrpl signature 0x{WRPL_SIGNATURE:08X}")
            }
        }
    }
}

impl std::error::Error for ContainerError {}

/// Parse the fixed header. Fails only when the file is too short or the
/// signature is wrong; an unknown format magic is reported, not rejected.
pub fn parse_header(file: &[u8], diag: &mut Diagnostics) -> Result<ParsedHeader, ContainerError> {
    if file.len() < 8 {
        return Err(ContainerError::TooShort { len: file.len() });
    }
    let signature = u32::from_le_bytes(file[0..4].try_into().unwrap());
    if signature != WRPL_SIGNATURE {
        return Err(ContainerError::BadSignature { found: signature });
    }
    if file.len() < HEADER_SIZE {
        return Err(ContainerError::TooShort { len: file.len() });
    }
    let h = &file[..HEADER_SIZE];
    let format_magic = u32::from_le_bytes(h[4..8].try_into().unwrap());
    let profile = versions::lookup(format_magic);
    if profile.is_none() {
        diag.warn(
            "unknown_format_magic",
            "header",
            Some(4),
            format!("format magic 0x{format_magic:08X} is not in the version registry; header layout applied best-effort and all fields downgraded to EXPERIMENTAL"),
        );
    }

    let mut fields = Vec::with_capacity(HEADER_FIELDS.len());
    for spec in HEADER_FIELDS {
        let raw = &h[spec.offset..spec.offset + spec.size];
        let (value, residue_hex) = interpret(spec, raw);
        let evidence = match (spec.kind, profile.is_some()) {
            (Kind::Opaque, _) => Evidence::Unknown,
            (_, false) => Evidence::Experimental,
            _ => Evidence::Reported,
        };
        fields.push(HeaderField { name: spec.name, offset: spec.offset, size: spec.size, raw_hex: hex(raw), value, evidence, residue_hex });
    }

    let get = |name: &str| fields.iter().find(|x| x.name == name).unwrap();
    let num = |name: &str| get(name).value.as_u64().unwrap_or(0);
    let txt = |name: &str| get(name).value.as_str().unwrap_or("").to_string();
    let header = Header {
        signature,
        format_magic,
        footer_blk_offset: num("footer_blk_offset") as u32,
        session_id: num("session_id"),
        replay_part_number: num("replay_part_number") as u8,
        segment_length_sec: num("segment_length_sec") as u16,
        settings_blk_size: num("settings_blk_size") as u16,
        start_time: num("start_time") as u32,
        player_no: get("player_no").value.as_i64().unwrap_or(0) as i32,
        game_version_raw: num("game_version"),
        level_path: txt("level_path"),
        mission_file: txt("mission_file"),
        mission_name: txt("mission_name"),
        location_name: txt("location_name"),
        environment: txt("environment"),
        weather: txt("weather"),
    };
    // Signature was checked against a constant above.
    if let Some(sig) = fields.iter_mut().find(|x| x.name == "signature") {
        sig.evidence = Evidence::Verified;
    }
    if profile.is_some() {
        if let Some(m) = fields.iter_mut().find(|x| x.name == "format_magic") {
            m.evidence = Evidence::Verified;
        }
    }
    Ok(ParsedHeader { header, fields, profile })
}

/// Mark a header field as verified by a structural cross-check.
pub fn promote(fields: &mut [HeaderField], name: &str) {
    if let Some(x) = fields.iter_mut().find(|x| x.name == name) {
        if x.evidence == Evidence::Reported {
            x.evidence = Evidence::Verified;
        }
    }
}

/// Compute section boundaries and refuse nonsensical offsets.
pub fn layout(file: &[u8], header: &Header, diag: &mut Diagnostics) -> Layout {
    let len = file.len() as u64;
    let section = |name, offset: u64, length: u64| Section {
        name,
        offset,
        length,
        sha256: Some(crate::evidence::sha256_hex(&file[offset as usize..(offset + length) as usize])),
    };
    let header_sec = section("header", 0, HEADER_SIZE as u64);

    let settings_end = HEADER_SIZE as u64 + header.settings_blk_size as u64;
    let settings_blk = if header.settings_blk_size == 0 {
        None
    } else if settings_end > len {
        diag.error(
            "settings_out_of_bounds",
            "header",
            Some(748),
            format!("settings_blk_size {} runs past end of file ({len} bytes)", header.settings_blk_size),
        );
        None
    } else {
        Some(section("settings_blk", HEADER_SIZE as u64, header.settings_blk_size as u64))
    };

    let footer = header.footer_blk_offset as u64;
    let footer_valid = footer != 0 && footer >= settings_end && footer <= len;
    if footer != 0 && !footer_valid {
        diag.error(
            "footer_offset_invalid",
            "header",
            Some(684),
            format!("footer_blk_offset {footer} is outside [{settings_end}, {len}]; treating stream as running to EOF"),
        );
    }
    if footer == 0 {
        diag.info("no_footer", "header", Some(684), "footer_blk_offset is 0: no results block (replay not finalized)");
    }

    let stream_end = if footer_valid { footer } else { len };
    let packet_stream = (settings_end <= len && stream_end > settings_end)
        .then(|| section("packet_stream", settings_end, stream_end - settings_end));
    if packet_stream.is_none() {
        diag.error("no_packet_stream", "container", Some(settings_end), "no bytes available for the packet stream");
    }
    let footer_blk = (footer_valid && footer < len).then(|| section("footer_blk", footer, len - footer));

    Layout { header: header_sec, settings_blk, packet_stream, footer_blk }
}

/// Interpret the 64-bit game version as four little-endian u16 words, most
/// significant last: `[build, patch, minor, major]`.
///
/// EXPERIMENTAL: consistent with Static 57 replays (2.59.0.80..86) but not
/// documented by any source.
pub fn game_version_string(raw: u64) -> String {
    let w = |i: u32| (raw >> (16 * i)) & 0xffff;
    format!("{}.{}.{}.{}", w(3), w(2), w(1), w(0))
}

/// Client/server classification from the two published heuristics.
#[derive(Debug, Clone, Serialize)]
pub struct Recording {
    pub kind: &'static str,
    pub evidence: Evidence,
    pub basis: String,
}

pub fn classify_recording(header: &Header) -> Recording {
    let player_says_server = header.player_no == i32::MIN;
    let segment_says_server = header.segment_length_sec == 0x5A;
    match (player_says_server, segment_says_server) {
        (false, false) => Recording {
            kind: "client",
            evidence: Evidence::Reported,
            basis: format!("player_no={} (server uses 0x80000000), segment_length_sec={} (server uses 90)", header.player_no, header.segment_length_sec),
        },
        (true, true) => Recording {
            kind: "server",
            evidence: Evidence::Reported,
            basis: "player_no=0x80000000 and segment_length_sec=90".into(),
        },
        _ => Recording {
            kind: "unknown",
            evidence: Evidence::Unknown,
            basis: format!("heuristics disagree: player_no={}, segment_length_sec={}", header.player_no, header.segment_length_sec),
        },
    }
}
