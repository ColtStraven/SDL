//! Read-only link to the Strike Master research databases, and proposed
//! findings for review. SDL never writes to these databases.

use crate::evidence::Evidence;
use crate::measure::round3;
use crate::weapons::WeaponEvent;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

pub const AIRCRAFT_DB: &str = "static57-war-thunder-aircraft.json";
pub const WEAPONS_DB: &str = "static57-war-thunder-weapons.json";
pub const THREATS_DB: &str = "static57-war-thunder-threats.json";

#[derive(Debug, Default)]
pub struct Databases {
    /// Wiki slug (the game's internal vehicle name) to permanent aircraft id.
    aircraft_by_slug: HashMap<String, String>,
    /// `(permanent id, designator)`.
    weapons: Vec<(String, String)>,
    pub threats_present: bool,
}

fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

/// Core designator of an internal weapon name: drop a country prefix and
/// `_default`, then join tokens up to the first one containing a digit, for
/// example `us_aim9l_sidewinder_default` -> `aim9l`, `us_agm_65b` -> `agm65b`,
/// `su_r_73e` -> `r73e`.
pub fn weapon_core(name: &str) -> String {
    let mut core = String::new();
    let tokens = name.trim_end_matches("_default").split('_').collect::<Vec<_>>();
    let skip = matches!(tokens[0], "us" | "su" | "uk" | "fr" | "cn" | "jp" | "il" | "swd" | "ger" | "it" | "germ") as usize;
    for t in &tokens[skip..] {
        core.push_str(&normalize(t));
        if t.chars().any(|c| c.is_ascii_digit()) {
            break;
        }
    }
    core
}

/// Designator of a database weapon name: its leading words up to the first
/// containing a digit or followed by a lowercase word, for example
/// `AIM-9L Sidewinder air-to-air missiles` -> `aim9l`, `RB 99 ...` -> `rb99`,
/// `Red Top air-to-air missiles` -> `redtop`.
pub fn database_designator(name: &str) -> String {
    let mut key = String::new();
    let mut words = name.split_whitespace().peekable();
    while let Some(w) = words.next() {
        key.push_str(&normalize(w));
        let next_lower = words.peek().is_none_or(|n| n.starts_with(|c: char| c.is_lowercase()));
        if w.chars().any(|c| c.is_ascii_digit()) || next_lower {
            break;
        }
    }
    key
}

impl Databases {
    /// Load whatever databases exist in `dir`. Files are only read.
    pub fn load(dir: &Path) -> std::io::Result<Databases> {
        let read = |name: &str| -> std::io::Result<serde_json::Value> {
            Ok(serde_json::from_slice(&std::fs::read(dir.join(name))?)?)
        };
        let mut db = Databases { threats_present: dir.join(THREATS_DB).exists(), ..Default::default() };
        for a in read(AIRCRAFT_DB)?["aircraft"].as_array().into_iter().flatten() {
            if let (Some(id), Some(url)) = (a["id"].as_str(), a["source_url"].as_str()) {
                if let Some(slug) = url.rsplit('/').next() {
                    db.aircraft_by_slug.insert(slug.to_string(), id.to_string());
                }
            }
        }
        for w in read(WEAPONS_DB)?["weapons"].as_array().into_iter().flatten() {
            if let (Some(id), Some(name)) = (w["id"].as_str(), w["name"].as_str()) {
                db.weapons.push((id.to_string(), database_designator(name)));
            }
        }
        Ok(db)
    }

    pub fn aircraft_id(&self, vehicle: &str) -> Option<&str> {
        self.aircraft_by_slug.get(vehicle).map(|s| s.as_str())
    }

    /// Permanent weapon id when exactly one database name matches.
    pub fn weapon_id(&self, name: &str) -> Option<&str> {
        let core = weapon_core(name);
        if core.len() < 3 {
            return None;
        }
        let mut hits = self.weapons.iter().filter(|(_, n)| *n == core);
        match (hits.next(), hits.next()) {
            (Some(h), None) => Some(&h.0),
            _ => None,
        }
    }
}

/// One observed shot, proposed for review against the research databases.
#[derive(Debug, Clone, Serialize)]
pub struct Proposal {
    pub proposal_id: String,
    /// Always `proposed_for_review`: nothing is applied automatically.
    pub status: &'static str,
    pub weapon_name: String,
    pub weapon_name_source: &'static str,
    pub weapon_name_evidence: Evidence,
    pub s57_weapon_id: Option<String>,
    pub vehicle: Option<String>,
    pub launcher_evidence: Evidence,
    pub s57_aircraft_id: Option<String>,
    pub database_match_evidence: Evidence,
    pub launch_altitude_m: Option<f64>,
    pub launch_aircraft_speed_mps: Option<f64>,
    pub target_vehicle: Option<String>,
    pub target_method: Option<String>,
    pub target_evidence: Evidence,
    pub target_aspect_deg: Option<f64>,
    pub closing_speed_mps: Option<f64>,
    pub samples: u64,
    pub time_of_flight_s: Option<f64>,
    /// Sample spacing near the end of flight bounds the time of flight.
    pub time_of_flight_uncertainty_s: Option<f64>,
    pub ground_range_m: f64,
    pub max_speed_mps: f64,
    pub end_speed_mps: f64,
    pub distance_to_target_at_last_sample_m: Option<f64>,
    /// Outcome class. `unknown` is not a miss.
    pub outcome: &'static str,
    pub outcome_evidence: Evidence,
    pub target_distance_at_launch_m: Option<f64>,
    pub target_altitude_at_launch_m: Option<f64>,
    pub target_distance_status: &'static str,
    pub replay_sha256: String,
    pub replay_file: String,
    pub weapon_id: String,
    pub first_packet_index: u64,
    pub timestamp_ms: u32,
    /// Launcher verified, weapon named, target geometry present, and a
    /// supporting outcome record. Not a statement of LAR readiness.
    pub complete_record: bool,
    pub caution: &'static str,
}

pub const PROPOSAL_CAUTION: &str = "Single observed shot, not a launch envelope. An unknown outcome is not proof of a miss.";

/// One proposal per named weapon. Unnamed weapons have no database identity.
pub fn proposals(sha256: &str, file: &str, weapons: &[WeaponEvent], db: Option<&Databases>) -> Vec<Proposal> {
    weapons
        .iter()
        .filter_map(|w| {
            let name = w.weapon_name.clone()?;
            let vehicle = w.launcher.vehicle.clone();
            let s57_weapon_id = db.and_then(|d| d.weapon_id(&name)).map(String::from);
            let s57_aircraft_id = db.zip(vehicle.as_deref()).and_then(|(d, v)| d.aircraft_id(v)).map(String::from);
            let t = &w.trajectory;
            let g = w.geometry.as_ref();
            let confirmed = matches!(w.outcome.class, "confirmed_kill" | "confirmed_damage" | "confirmed_hit");
            Some(Proposal {
                launcher_evidence: w.launcher.evidence,
                target_method: w.target.method.clone(),
                target_evidence: w.target.evidence,
                target_aspect_deg: g.and_then(|g| g.target_aspect_deg).map(round3),
                closing_speed_mps: g.and_then(|g| g.closing_speed_mps).map(round3),
                complete_record: w.launcher.evidence == Evidence::Verified && g.is_some() && confirmed,
                proposal_id: format!("{}-{}", &sha256[..8], w.weapon_id),
                status: "proposed_for_review",
                weapon_name: name,
                weapon_name_source: w.weapon_name_source,
                weapon_name_evidence: w.weapon_name_evidence,
                database_match_evidence: if s57_weapon_id.is_some() { Evidence::Experimental } else { Evidence::Unknown },
                s57_weapon_id,
                vehicle,
                s57_aircraft_id,
                launch_altitude_m: t.launch_altitude_m.map(round3),
                launch_aircraft_speed_mps: w.launcher.aircraft_speed_mps.map(round3),
                target_vehicle: w.target.vehicle.clone(),
                samples: t.samples,
                time_of_flight_s: t.time_of_flight_s.map(round3),
                time_of_flight_uncertainty_s: t.max_sample_interval_s.map(round3),
                ground_range_m: round3(t.ground_range_m),
                max_speed_mps: round3(t.max_speed_mps),
                end_speed_mps: round3(t.last_speed_mps),
                distance_to_target_at_last_sample_m: w.outcome.distance_to_victim_m.map(round3),
                outcome: w.outcome.class,
                outcome_evidence: w.outcome.evidence,
                target_distance_at_launch_m: g.map(|g| round3(g.target_distance_m)),
                target_altitude_at_launch_m: g.map(|g| round3(g.target_altitude_m)),
                target_distance_status: if g.is_some() { "derived: launch point and established target at launch time" } else { "unavailable: no tracked target established" },
                replay_sha256: sha256.to_string(),
                replay_file: file.to_string(),
                weapon_id: w.weapon_id.clone(),
                first_packet_index: w.first_packet_index,
                timestamp_ms: t.first_ms,
                caution: PROPOSAL_CAUTION,
            })
        })
        .collect()
}
