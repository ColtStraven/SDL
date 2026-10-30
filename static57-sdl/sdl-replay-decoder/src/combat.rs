//! Battle log records: kills (MPI 0xF058) and damage (0xF056, 0xF157).
//!
//! Field numbers are the id-field mask bits. Meanings are REPORTED by
//! third-party sources; SDL cross-checks them against the unit registry.

use crate::cockpit::mpi_header;
use crate::idfield::{self, Field};
use serde::Serialize;

pub const MSG_KILL: u16 = 0xF058;
pub const MSG_CRITICAL_DAMAGE: u16 = 0xF056;
pub const MSG_SEVERE_DAMAGE: u16 = 0xF157;
/// Reported as a hit marker for the recording author.
pub const MSG_HIT_MARKER: u16 = 0xF118;

pub const NO_UID: u16 = 0x7FF;

fn u16_field(msg: &[u8], fields: &[Field], n: u8) -> Option<u16> {
    let f = fields.iter().find(|f| f.number == n)?;
    idfield::field_reader(msg, f).u16_le("field").ok()
}

fn u32_field(msg: &[u8], fields: &[Field], n: u8) -> Option<u32> {
    let f = fields.iter().find(|f| f.number == n)?;
    idfield::field_reader(msg, f).u32_le("field").ok()
}

fn str_field(msg: &[u8], fields: &[Field], n: u8) -> Option<String> {
    let f = fields.iter().find(|f| f.number == n)?;
    idfield::read_string(msg, f).ok().filter(|s| !s.is_empty())
}

/// High bits of a unit reference hold the object kind: 0 is an aircraft.
fn is_surface(v: Option<u16>) -> bool {
    v.is_some_and(|u| u != 0xFFFF && u >> 11 != 0)
}

fn uid(v: Option<u16>) -> Option<u16> {
    v.map(|u| u & 0x7FF).filter(|u| *u != NO_UID)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KillEvent {
    pub timestamp_ms: u32,
    pub packet_index: u64,
    pub victim_uid: Option<u16>,
    pub killer_uid: Option<u16>,
    /// The victim or killer reference has a non-aircraft object kind.
    pub victim_surface: bool,
    pub killer_surface: bool,
    pub killer_slot: Option<u32>,
    /// The killer's vehicle, not the victim's.
    pub killer_vehicle: Option<String>,
    pub weapon: Option<String>,
    /// Field 12: reported as the destroyed weapon/round name.
    pub destroyed_weapon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DamageEvent {
    pub timestamp_ms: u32,
    pub packet_index: u64,
    /// `critical` or `severe`.
    pub kind: &'static str,
    pub victim_uid: Option<u16>,
    pub victim_slot: Option<u32>,
    pub victim_vehicle: Option<String>,
    pub attacker_uid: Option<u16>,
    pub victim_surface: bool,
    pub attacker_surface: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HitMarker {
    pub timestamp_ms: u32,
    pub packet_index: u64,
    pub target_uid: Option<u16>,
    pub target_surface: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CombatRecord {
    Kill(KillEvent),
    Damage(DamageEvent),
    Hit(HitMarker),
}

/// Decode a type-4 payload if it is a recognised battle record.
pub fn decode(timestamp_ms: u32, packet_index: u64, payload: &[u8]) -> Option<CombatRecord> {
    let (_, mid) = mpi_header(payload)?;
    if !matches!(mid, MSG_KILL | MSG_CRITICAL_DAMAGE | MSG_SEVERE_DAMAGE | MSG_HIT_MARKER) {
        return None;
    }
    let msg = &payload[4..];
    let fields = idfield::parse(msg).ok()?;
    Some(match mid {
        MSG_KILL => CombatRecord::Kill(KillEvent {
            timestamp_ms,
            packet_index,
            victim_uid: uid(u16_field(msg, &fields, 3)),
            killer_uid: uid(u16_field(msg, &fields, 4)),
            victim_surface: is_surface(u16_field(msg, &fields, 3)),
            killer_surface: is_surface(u16_field(msg, &fields, 4)),
            killer_slot: u32_field(msg, &fields, 1).filter(|v| *v != u32::MAX),
            killer_vehicle: str_field(msg, &fields, 2),
            weapon: str_field(msg, &fields, 10),
            destroyed_weapon: str_field(msg, &fields, 12),
        }),
        MSG_HIT_MARKER => CombatRecord::Hit(HitMarker {
            timestamp_ms,
            packet_index,
            target_uid: uid(u16_field(msg, &fields, 1)),
            target_surface: is_surface(u16_field(msg, &fields, 1)),
        }),
        _ => CombatRecord::Damage(DamageEvent {
            timestamp_ms,
            packet_index,
            kind: if mid == MSG_CRITICAL_DAMAGE { "critical" } else { "severe" },
            victim_uid: uid(u16_field(msg, &fields, 1)),
            victim_slot: u32_field(msg, &fields, 2).filter(|v| *v != u32::MAX),
            victim_vehicle: str_field(msg, &fields, 3),
            attacker_uid: uid(u16_field(msg, &fields, 4)),
            victim_surface: is_surface(u16_field(msg, &fields, 1)),
            attacker_surface: is_surface(u16_field(msg, &fields, 4)),
        }),
    })
}
