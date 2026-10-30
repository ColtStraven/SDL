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

fn parse_unit(created_ms: u32, e: &EntityCreate) -> Option<Unit> {
    let d = &e.data;
    if d.len() < 32 || d[28] != 0x30 || d[29] != 0x70 {
        return None;
    }
    let f = |o: usize| f32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    let mut strings = prefixed_strings(&d[32..], 3).into_iter();
    Some(Unit {
        uid: u16::from_le_bytes([d[30], d[31]]) & 0x7FF,
        created_ms,
        template: e.template.clone(),
        vehicle: strings.next(),
        mission_slot: strings.next(),
        loadout: strings.next(),
        equipment: strings.collect(),
        spawn_position_m: [f(11), f(15), f(19)],
        controlled_by_author: e.template.contains("controlled_hero"),
    })
}

#[derive(Debug, Default)]
pub struct Registry {
    by_uid: HashMap<u16, Vec<Unit>>,
    pub units: u64,
}

impl Registry {
    /// Record an entity creation if it is an aircraft unit.
    pub fn push(&mut self, t: u32, e: &EntityCreate) {
        if !e.template.starts_with("aircraft") {
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
