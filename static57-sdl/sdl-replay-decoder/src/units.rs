//! Unit identity from ECS creation records.
//!
//! Each aircraft unit entity carries, at fixed offsets in its creation data:
//! a spawn position (three f32 at byte 11) and the marker `30 70` at byte 28
//! followed by the 11-bit unit uid (u16le at byte 30). Vehicle class, mission
//! slot and loadout follow as length-prefixed strings. Component decoding is
//! not implemented: these offsets are SDL observations, validated against the
//! flight sync (spawn position) and the battle log (vehicle names).
//!
//! Uids are reused, so a vehicle is resolved at a time: the newest creation
//! at or before that time, allowing for syncs that slightly precede their
//! entity.

use crate::ecs::EntityCreate;
use serde::Serialize;
use std::collections::HashMap;

/// Pre-spawn placeholder holding a player slot; it never flies.
pub const PLACEHOLDER_VEHICLE: &str = "dummy_plane";

/// A sync may precede its entity's creation by up to this long.
pub const EARLY_SYNC_MS: u32 = 300;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Unit {
    /// Raw entity id of this unit.
    pub eid: u32,
    pub uid: u16,
    pub created_ms: u32,
    pub template: String,
    pub vehicle: Option<String>,
    pub mission_slot: Option<String>,
    pub loadout: Option<String>,
    /// Remaining length-prefixed strings: weapon and module names.
    pub equipment: Vec<String>,
    pub spawn_position_m: [f32; 3],
    /// Template marks the unit controlled by the recording client.
    pub controlled_by_author: bool,
}

/// Length-prefixed identifier strings: a byte `n` followed by `n` identifier
/// characters. Used only as a scanner over uninterpreted component data.
pub fn prefixed_strings(data: &[u8], min_len: usize) -> Vec<String> {
    let ident = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/' | b'.' | b'!');
    let at = |i: usize, min: usize| -> Option<usize> {
        let n = *data.get(i)? as usize;
        (n >= min && i + 1 + n <= data.len() && data[i + 1..i + 1 + n].iter().all(|b| ident(*b))).then_some(n)
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        // Two-character names (vehicle `f1`) are accepted only directly
        // before another string, which random bytes rarely imitate.
        let n = at(i, min_len).or_else(|| at(i, 2).filter(|n| at(i + 1 + n, min_len).is_some()));
        match n {
            Some(n) => {
                out.push(String::from_utf8_lossy(&data[i + 1..i + 1 + n]).into_owned());
                i += 1 + n;
            }
            None => i += 1,
        }
    }
    out
}

/// Equipment strings that can be weapons: country-prefixed store names.
/// Countermeasures, guns, belts and modifications are left out.
pub fn weapon_equipment(equipment: &[String]) -> Vec<String> {
    const PREFIXES: [&str; 13] = ["us_", "su_", "uk_", "fr_", "cn_", "jp_", "il_", "swd_", "swe_", "ger_", "germ_", "it_", "za_"];
    equipment
        .iter()
        .filter(|s| PREFIXES.iter().any(|p| s.starts_with(p)))
        .filter(|s| !["_belt", "_ltc_", "countermeasure", "_air_targets", "_gun", "_pod_gun"].iter().any(|x| s.contains(x)))
        .cloned()
        .collect()
}

/// Offset of the uid marker: `30 70` on aircraft, `38 70` on ground units.
/// Normally byte 28; some records carry extra leading fields.
fn uid_marker(d: &[u8]) -> Option<usize> {
    (24..48.min(d.len().saturating_sub(3))).find(|&o| d[o + 1] == 0x70 && d[o] & 0xF7 == 0x30)
}

fn parse_unit(created_ms: u32, e: &EntityCreate) -> Option<Unit> {
    let d = &e.data;
    let m = uid_marker(d)?;
    if m < 17 {
        return None;
    }
    let f = |o: usize| f32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    let mut strings = prefixed_strings(&d[m + 4..], 3).into_iter();
    Some(Unit {
        eid: e.eid,
        uid: u16::from_le_bytes([d[m + 2], d[m + 3]]) & 0x7FF,
        created_ms,
        template: e.template.clone(),
        vehicle: strings.next(),
        mission_slot: strings.next(),
        loadout: strings.next(),
        equipment: strings.collect(),
        spawn_position_m: [f(m - 17), f(m - 13), f(m - 9)],
        controlled_by_author: e.template.contains("controlled_hero"),
    })
}

/// Any unit-like entity, keyed by raw entity id. Launchers can be aircraft,
/// ground units or ships, so this is wider than the aircraft registry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntityInfo {
    pub eid: u32,
    pub created_ms: u32,
    /// `aircraft`, `ground` or `ship`.
    pub kind: &'static str,
    pub template: String,
    pub vehicle: Option<String>,
    pub mission_slot: Option<String>,
    pub loadout: Option<String>,
    pub uid: Option<u16>,
    pub equipment: Vec<String>,
    pub spawn_position_m: Option<[f32; 3]>,
}

fn entity_kind(template: &str) -> Option<&'static str> {
    let base = template.split('+').next().unwrap_or("");
    match base {
        "aircraft" => Some("aircraft"),
        "tank" | "simple_tank" | "fortification" => Some("ground"),
        "ship" => Some("ship"),
        _ => None,
    }
}

fn clean_vehicle(s: String) -> String {
    s.trim_start_matches("tankModels/").trim_start_matches("ships/").trim_start_matches("structures/").to_string()
}

#[derive(Debug, Default)]
pub struct Registry {
    by_uid: HashMap<u16, Vec<Unit>>,
    by_eid: HashMap<u32, Vec<EntityInfo>>,
    pub units: u64,
}

impl Registry {
    /// The entity with raw id `eid` at time `t`. Ids are reused only with a
    /// new generation, but a replay can recreate an entity, so time matters.
    pub fn entity(&self, eid: u32, t: u32) -> Option<&EntityInfo> {
        self.by_eid.get(&eid)?.iter().rfind(|u| u.created_ms <= t)
    }

    /// The ground or ship unit holding `uid` at time `t`.
    pub fn surface_unit(&self, uid: u16, t: u32) -> Option<&EntityInfo> {
        self.by_eid.values().flatten().filter(|u| u.kind != "aircraft" && u.uid == Some(uid) && u.created_ms <= t).max_by_key(|u| u.created_ms)
    }

    /// Record an entity creation if it is a unit.
    pub fn push(&mut self, t: u32, e: &EntityCreate) {
        let Some(kind) = entity_kind(&e.template) else { return };
        let marker = uid_marker(&e.data);
        let strings = prefixed_strings(e.data.get(marker.map_or(32, |m| m + 4)..).unwrap_or(&[]), 3);
        // Marker `30 70` on aircraft and `38 70` on ground units, then the uid.
        let parsed = marker.map(|m| u16::from_le_bytes([e.data[m + 2], e.data[m + 3]]) & 0x7FF);
        let spawn = marker.filter(|m| *m >= 17).map(|m| [m - 17, m - 13, m - 9].map(|o| f32::from_le_bytes(e.data[o..o + 4].try_into().unwrap())));
        let vehicle = strings.first().cloned().map(clean_vehicle);
        if vehicle.as_deref() != Some(PLACEHOLDER_VEHICLE) {
            self.by_eid.entry(e.eid).or_default().push(EntityInfo {
                eid: e.eid,
                created_ms: t,
                kind,
                template: e.template.clone(),
                vehicle,
                mission_slot: strings.get(1).cloned(),
                loadout: strings.get(2).cloned(),
                uid: parsed,
                equipment: strings.into_iter().skip(3).collect(),
                spawn_position_m: spawn,
            });
        }
        if kind != "aircraft" {
            return;
        }
        if let Some(u) = parse_unit(t, e).filter(|u| u.vehicle.as_deref() != Some(PLACEHOLDER_VEHICLE)) {
            self.units += 1;
            self.by_uid.entry(u.uid).or_default().push(u);
        }
    }

    /// The unit holding `uid` at time `t`.
    pub fn resolve(&self, uid: u16, t: u32) -> Option<&Unit> {
        self.by_uid.get(&uid)?.iter().rfind(|u| u.created_ms <= t.saturating_add(EARLY_SYNC_MS))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Unit> {
        self.by_uid.values().flatten()
    }
}
