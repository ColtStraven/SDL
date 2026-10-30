//! Per-replay scientific research dataset.
//!
//! Everything here is measured from decoded structures. Each value carries
//! its evidence class, and unsupported measurements stay `null`. The Markdown
//! report is rendered from this dataset alone (`report.rs`).

use crate::combat::{DamageEvent, KillEvent};
use crate::evidence::Evidence;
use crate::lab::{self, Extraction};
use crate::manifest::{Options, DECODER_NAME, DECODER_VERSION};
use crate::measure::{self, round3, At, Gap, Maneuver, Pt, Stat, Transition};
use crate::strikemaster::{self, Databases, Proposal};
use crate::tracks::{CockpitValidation, Coverage, KinematicValidation, TrackConfig, TrackSummary};
use crate::units::Registry;
use crate::weapons::{WeaponEvent, WeaponSample, WeaponStats};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io;

pub const RESEARCH_SCHEMA: &str = "static57.sdl.research-data/0.2";
/// Intervals inside a track longer than this count as missing data.
pub const MISSING_MIN_MS: u32 = 1_000;
/// Spawn position agreement that supports a uid-to-vehicle link.
pub const SPAWN_MATCH_M: f64 = 1_000.0;

#[derive(Debug, Clone, Serialize)]
pub struct ReplayInfo {
    pub file_name: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub status: &'static str,
    pub format_magic: String,
    pub game_version: String,
    pub game_version_evidence: Evidence,
    pub session_id_hex: Option<String>,
    pub start_time_utc: Option<String>,
    pub mission: Option<String>,
    pub level: Option<String>,
    pub environment: Option<String>,
    pub weather: Option<String>,
    pub recording: Option<&'static str>,
    pub timeline_first_ms: Option<u32>,
    pub timeline_last_ms: Option<u32>,
    pub timeline_span_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Identity {
    pub vehicle: Option<String>,
    pub mission_slot: Option<String>,
    pub loadout: Option<String>,
    /// `battle_log_confirmed`, `battle_log_only`, `spawn_match`,
    /// `registry_only`, `conflict`, or `unresolved`.
    pub confidence: &'static str,
    pub evidence: Evidence,
    pub spawn_distance_m: Option<f64>,
    pub battle_log_agreements: u32,
    pub battle_log_conflicts: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Destruction {
    pub timestamp_ms: u32,
    pub packet_index: u64,
    pub killer_uid: Option<u16>,
    pub killer_vehicle: Option<String>,
    pub weapon: Option<String>,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackResearch {
    pub track_id: String,
    pub uid: u16,
    pub segment: u32,
    pub role: &'static str,
    pub identity: Identity,
    pub start_reason: &'static str,
    pub start_ms: u32,
    pub end_ms: u32,
    pub duration_s: f64,
    pub samples: u64,
    /// Samples divided by the nominal 4 Hz count over the duration.
    pub sample_coverage: Option<f64>,
    pub withheld_records: u64,
    pub flagged_position_jumps: u64,
    pub saturated_speed_samples: u64,
    pub ground_flag_fraction: Option<f64>,
    pub path_length_m: f64,
    pub altitude_m: Stat,
    pub speed_mps: Stat,
    pub groundspeed_mps: Stat,
    pub vertical_speed_mps: Stat,
    /// Present for the recording author; remote aircraft are network copies.
    pub maneuver: Option<Maneuver>,
    pub transitions: Vec<Transition>,
    pub transition_count: u64,
    pub internal_gaps_over_1s: u64,
    pub largest_internal_gap_ms: u32,
    pub end_altitude_m: Option<f64>,
    pub end_speed_mps: Option<f64>,
    pub destruction: Option<Destruction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Anomaly {
    pub kind: &'static str,
    pub count: u64,
    pub evidence: Evidence,
    pub note: String,
    pub examples: Vec<SourceRef>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceRef {
    pub subject: String,
    pub packet_index: Option<u64>,
    pub timestamp_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissingInterval {
    pub track_id: String,
    pub role: &'static str,
    #[serde(flatten)]
    pub gap: Gap,
    pub cause: &'static str,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MissingData {
    pub intervals_total: u64,
    pub missing_seconds_total: f64,
    /// Author intervals first, then the longest others.
    pub intervals: Vec<MissingInterval>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: String,
    pub statement: String,
    pub value: Option<f64>,
    pub unit: Option<&'static str>,
    pub evidence: Evidence,
    pub method: String,
    pub sources: Vec<SourceRef>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldCoverage {
    pub family: &'static str,
    /// `decoded`, `partial`, `raw_only`, or `not_decoded`.
    pub status: &'static str,
    pub evidence: Evidence,
    pub note: &'static str,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct IdentitySummary {
    pub aircraft_units_seen: u64,
    pub tracks_by_confidence: BTreeMap<&'static str, u64>,
    pub kill_records_vehicle_match: u64,
    pub kill_records_vehicle_mismatch: u64,
    pub damage_records_vehicle_match: u64,
    pub damage_records_vehicle_mismatch: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorSummary {
    pub uids: Vec<u16>,
    pub vehicles: Vec<String>,
    pub track_ids: Vec<String>,
    pub samples: u64,
    /// Tracked time, including any time parked.
    pub tracked_seconds: f64,
    pub max_altitude_m: Option<f64>,
    pub max_speed_mps: Option<f64>,
    pub speed_saturated: bool,
    pub max_load_factor_g: Option<f64>,
    pub max_climb_10s_mps: Option<f64>,
    pub max_turn_rate_deg_s: Option<f64>,
    pub max_specific_excess_power_mps: Option<f64>,
    pub weapons_released: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CombatSummary {
    pub kills_seen: u64,
    pub damage_records: u64,
    pub kills_by_weapon: BTreeMap<String, u64>,
    pub author_kills: Vec<KillEvent>,
    pub author_deaths: Vec<KillEvent>,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeaponsSection {
    pub stats: WeaponStats,
    pub by_template: BTreeMap<String, u64>,
    pub events: Vec<WeaponEvent>,
    pub limitations: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StrikeMasterSection {
    pub databases_loaded: bool,
    pub database_note: &'static str,
    pub proposals: Vec<Proposal>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecoderInfo {
    pub name: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResearchData {
    pub schema: &'static str,
    pub decoder: DecoderInfo,
    pub replay: ReplayInfo,
    pub decoded_field_coverage: Vec<FieldCoverage>,
    pub flight_coverage: Coverage,
    pub kinematic_validation: KinematicValidation,
    pub cockpit_validation: CockpitValidation,
    pub author: Option<AuthorSummary>,
    pub identity: IdentitySummary,
    pub tracks: Vec<TrackResearch>,
    pub anomalies: Vec<Anomaly>,
    pub missing_data: MissingData,
    pub combat: CombatSummary,
    pub weapons: WeaponsSection,
    pub strike_master: StrikeMasterSection,
    pub findings: Vec<Finding>,
    pub unresolved_questions: Vec<String>,
}

pub struct ResearchRun {
    pub data: ResearchData,
    pub weapon_samples: Vec<(String, Vec<WeaponSample>)>,
}

/// Static inventory of what this decoder version can and cannot extract.
pub fn field_coverage() -> Vec<FieldCoverage> {
    use Evidence::*;
    let f = |family, status, evidence, note| FieldCoverage { family, status, evidence, note };
    vec![
        f("aircraft_position_altitude", "decoded", Verified, "World metres, +Y up. Y matches cockpit altimeters."),
        f("aircraft_orientation", "decoded", Verified, "Heading, pitch, bank. Checked against cockpit and flight path."),
        f("aircraft_velocity", "decoded", Verified, "Saturates at 500 m/s (value is then a lower bound)."),
        f("aircraft_identity", "partial", Derived, "Vehicle, slot and loadout from ECS creation, checked against spawn position and battle log. Not every uid resolves."),
        f("aircraft_controls", "raw_only", Reported, "Seven control bytes exported raw in flight_tracks.jsonl; layout reported, not validated."),
        f("engine_state", "raw_only", Reported, "State byte exported; meanings reported, not validated. Power and health fields not scaled."),
        f("weapon_trajectory", "decoded", Verified, "MPI 0xF0DB position, velocity, time since launch. All messages consume their declared bit length."),
        f("weapon_launch", "partial", Derived, "Launch position from ECS creation; launch time from timer. Launcher by proximity or kill record."),
        f("weapon_identity", "partial", Derived, "Kill records name some weapons; the per-replay type id carries a name to others of that type (98.8% in hold-out). Types with no kill stay unnamed."),
        f("weapon_outcome", "partial", Derived, "Kill linkage by geometry and killer. All other outcomes unknown; last sample is not an established impact."),
        f("battle_log", "partial", Verified, "Kills and damage: uids, killer vehicle, weapon name. Other fields not decoded."),
        f("radar_sensors", "not_decoded", Unknown, "Sensor records delimited in type 2, values not interpreted."),
        f("missile_seeker", "not_decoded", Unknown, "Seeker blocks delimited and skipped, values not interpreted."),
        f("countermeasures", "not_decoded", Unknown, "Records delimited in type 2, values not interpreted."),
        f("ground_units_air_defense", "not_decoded", Unknown, "MPI 0xF073 ground positions not decoded. SAM launches appear only as weapons with no launcher."),
        f("localhost_8111_fusion", "not_decoded", Unknown, "No synchronized recording processed."),
    ]
}

fn at_ref(subject: &str, a: Option<At>) -> SourceRef {
    SourceRef { subject: subject.to_string(), packet_index: a.map(|x| x.packet_index), timestamp_ms: a.map(|x| x.timestamp_ms) }
}

fn identify(t: &TrackSummary, first: Option<&Pt>, reg: &Registry, kills: &[KillEvent], damage: &[DamageEvent]) -> Identity {
    let u = reg.resolve(t.uid, t.start_ms);
    let spawn_distance_m = match (u, first) {
        (Some(u), Some(p)) if u.created_ms.abs_diff(t.start_ms) <= 30_000 => {
            let s = u.spawn_position_m;
            Some(round3((0..3).map(|k| ((s[k] - p.pos[k]) as f64).powi(2)).sum::<f64>().sqrt()))
        }
        _ => None,
    };
    let in_span = |ts: u32| ts >= t.start_ms && ts <= t.end_ms.saturating_add(5_000);
    let log: Vec<&str> = kills
        .iter()
        .filter(|k| k.killer_uid == Some(t.uid) && in_span(k.timestamp_ms))
        .filter_map(|k| k.killer_vehicle.as_deref())
        .chain(damage.iter().filter(|d| d.attacker_uid == Some(t.uid) && in_span(d.timestamp_ms)).filter_map(|d| d.victim_vehicle.as_deref()))
        .collect();
    let reg_vehicle = u.and_then(|u| u.vehicle.clone());
    let agree = log.iter().filter(|v| reg_vehicle.as_deref() == Some(**v)).count() as u32;
    let conflict = if reg_vehicle.is_some() { log.len() as u32 - agree } else { 0 };
    let (vehicle, confidence, evidence) = if agree > 0 && conflict == 0 {
        (reg_vehicle, "battle_log_confirmed", Evidence::Verified)
    } else if conflict > 0 {
        (log.first().map(|s| s.to_string()), "conflict", Evidence::Experimental)
    } else if let Some(v) = log.first() {
        (Some(v.to_string()), "battle_log_only", Evidence::Derived)
    } else if reg_vehicle.is_some() && spawn_distance_m.is_some_and(|d| d <= SPAWN_MATCH_M) {
        (reg_vehicle, "spawn_match", Evidence::Derived)
    } else if reg_vehicle.is_some() {
        (reg_vehicle, "registry_only", Evidence::Experimental)
    } else {
        (None, "unresolved", Evidence::Unknown)
    };
    Identity {
        vehicle,
        mission_slot: u.and_then(|u| u.mission_slot.clone()),
        loadout: u.and_then(|u| u.loadout.clone()),
        confidence,
        evidence,
        spawn_distance_m,
        battle_log_agreements: agree,
        battle_log_conflicts: conflict,
    }
}

fn track_research(t: &TrackSummary, pts: &[Pt], x: &Extraction) -> TrackResearch {
    let author = t.author_samples * 2 >= t.samples && t.samples > 0;
    let duration_s = (t.end_ms - t.start_ms) as f64 / 1000.0;
    let gaps = measure::gaps(pts, measure::MANEUVER_BASELINE_MS);
    let trs = measure::transitions(pts);
    let destruction = x
        .kills
        .iter()
        .filter(|k| k.victim_uid == Some(t.uid) && k.timestamp_ms + 3_000 >= t.end_ms && k.timestamp_ms <= t.end_ms + 8_000)
        .min_by_key(|k| k.timestamp_ms.abs_diff(t.end_ms))
        .map(|k| Destruction {
            timestamp_ms: k.timestamp_ms,
            packet_index: k.packet_index,
            killer_uid: k.killer_uid,
            killer_vehicle: k.killer_vehicle.clone(),
            weapon: k.weapon.clone(),
            evidence: Evidence::Derived,
        });
    let last = pts.last();
    TrackResearch {
        track_id: t.track_id.clone(),
        uid: t.uid,
        segment: t.segment,
        role: if author { "author" } else { "remote" },
        identity: identify(t, pts.first(), &x.registry, &x.kills, &x.damage),
        start_reason: t.start_reason,
        start_ms: t.start_ms,
        end_ms: t.end_ms,
        duration_s: round3(duration_s),
        samples: t.samples,
        sample_coverage: (duration_s > 0.0).then(|| round3((t.samples as f64 / (duration_s * 4.0)).min(1.0))),
        withheld_records: t.withheld_records,
        flagged_position_jumps: t.flagged_jumps,
        saturated_speed_samples: t.saturated_speed_samples,
        ground_flag_fraction: (!pts.is_empty()).then(|| round3(pts.iter().filter(|p| p.ground_flag).count() as f64 / pts.len() as f64)),
        path_length_m: round3(t.path_length_m),
        altitude_m: measure::field_stat(pts, measure::altitude),
        speed_mps: measure::field_stat(pts, |p| p.speed as f64),
        groundspeed_mps: measure::field_stat(pts, |p| (p.vel[0] as f64).hypot(p.vel[2] as f64)),
        vertical_speed_mps: measure::field_stat(pts, |p| p.vel[1] as f64),
        maneuver: author.then(|| measure::maneuver(pts)),
        transition_count: trs.len() as u64,
        transitions: if author { trs } else { Vec::new() },
        internal_gaps_over_1s: gaps.len() as u64,
        largest_internal_gap_ms: gaps.iter().map(|g| g.duration_ms).max().unwrap_or(0),
        end_altitude_m: last.map(|p| round3(measure::altitude(p))),
        end_speed_mps: last.map(|p| round3(p.speed as f64)),
        destruction,
    }
}

fn anomalies(tracks: &[TrackResearch], points: &BTreeMap<String, Vec<Pt>>, flight: &crate::tracks::FlightReport) -> Vec<Anomaly> {
    let mut out = Vec::new();
    let mut push = |kind, count: u64, evidence, note: String, examples: Vec<SourceRef>| {
        if count > 0 {
            out.push(Anomaly { kind, count, evidence, note, examples });
        }
    };
    let jump_examples = |want_author: bool| -> Vec<SourceRef> {
        tracks
            .iter()
            .filter(|t| (t.role == "author") == want_author)
            .flat_map(|t| points.get(&t.track_id).into_iter().flatten().filter(|p| p.jump).map(move |p| (t, p)))
            .take(3)
            .map(|(t, p)| SourceRef { subject: t.track_id.clone(), packet_index: Some(p.packet), timestamp_ms: Some(p.t) })
            .collect()
    };
    let (mut remote_jumps, mut author_jumps, mut sat) = (0, 0, 0);
    for t in tracks {
        if t.role == "author" {
            author_jumps += t.flagged_position_jumps;
        } else {
            remote_jumps += t.flagged_position_jumps;
        }
        sat += t.saturated_speed_samples;
    }
    push(
        "remote_position_jump",
        remote_jumps,
        Evidence::Derived,
        "Remote aircraft moved 200 m to 2 km beyond velocity x time between syncs. Treated as network correction; segment not split.".into(),
        jump_examples(false),
    );
    push(
        "author_position_jump",
        author_jumps,
        Evidence::Derived,
        "The recording author jumped 200 m to 2 km between syncs. Unusual: inspect before using this interval.".into(),
        jump_examples(true),
    );
    push("segment_split_by_jump", flight.segment_starts.get("position_jump").copied().unwrap_or(0), Evidence::Derived, "Unexplained jump over 2 km started a new segment: possible respawn or uid reuse.".into(), vec![]);
    push("segment_split_by_gap", flight.segment_starts.get("gap").copied().unwrap_or(0), Evidence::Derived, "Gap over 5 s started a new segment. Continuity across the gap is unproven.".into(), vec![]);
    let abrupt: Vec<(&TrackResearch, At)> = tracks
        .iter()
        .filter_map(|t| t.maneuver.as_ref().map(|m| (t, m)))
        .flat_map(|(t, m)| m.abrupt_examples.iter().map(move |a| (t, *a)))
        .collect();
    push(
        "abrupt_velocity_change",
        tracks.iter().filter_map(|t| t.maneuver.as_ref()).map(|m| m.abrupt_windows).sum(),
        Evidence::Derived,
        "Author speed changed over 60 m/s per second: collision, break-up or ground impact candidate. Excluded from maneuver statistics.".into(),
        abrupt.iter().take(3).map(|(t, a)| at_ref(&t.track_id, Some(*a))).collect(),
    );
    push(
        "speed_saturation",
        sat,
        Evidence::Verified,
        "Speed field at its 500 m/s ceiling: the value is a lower bound, and maneuver figures skip these samples.".into(),
        tracks
            .iter()
            .filter(|t| t.saturated_speed_samples > 0)
            .take(3)
            .map(|t| at_ref(&t.track_id, t.speed_mps.max_at))
            .collect(),
    );
    out
}

fn missing_data(tracks: &[TrackResearch], points: &BTreeMap<String, Vec<Pt>>) -> MissingData {
    let mut all = Vec::new();
    for t in tracks {
        for g in measure::gaps(points.get(&t.track_id).map(|v| v.as_slice()).unwrap_or(&[]), MISSING_MIN_MS) {
            all.push(MissingInterval { track_id: t.track_id.clone(), role: t.role, gap: g, cause: "no state record" });
        }
    }
    let total = all.iter().map(|i| i.gap.duration_ms as f64 / 1000.0).sum::<f64>();
    let count = all.len() as u64;
    all.sort_by_key(|i| (i.role != "author", std::cmp::Reverse(i.gap.duration_ms)));
    all.truncate(25);
    MissingData { intervals_total: count, missing_seconds_total: round3(total), intervals: all }
}

fn author_summary(tracks: &[TrackResearch], weapons: &[WeaponEvent], uids: &[u16]) -> Option<AuthorSummary> {
    let a: Vec<&TrackResearch> = tracks.iter().filter(|t| t.role == "author").collect();
    if a.is_empty() {
        return None;
    }
    let max_of = |f: &dyn Fn(&TrackResearch) -> Option<f64>| a.iter().filter_map(|t| f(t)).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
    let mut vehicles: Vec<String> = a.iter().filter_map(|t| t.identity.vehicle.clone()).collect();
    vehicles.sort();
    vehicles.dedup();
    let man = |f: &dyn Fn(&Maneuver) -> Option<f64>| max_of(&|t| t.maneuver.as_ref().and_then(|m| f(m)));
    Some(AuthorSummary {
        uids: uids.to_vec(),
        vehicles,
        track_ids: a.iter().map(|t| t.track_id.clone()).collect(),
        samples: a.iter().map(|t| t.samples).sum(),
        tracked_seconds: round3(a.iter().map(|t| t.duration_s).sum()),
        max_altitude_m: max_of(&|t| t.altitude_m.max),
        max_speed_mps: max_of(&|t| t.speed_mps.max),
        speed_saturated: a.iter().any(|t| t.saturated_speed_samples > 0),
        max_load_factor_g: man(&|m| m.load_factor_g.max),
        max_climb_10s_mps: man(&|m| m.climb_rate_10s_mps.max),
        max_turn_rate_deg_s: man(&|m| m.turn_rate_deg_s.max),
        max_specific_excess_power_mps: man(&|m| m.specific_excess_power_5s_mps.max),
        weapons_released: weapons.iter().filter(|w| w.launcher.is_author).count() as u64,
    })
}

fn findings(d: &ResearchData) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    let mut add = |statement: String, value: Option<f64>, unit: Option<&'static str>, evidence, method: &str, sources: Vec<SourceRef>| {
        let id = format!("F-{:03}", out.len() + 1);
        out.push(Finding { id, statement, value, unit, evidence, method: method.to_string(), sources });
    };
    let author_tracks: Vec<&TrackResearch> = d.tracks.iter().filter(|t| t.role == "author").collect();
    if let Some(a) = &d.author {
        let vehicle = if a.vehicles.is_empty() { "unresolved vehicle".to_string() } else { a.vehicles.join(", ") };
        let best = |f: &dyn Fn(&TrackResearch) -> &Stat| -> Option<(&TrackResearch, f64, Option<At>)> {
            author_tracks
                .iter()
                .filter_map(|t| f(t).max.map(|v| (*t, v, f(t).max_at)))
                .max_by(|x, y| x.1.partial_cmp(&y.1).unwrap())
        };
        if let Some((t, v, at)) = best(&|t| &t.altitude_m) {
            add(format!("Author ({vehicle}) reached {v} m altitude."), Some(v), Some("m"), Evidence::Verified, "Maximum decoded position Y over author tracks.", vec![at_ref(&t.track_id, at)]);
        }
        if let Some((t, v, at)) = best(&|t| &t.speed_mps) {
            let (ev, note) = if t.saturated_speed_samples > 0 { (Evidence::Derived, " (field saturated: true speed was at or above this)") } else { (Evidence::Verified, "") };
            add(format!("Author ({vehicle}) reached {v} m/s true speed{note}."), Some(v), Some("m/s"), ev, "Maximum decoded speed over author tracks.", vec![at_ref(&t.track_id, at)]);
        }
        let mbest = |f: &dyn Fn(&Maneuver) -> &Stat| -> Option<(&TrackResearch, f64, Option<At>)> {
            author_tracks
                .iter()
                .filter_map(|t| t.maneuver.as_ref().and_then(|m| f(m).max.map(|v| (*t, v, f(m).max_at))))
                .max_by(|x, y| x.1.partial_cmp(&y.1).unwrap())
        };
        if let Some((t, v, at)) = mbest(&|m| &m.load_factor_g) {
            add(format!("Author peak load factor {v} g over a 1 s baseline."), Some(v), Some("g"), Evidence::Derived, "Velocity difference over 1 s plus gravity. Shorter peaks are attenuated.", vec![at_ref(&t.track_id, at)]);
        }
        if let Some((t, v, at)) = mbest(&|m| &m.climb_rate_10s_mps) {
            add(format!("Author best sustained climb {v} m/s over 10 s."), Some(v), Some("m/s"), Evidence::Derived, "Altitude change over a 10 s baseline. Not a controlled climb test.", vec![at_ref(&t.track_id, at)]);
        }
        if let Some((t, v, at)) = mbest(&|m| &m.specific_excess_power_5s_mps) {
            add(format!("Author peak specific excess power {v} m/s over 5 s."), Some(v), Some("m/s"), Evidence::Derived, "Rate of change of energy height. Aircraft configuration is unknown.", vec![at_ref(&t.track_id, at)]);
        }
    }
    let cv = &d.cockpit_validation;
    if let (Some(p50), Some(id), Some(unit)) = (cv.altitude_m.abs_error.p50, cv.altimeter_param_id, cv.altimeter_unit) {
        add(
            format!("Decoded author altitude agrees with cockpit parameter {id} ({unit}) to a median of {} m.", round3(p50)),
            Some(round3(p50)),
            Some("m"),
            Evidence::Verified,
            "Author state interpolated to each 0xD04A timestamp.",
            vec![],
        );
    }
    for w in d.weapons.events.iter().filter(|w| w.launcher.is_author && w.outcome.status == "kill_linked") {
        add(
            format!(
                "Author weapon {} ({}) linked to a kill of {} after {} s.",
                w.weapon_id,
                w.weapon_name.as_deref().unwrap_or("unnamed"),
                w.outcome.victim_vehicle.as_deref().unwrap_or("unresolved target"),
                w.trajectory.time_of_flight_s.map(|v| round3(v).to_string()).unwrap_or_else(|| "unknown".into()),
            ),
            w.trajectory.time_of_flight_s.map(round3),
            Some("s"),
            Evidence::Derived,
            "Weapon track end matched to a kill record by time, victim distance and killer.",
            vec![SourceRef { subject: w.weapon_id.clone(), packet_index: Some(w.first_packet_index), timestamp_ms: Some(w.trajectory.first_ms) }],
        );
    }
    out
}

fn questions(d: &ResearchData) -> Vec<String> {
    let mut q = Vec::new();
    if d.identity.tracks_by_confidence.get("unresolved").copied().unwrap_or(0) > 0 {
        q.push("Which vehicles fly the unresolved uids? Needs ECS component decoding or more battle-log evidence.".to_string());
    }
    let unnamed = d.weapons.events.iter().filter(|w| w.weapon_name.is_none()).count();
    if unnamed > 0 {
        q.push(format!("{unnamed} weapons have no name: their type id has no kill record in this replay. Pooling replays cannot help, since ids are per replay."));
    }
    let unknown = d.weapons.events.iter().filter(|w| w.outcome.status == "unknown").count();
    if unknown > 0 {
        q.push(format!("{unknown} weapons have an unknown outcome. Is the last sample an impact, a self-destruct, or loss of replication?"));
    }
    if d.cockpit_validation.cockpit_messages == 0 {
        q.push("This replay has no cockpit messages, so no independent instrument check was possible.".to_string());
    }
    q.push("How do world X/Z relate to map north? Needs a synchronized 8111 recording.".to_string());
    q.push("Can the target of a weapon be established from sensor or seeker records? They are delimited but not decoded.".to_string());
    q
}

/// Run the full research pass on one replay.
pub fn run(file_name: &str, file: &[u8], db: Option<&Databases>) -> io::Result<ResearchRun> {
    let mut points: BTreeMap<String, Vec<Pt>> = BTreeMap::new();
    let x = lab::extract(file_name, file, Options::default(), TrackConfig::default(), |s| {
        points.entry(s.track_id.clone()).or_default().push(Pt::from_sample(s));
    })?;
    let tracks: Vec<TrackResearch> = x
        .flight
        .track_summaries
        .iter()
        .map(|t| track_research(t, points.get(&t.track_id).map(|v| v.as_slice()).unwrap_or(&[]), &x))
        .collect();

    let m = &x.manifest;
    let s = m.session.as_ref();
    let clock = m.packets.as_ref().map(|p| &p.clock);
    let replay = ReplayInfo {
        file_name: m.input.file_name.clone(),
        sha256: m.input.sha256.clone(),
        size_bytes: m.input.size,
        status: m.status,
        format_magic: m.format.as_ref().map(|f| f.format_magic_hex.clone()).unwrap_or_default(),
        game_version: s.map(|s| s.game_version.value.clone()).unwrap_or_default(),
        game_version_evidence: Evidence::Experimental,
        session_id_hex: s.map(|s| s.session_id_hex.clone()),
        start_time_utc: s.map(|s| s.start_time_utc.value.clone()),
        mission: s.map(|s| s.mission_name.value.clone()),
        level: s.map(|s| s.level_path.value.clone()),
        environment: s.map(|s| s.environment.value.clone()),
        weather: s.map(|s| s.weather.value.clone()),
        recording: s.map(|s| s.recording.kind),
        timeline_first_ms: clock.and_then(|c| c.first_timestamp_ms),
        timeline_last_ms: clock.and_then(|c| c.last_timestamp_ms),
        timeline_span_s: clock.and_then(|c| c.span_ms).map(|v| round3(v as f64 / 1000.0)),
    };

    let mut identity = IdentitySummary { aircraft_units_seen: x.registry.units, ..Default::default() };
    for t in &tracks {
        *identity.tracks_by_confidence.entry(t.identity.confidence).or_default() += 1;
    }
    for k in &x.kills {
        if let (Some(u), Some(v)) = (k.killer_uid.and_then(|u| x.registry.resolve(u, k.timestamp_ms)), &k.killer_vehicle) {
            if u.vehicle.as_deref() == Some(v.as_str()) {
                identity.kill_records_vehicle_match += 1;
            } else {
                identity.kill_records_vehicle_mismatch += 1;
            }
        }
    }
    for d in &x.damage {
        if let (Some(u), Some(v)) = (d.attacker_uid.and_then(|u| x.registry.resolve(u, d.timestamp_ms)), &d.victim_vehicle) {
            if u.vehicle.as_deref() == Some(v.as_str()) {
                identity.damage_records_vehicle_match += 1;
            } else {
                identity.damage_records_vehicle_mismatch += 1;
            }
        }
    }

    let author = author_summary(&tracks, &x.weapons, &x.flight.author_uids);
    let mut kills_by_weapon: BTreeMap<String, u64> = BTreeMap::new();
    for k in &x.kills {
        *kills_by_weapon.entry(k.weapon.clone().unwrap_or_else(|| "(none recorded)".into())).or_default() += 1;
    }
    let is_author_uid = |u: Option<u16>| u.is_some_and(|u| x.flight.author_uids.contains(&u));
    let combat = CombatSummary {
        kills_seen: x.kills.len() as u64,
        damage_records: x.damage.len() as u64,
        kills_by_weapon,
        author_kills: x.kills.iter().filter(|k| is_author_uid(k.killer_uid)).cloned().collect(),
        author_deaths: x.kills.iter().filter(|k| is_author_uid(k.victim_uid)).cloned().collect(),
        evidence: Evidence::Derived,
    };
    let mut by_template: BTreeMap<String, u64> = BTreeMap::new();
    for w in &x.weapons {
        *by_template.entry(w.template.clone()).or_default() += 1;
    }
    let weapons = WeaponsSection {
        stats: x.weapon_stats.clone(),
        by_template,
        events: x.weapons.clone(),
        limitations: vec![
            "A name comes from a kill record or a shared type id. Types that never killed stay unnamed, so named statistics still lean toward effective weapons.",
            "The last observed sample is not an established impact. End time uncertainty is the sample interval.",
            "Launchers are attributed by geometry or kill record, not by a decoded launcher field.",
            "Sample spacing varies from 20 ms to over 1 s depending on network relevance.",
        ],
    };
    let proposals = strikemaster::proposals(&m.input.sha256, &m.input.file_name, &x.weapons, db);
    let strike_master = StrikeMasterSection {
        databases_loaded: db.is_some(),
        database_note: "Read-only. Proposals are for review; no database is modified by SDL.",
        proposals,
    };

    let anomalies = anomalies(&tracks, &points, &x.flight);
    let missing_data = missing_data(&tracks, &points);
    let mut data = ResearchData {
        schema: RESEARCH_SCHEMA,
        decoder: DecoderInfo { name: DECODER_NAME, version: DECODER_VERSION },
        replay,
        decoded_field_coverage: field_coverage(),
        flight_coverage: x.flight.coverage.clone(),
        kinematic_validation: x.flight.kinematic_validation.clone(),
        cockpit_validation: x.flight.cockpit_validation.clone(),
        author,
        identity,
        tracks,
        anomalies,
        missing_data,
        combat,
        weapons,
        strike_master,
        findings: Vec::new(),
        unresolved_questions: Vec::new(),
    };
    data.findings = findings(&data);
    data.unresolved_questions = questions(&data);
    Ok(ResearchRun { data, weapon_samples: x.weapon_samples })
}
