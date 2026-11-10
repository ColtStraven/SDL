//! Selected MPI (packet type 4) messages.
//!
//! MPI payloads begin with a 4-byte signature. In client replays the client
//! messages read `02 58 <id lo> <id hi>` or `03 58 <id lo> <id hi>`, where the
//! last two bytes are the message id little-endian (REPORTED by
//! WT_KNOWLEDGE_DOCS; consistent with every message listed below).
//!
//! Only messages with a reproducible structural check are decoded here. All
//! other MPI payloads stay raw.

use crate::evidence::{hex, Evidence};
use serde::Serialize;

/// `0xD04A ReplayCockpitParams`: the replay author's cockpit instruments.
pub const SIG_COCKPIT: [u8; 4] = [0x03, 0x58, 0x4A, 0xD0];
/// `0xF058` client kill record.
pub const SIG_KILL: [u8; 4] = [0x02, 0x58, 0x58, 0xF0];
/// `0xF056` client "damage with attacker" record.
pub const SIG_DAMAGE: [u8; 4] = [0x02, 0x58, 0x56, 0xF0];
/// `0xF118` client hit marker (the author hit something).
pub const SIG_HIT_MARKER: [u8; 4] = [0x02, 0x58, 0x18, 0xF1];

/// Cockpit parameter ids: SDL name and the matching localhost:8111
/// `/indicators` key.
///
/// VERIFIED for replay `#2026.11.09 18.50.56` (Me 262 A-1a/U4) against the
/// simultaneous Phosphor 8111 capture: each id matched its `/indicators` field
/// (e.g. id 0 vs `speed` RMS 0.06 m/s, id 40 vs `altitude_hour` RMS 0.24 m,
/// id 52 vs `aviahorizon_roll` RMS 0.32°). Ids can differ by aircraft; on
/// other aircraft treat these names as REPORTED until checked. id 75 is
/// REPORTED only (WT_KNOWLEDGE_DOCS).
pub const COCKPIT_PARAMS: &[(u16, &str, &str)] = &[
    (0, "ias_m_s", "speed"),
    (13, "pedals", "pedals"),
    (23, "stick_elevator", "stick_elevator"),
    (31, "stick_ailerons", "stick_ailerons"),
    (39, "vertical_speed_m_s", "vario"),
    (40, "altitude_m", "altitude_hour"),
    (41, "altitude_min_hand", "altitude_min"),
    (52, "roll_deg_inverted", "aviahorizon_roll"),
    (53, "pitch_deg_inverted", "aviahorizon_pitch"),
    (58, "slip_ball", "bank"),
    (61, "turn_needle", "turn"),
    (64, "compass_deg", "compass"),
    (69, "clock_hour", "clock_hour"),
    (71, "clock_min", "clock_min"),
    (73, "clock_sec", "clock_sec"),
    (75, "manifold_pressure", "manifold_pressure"),
    (81, "engine1_rpm", "rpm"),
    (82, "engine2_rpm", "rpm1"),
    (129, "engine1_oil_pressure", "oil_pressure"),
    (130, "engine2_oil_pressure", "oil_pressure1"),
    (141, "engine1_water_temperature", "water_temperature"),
    (142, "engine2_water_temperature", "water_temperature1"),
    (195, "fuel_kg", "fuel"),
    (196, "fuel1_kg", "fuel1"),
    (198, "fuel_pressure", "fuel_pressure"),
    (199, "fuel_pressure1", "fuel_pressure1"),
    (200, "fuel_pressure2", "fuel_pressure2"),
    (201, "fuel_pressure3", "fuel_pressure3"),
    (225, "gears", "gears"),
    (226, "gears1", "gears1"),
    (231, "flaps", "flaps"),
    (232, "flaps1", "flaps1"),
    (240, "engine1_throttle", "throttle"),
    (241, "engine2_throttle", "throttle1"),
    (330, "ammo_counter1", "ammo_counter1"),
    (331, "ammo_counter2", "ammo_counter2"),
    (346, "ammo_counter1_lamp", "ammo_counter1_lamp"),
    (347, "ammo_counter2_lamp", "ammo_counter2_lamp"),
];

pub fn cockpit_param_name(id: u16) -> Option<&'static str> {
    COCKPIT_PARAMS.iter().find(|p| p.0 == id).map(|p| p.1)
}

/// The localhost:8111 `/indicators` key for a cockpit parameter id.
pub fn cockpit_param_indicator_key(id: u16) -> Option<&'static str> {
    COCKPIT_PARAMS.iter().find(|p| p.0 == id).map(|p| p.2)
}

#[derive(Debug, Clone, Serialize)]
pub struct CockpitParam {
    pub id: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<&'static str>,
    pub value: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CockpitSample {
    pub params: Vec<CockpitParam>,
    /// Declared nested length in bits (LEB128 after the signature).
    pub declared_bits: u64,
}

impl CockpitSample {
    pub fn get(&self, id: u16) -> Option<f32> {
        self.params.iter().find(|p| p.id == id).map(|p| p.value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CockpitError {
    NotCockpit,
    Truncated,
    /// Declared bit length disagrees with `16 + 48 * count` or the payload size.
    LengthMismatch { declared_bits: u64, count: u16, payload_bytes: usize },
}

/// LEB128 (7 bits per byte, low group first, high bit = continue).
pub fn read_leb128(b: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    let mut shift = 0;
    loop {
        let c = *b.get(*pos)?;
        *pos += 1;
        if shift > 56 {
            return None;
        }
        v |= ((c & 0x7f) as u64) << shift;
        shift += 7;
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
}

/// Decode `0xD04A ReplayCockpitParams`:
/// `03 58 4A D0`, LEB128 bit length, `u16le count`, `count x (u16le id, f32le value)`.
///
/// Structure VERIFIED on 2,495 of 2,495 messages in replay
/// `#2026.11.09 18.50.56`: the declared length equals `16 + 48 * count` bits
/// and ends exactly at the end of the payload.
pub fn parse_cockpit(payload: &[u8]) -> Result<CockpitSample, CockpitError> {
    if !payload.starts_with(&SIG_COCKPIT) {
        return Err(CockpitError::NotCockpit);
    }
    let mut p = 4;
    let declared_bits = read_leb128(payload, &mut p).ok_or(CockpitError::Truncated)?;
    let count_bytes = payload.get(p..p + 2).ok_or(CockpitError::Truncated)?;
    let count = u16::from_le_bytes([count_bytes[0], count_bytes[1]]);
    let body_bytes = 2 + 6 * count as usize;
    if declared_bits != 8 * body_bytes as u64 || p + body_bytes != payload.len() {
        return Err(CockpitError::LengthMismatch { declared_bits, count, payload_bytes: payload.len() });
    }
    p += 2;
    let params = (0..count)
        .map(|_| {
            let id = u16::from_le_bytes([payload[p], payload[p + 1]]);
            let value = f32::from_le_bytes(payload[p + 2..p + 6].try_into().unwrap());
            p += 6;
            CockpitParam { id, name: cockpit_param_name(id), value }
        })
        .collect();
    Ok(CockpitSample { params, declared_bits })
}

/// Length-prefixed (`u8` length) printable ASCII strings in a payload, with
/// their offsets. Used to surface vehicle and weapon names without claiming a
/// field layout.
pub fn length_prefixed_strings(b: &[u8], min_len: usize) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let n = b[i] as usize;
        if n >= min_len && i + 1 + n <= b.len() {
            let s = &b[i + 1..i + 1 + n];
            if s.iter().all(|c| (0x21..0x7f).contains(c)) {
                out.push((i + 1, String::from_utf8_lossy(s).into_owned()));
                i += 1 + n;
                continue;
            }
        }
        i += 1;
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct CombatRecord {
    /// `kill`, `damage` or `hit_marker`.
    pub kind: &'static str,
    pub message_id_hex: String,
    /// Vehicle names found as length-prefixed strings. EXPERIMENTAL: which
    /// vehicle (killer or victim) each string names depends on field layout.
    pub vehicle_names: Vec<String>,
    /// For kill records: the two `u16` values directly after the first
    /// vehicle name. Read as `[victim_unit_id, killer_unit_id]` following the
    /// published field order (fields 2, 3, 4). EXPERIMENTAL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_ids_after_name: Option<[u16; 2]>,
    pub evidence: Evidence,
    pub raw_hex: String,
}

pub fn parse_combat(payload: &[u8]) -> Option<CombatRecord> {
    let kind = if payload.starts_with(&SIG_KILL) {
        "kill"
    } else if payload.starts_with(&SIG_DAMAGE) {
        "damage"
    } else if payload.starts_with(&SIG_HIT_MARKER) {
        "hit_marker"
    } else {
        return None;
    };
    let strings = length_prefixed_strings(&payload[4..], 4);
    let unit_ids_after_name = if kind == "kill" {
        strings.first().and_then(|(off, s)| {
            let at = 4 + off + s.len();
            payload.get(at..at + 4).map(|b| [u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]])])
        })
    } else {
        None
    };
    Some(CombatRecord {
        kind,
        message_id_hex: format!("0x{:02X}{:02X}", payload[3], payload[2]),
        vehicle_names: strings.into_iter().map(|(_, s)| s).collect(),
        unit_ids_after_name,
        evidence: Evidence::Experimental,
        raw_hex: hex(payload),
    })
}
