//! Weapon reconstruction: joins store creation (ECS), in-flight records
//! (MPI 0xF0DB), aircraft state and the battle log.
//!
//! - Entity link: a `ShellsData` id is `entity_index << 10 | (generation + 2)`.
//! - Launcher: the creation record names its launcher's entity id, resolved
//!   through the unit registry. Never inferred from geometry.
//! - Weapon name: a linked kill record, or another weapon with the same
//!   per-replay type id (98.8% in hold-out). The launcher's equipment list
//!   failed hold-out, so it supplies candidates only, never a name. See
//!   `docs/FINDINGS_M5_WEAPON_TYPE.md`.
//! - Outcome: a kill link is DERIVED. Any other weapon has an unknown outcome:
//!   the last observed sample is not an established impact.

use crate::aircraft::TargetRecord;
use crate::combat::{DamageEvent, HitMarker, KillEvent};
use crate::seeker::{self, SeekerObservation};
use crate::ecs::EntityCreate;
use crate::evidence::{hex, Evidence};
use crate::shells::ShellsMessage;
use crate::units::{weapon_equipment, Registry};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Store templates that produce in-flight records.
pub fn is_store_template(t: &str) -> bool {
    matches!(t, "rocket" | "bomb" | "torpedo") || t.starts_with("rocket+")
}

/// `(index, generation)` of an ECS raw id.
pub fn entity_key(eid: u32) -> (u32, u32) {
    (eid & 0x3F_FFFF, eid >> 22)
}

/// `(index, generation)` of a `ShellsData` id.
pub fn shell_key(id: u32) -> (u32, u32) {
    (id >> 10, (id & 0x3FF).wrapping_sub(2))
}

/// Kill link limits: track end relative to the kill, and distance to victim.
pub const KILL_WINDOW_BEFORE_MS: u32 = 3_000;
pub const KILL_WINDOW_AFTER_MS: u32 = 1_500;
pub const KILL_MAX_M: f64 = 400.0;
/// Surface victims are compared with their creation position, so moved units need slack.
pub const KILL_MAX_SURFACE_M: f64 = 1_500.0;
/// Shorter flights cannot carry a kill link.
pub const MIN_LINK_FLIGHT_S: f64 = 0.3;
/// A link needs the weapon's path to account for the distance it closed.
pub const MAX_REACH_FACTOR: f64 = 1.5;
pub const MAX_REACH_SLACK_M: f64 = 1_500.0;
/// A second weapon within this of the best distance makes a kill ambiguous.
pub const AMBIGUOUS_RATIO: f64 = 1.5;
pub const AMBIGUOUS_SLACK_M: f64 = 30.0;
/// A creation record this long after launch describes a weapon already in
/// flight, so its position is not a launch position.
pub const LATE_CREATION_MS: u32 = 2_000;
/// Silence after which an entity id is treated as a new weapon.
pub const REUSE_GAP_MS: u32 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct WeaponSample {
    pub timestamp_ms: u32,
    pub packet_index: u64,
    pub position_m: [f32; 3],
    pub velocity_mps: [f32; 3],
    /// First timer: seconds since launch.
    pub time_since_launch_s: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Launcher {
    pub uid: Option<u16>,
    pub vehicle: Option<String>,
    /// Launcher's loadout preset: a naming hint, not a weapon identification.
    pub loadout: Option<String>,
    pub is_author: bool,
    /// Raw launcher entity id from the creation record.
    pub entity_id: Option<u32>,
    /// `aircraft`, `ground` or `ship`.
    pub kind: Option<&'static str>,
    /// Distance from the creation position to the launcher at creation.
    pub distance_m: Option<f64>,
    pub aircraft_speed_mps: Option<f64>,
    pub aircraft_altitude_m: Option<f64>,
    pub velocity_mps: Option<[f32; 3]>,
    pub evidence: Evidence,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Trajectory {
    pub samples: u64,
    pub first_ms: u32,
    pub last_ms: u32,
    pub time_of_flight_s: Option<f64>,
    pub time_of_flight_source: &'static str,
    /// Largest interval between samples: the uncertainty of the end time.
    pub max_sample_interval_s: Option<f64>,
    /// First seen well after launch: launch position and geometry withheld.
    pub launch_late: bool,
    pub launch_position_m: Option<[f32; 3]>,
    pub launch_altitude_m: Option<f64>,
    pub first_speed_mps: f64,
    pub max_speed_mps: f64,
    pub max_altitude_m: f64,
    pub last_position_m: [f32; 3],
    pub last_altitude_m: f64,
    pub last_speed_mps: f64,
    /// Horizontal distance from launch (or first sample) to the last sample.
    pub ground_range_m: f64,
    pub path_length_m: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outcome {
    /// `confirmed_kill`, `confirmed_damage`, `confirmed_hit`, `ambiguous`, or
    /// `unknown`. A weapon disappearing establishes nothing, so a shot with no
    /// supporting record stays `unknown`.
    pub class: &'static str,
    pub basis: Option<String>,
    /// `kill_linked` when a kill record is attached, else `unknown`.
    pub status: &'static str,
    pub victim_uid: Option<u16>,
    pub victim_vehicle: Option<String>,
    pub kill_timestamp_ms: Option<u32>,
    pub kill_packet_index: Option<u64>,
    pub distance_to_victim_m: Option<f64>,
    /// Victim distance and altitude at launch time, for linked kills only.
    pub target_distance_at_launch_m: Option<f64>,
    pub target_altitude_at_launch_m: Option<f64>,
    /// Nearest aircraft to the last sample, within the sync interval.
    pub nearest_aircraft_uid: Option<u16>,
    pub nearest_aircraft_m: Option<f64>,
    pub evidence: Evidence,
}

/// Who the weapon was aimed at.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Target {
    /// `seeker`, `kill_record`, or `unavailable`.
    pub status: &'static str,
    pub uid: Option<u16>,
    pub vehicle: Option<String>,
    /// `aircraft` or `surface`.
    pub kind: Option<&'static str>,
    /// `seeker_estimate`, `seeker_line_of_sight`, both joined by `+`, or
    /// `kill_record`.
    pub method: Option<String>,
    pub support: Option<f64>,
    pub observations: Option<u32>,
    /// A designation record's uid: unconfirmed, it failed validation.
    pub designated_candidate_uid: Option<u16>,
    pub evidence: Evidence,
}

/// Launch geometry against an established aircraft target.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LaunchGeometry {
    pub target_distance_m: f64,
    pub target_altitude_m: f64,
    pub altitude_difference_m: f64,
    /// Angle from launcher velocity to the target.
    pub relative_bearing_deg: Option<f64>,
    pub target_speed_mps: f64,
    pub relative_speed_mps: Option<f64>,
    /// Positive when closing.
    pub closing_speed_mps: Option<f64>,
    /// Angle between target velocity and the line to the launcher: 0 is
    /// head-on, 180 is tail-on.
    pub target_aspect_deg: Option<f64>,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WeaponEvent {
    pub weapon_id: String,
    /// ECS template, or `unlinked` when no creation record was seen.
    pub template: String,
    /// ShellsData list index (0, 1 or 2).
    pub list: u8,
    /// Opaque bytes from the creation record; meaning not established.
    pub creation_prefix: Option<String>,
    /// Per-replay weapon type id from the creation record.
    pub type_id: Option<u32>,
    /// In-flight records carry a block in the seeker position. Bombs have
    /// it too, so this does not mean the weapon is guided.
    pub has_seeker_block: bool,
    pub weapon_name: Option<String>,
    /// `kill_record`, `replay_type_id`, or `unavailable`.
    pub weapon_name_source: &'static str,
    pub weapon_name_evidence: Evidence,
    /// Unconfirmed possibilities when no name could be established: disputed
    /// type-id names, or the launcher's equipment list.
    pub weapon_candidates: Vec<String>,
    pub launch_time_ms: Option<u32>,
    pub launch_time_source: &'static str,
    pub launcher: Launcher,
    pub trajectory: Trajectory,
    pub target: Target,
    pub geometry: Option<LaunchGeometry>,
    pub outcome: Outcome,
    pub first_packet_index: u64,
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn p64(p: [f32; 3]) -> [f64; 3] {
    p.map(|v| v as f64)
}

#[derive(Clone)]
struct Created {
    t: u32,
    template: String,
    prefix: Option<String>,
    type_id: Option<u32>,
    owner: Option<u32>,
    position: Option<[f32; 3]>,
}

/// One seeker observation with the weapon state it was sampled at.
struct SeekerSample {
    t: u32,
    position: [f64; 3],
    obs: SeekerObservation,
}

struct Track {
    key: (u32, u32),
    /// Launches of this entity id seen before this track.
    reuse: u32,
    list: u8,
    seeker_block: bool,
    seeker: Vec<SeekerSample>,
    created: Option<Created>,
    samples: Vec<WeaponSample>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WeaponStats {
    pub shells_messages: u64,
    pub shells_exact: u64,
    pub shells_failed: u64,
    pub weapon_records: u64,
    pub store_creations: u64,
    pub weapons_tracked: u64,
    pub weapons_linked_to_creation: u64,
    pub weapons_with_launcher: u64,
    pub weapons_by_author: u64,
    pub weapons_named: u64,
    pub named_by_kill_record: u64,
    pub named_by_type_id: u64,
    /// Leave-one-out: kill-named weapons whose name each method recovers
    /// with that kill hidden, and how many it attempted.
    pub holdout_type_id_correct: u64,
    pub holdout_type_id_attempted: u64,
    pub holdout_equipment_correct: u64,
    pub holdout_equipment_attempted: u64,
    pub kills_seen: u64,
    pub kills_linked_to_weapon: u64,
    /// Per target method: kill-linked weapons with a claim, and how many
    /// named the kill's victim.
    pub target_eval: BTreeMap<&'static str, (u64, u64)>,
    /// Per designation record kind: records near launch, and how many held
    /// a point within 300 m of where the weapon ended.
    pub designated_point_eval: BTreeMap<u8, (u64, u64)>,
    pub rejected_impossible_links: u64,
    pub outcome_classes: BTreeMap<&'static str, u64>,
    pub targets_seeker: u64,
    pub targets_kill_record: u64,
    pub geometry_available: u64,
    pub launchers_exact: u64,
}

/// Offset of the `01 00` marker that ends the opaque creation prefix.
fn creation_marker(d: &[u8]) -> Option<usize> {
    (4..d.len().min(11)).find(|&i| d[i] == 0x01 && d.get(i + 1) == Some(&0x00))
}

/// Launch position: three f32, 11 bytes after the marker (SDL observation on
/// rocket, bomb and torpedo creation records).
fn creation_position(d: &[u8]) -> Option<[f32; 3]> {
    let o = creation_marker(d)? + 11;
    if d.len() < o + 12 {
        return None;
    }
    let f = |o: usize| f32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    let p = [f(o), f(o + 4), f(o + 8)];
    (p[0].abs() < 3e5 && p[2].abs() < 3e5 && p[1] > -500.0 && p[1] < 40_000.0).then_some(p)
}

/// First varint of a creation record: a per-replay weapon type id. Ids are
/// allocated in blocks per loadout and do not carry across replays.
fn creation_type_id(d: &[u8]) -> Option<u32> {
    crate::bitstream::BitReader::new(d.get(3..)?).compressed("type_id").ok().map(|v| v as u32)
}

/// The launcher's entity id follows the type id, in compact form.
fn creation_owner(d: &[u8]) -> Option<u32> {
    let mut r = crate::bitstream::BitReader::new(d.get(3..)?);
    r.compressed("type_id").ok()?;
    crate::ecs::read_compact_eid(&mut r).ok()
}

/// Same weapon, different launcher variant.
pub fn base_name(n: &str) -> &str {
    n.trim_end_matches("_default")
}

/// Gun rounds never appear in `ShellsData`: a kill by one is not a store kill.
pub fn is_gun_round(n: &str) -> bool {
    const ROUND: [&str; 16] = ["ap", "api", "aphe", "he", "hei", "i", "t", "frag", "ball", "apit", "heat", "hvap", "apcr", "apds", "apfsds", "fs"];
    if n.split('_').all(|t| ROUND.contains(&t.to_ascii_lowercase().as_str())) {
        return true;
    }
    // Caliber prefix, as in `30mm_UBR6` or `35_228mm_aphe_i` (35 x 228 mm).
    n.split_once("mm_").and_then(|(c, _)| c.split('_').next()?.parse::<u32>().ok()).is_some_and(|c| c <= 57)
}

#[derive(Clone)]
struct NameQuery {
    type_id: Option<u32>,
    template: String,
    equipment: Vec<String>,
}

/// Names reported for each type id, and templates seen for each name.
#[derive(Default)]
struct NameEvidence {
    by_id: HashMap<u32, BTreeMap<String, u32>>,
    templates: HashMap<String, BTreeSet<String>>,
}

impl NameEvidence {
    fn add(&mut self, id: Option<u32>, name: &str, template: &str, by: i32) {
        let n = base_name(name).to_string();
        self.templates.entry(n.clone()).or_default().insert(template.to_string());
        if let Some(id) = id {
            let c = self.by_id.entry(id).or_default().entry(n).or_default();
            *c = c.saturating_add_signed(by);
        }
    }

    /// The clear majority name of a type id. Otherwise the disputed names.
    fn by_type_id(&self, id: Option<u32>) -> Result<Option<String>, Vec<String>> {
        let Some(m) = id.and_then(|i| self.by_id.get(&i)) else { return Ok(None) };
        let total: u32 = m.values().sum();
        let Some((name, n)) = m.iter().max_by_key(|x| *x.1) else { return Ok(None) };
        if total == 0 {
            Ok(None)
        } else if *n * 3 >= total * 2 {
            Ok(Some(name.clone()))
        } else {
            Err(m.iter().filter(|x| *x.1 > 0).map(|x| x.0.clone()).collect())
        }
    }

    /// Equipment names still possible for this weapon.
    fn candidates(&self, q: &NameQuery) -> Vec<String> {
        let mut out: Vec<String> = q
            .equipment
            .iter()
            .map(|n| base_name(n).to_string())
            .filter(|n| {
                // A name known from another template cannot be this weapon.
                self.templates.get(n).is_none_or(|t| t.contains(&q.template))
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

/// Bytes before the marker, preserved raw. The first varint is the type id;
/// the rest are not established.
fn creation_prefix(d: &[u8]) -> Option<String> {
    Some(hex(&d[3..creation_marker(d)?]))
}

#[derive(Default)]
pub struct Collector {
    designations: HashMap<u16, Vec<(u32, u16)>>,
    designated_points: HashMap<u16, Vec<(u32, u8, [f32; 3])>>,
    damage: Vec<DamageEvent>,
    hits: Vec<HitMarker>,
    created: HashMap<(u32, u32), Created>,
    tracks: Vec<Track>,
    /// Index of the current track for an entity id.
    current: HashMap<(u32, u32), usize>,
    kills: Vec<KillEvent>,
    stats: WeaponStats,
}

impl Collector {
    /// Unit references in an aircraft's target designation records.
    pub fn push_designations(&mut self, uid: u16, t: u32, targets: &[TargetRecord]) {
        for r in targets {
            if let Some(p) = r.position.filter(|p| p.iter().all(|v| v.is_finite() && v.abs() < 3e5) && *p != [0.0; 3]) {
                self.designated_points.entry(uid).or_default().push((t, r.kind, p));
            }
            if let Some(tail) = r.tail.filter(|v| v >> 16 == 0xFFFF) {
                let list = self.designations.entry(uid).or_default();
                let target = (tail & 0x7FF) as u16;
                if list.last().is_none_or(|l| l.1 != target || t > l.0 + 1_000) {
                    list.push((t, target));
                }
            }
        }
    }

    pub fn push_damage(&mut self, d: DamageEvent) {
        self.damage.push(d);
    }

    pub fn push_hit(&mut self, h: HitMarker) {
        self.hits.push(h);
    }

    pub fn push_create(&mut self, t: u32, e: &EntityCreate) {
        if !is_store_template(&e.template) {
            return;
        }
        self.stats.store_creations += 1;
        let position = creation_position(&e.data);
        self.created.insert(
            entity_key(e.eid),
            Created { t, template: e.template.clone(), prefix: creation_prefix(&e.data), type_id: creation_type_id(&e.data), owner: creation_owner(&e.data), position },
        );
    }

    pub fn note_failed_message(&mut self) {
        self.stats.shells_messages += 1;
        self.stats.shells_failed += 1;
    }

    pub fn push_shells(&mut self, packet_index: u64, t: u32, m: &ShellsMessage) {
        self.stats.shells_messages += 1;
        self.stats.shells_exact += m.exact as u64;
        if !m.exact {
            return;
        }
        for r in &m.records {
            self.stats.weapon_records += 1;
            let (velocity_mps, time_since_launch_s) = match &r.dynamics {
                Some(d) => (d.velocity_mps().map(|v| v as f32), d.timers_s.first().map(|v| *v as f32)),
                None => ([f32::NAN; 3], None),
            };
            let key = shell_key(r.eid);
            // Entity ids are reused: a timer running backwards or a long
            // silence means a new weapon.
            let reuse = self.current.get(&key).map(|&i| &self.tracks[i]).and_then(|tr| {
                let last = tr.samples.last()?;
                let restarted = matches!((time_since_launch_s, last.time_since_launch_s), (Some(a), Some(b)) if a + 0.5 < b);
                Some((tr.reuse, restarted || t.saturating_sub(last.timestamp_ms) > REUSE_GAP_MS))
            });
            let i = match reuse {
                Some((_, false)) => self.current[&key],
                other => {
                    self.tracks.push(Track {
                        key,
                        reuse: other.map_or(0, |o| o.0 + 1),
                        list: r.list,
                        seeker_block: false,
                        seeker: Vec::new(),
                        created: self.created.get(&key).cloned(),
                        samples: Vec::new(),
                    });
                    self.current.insert(key, self.tracks.len() - 1);
                    self.tracks.len() - 1
                }
            };
            if let Some(d) = &r.dynamics {
                if let Some(bits) = d.seeker_bits {
                    self.tracks[i].seeker_block = true;
                    let obs = seeker::parse(&d.seeker_block, bits);
                    if obs.target_estimate_m.is_some() || obs.line_of_sight.is_some() {
                        self.tracks[i].seeker.push(SeekerSample { t, position: p64(r.position_m), obs });
                    }
                }
            }
            self.tracks[i].samples.push(WeaponSample { timestamp_ms: t, packet_index, position_m: r.position_m, velocity_mps, time_since_launch_s });
        }
    }

    pub fn push_kill(&mut self, k: KillEvent) {
        self.stats.kills_seen += 1;
        self.kills.push(k);
    }

    /// Join tracks, launchers and kills. A kill chooses its best weapon: the
    /// nearest track end whose launcher can be the killer.
    #[allow(clippy::type_complexity)]
    pub fn finish(mut self, registry: &Registry, history: &AircraftHistory, author_uids: &[u16]) -> (Vec<WeaponEvent>, Vec<(String, Vec<WeaponSample>)>, WeaponStats) {
        let tracks = std::mem::take(&mut self.tracks);
        let speed = |w: &WeaponSample| w.velocity_mps.iter().map(|v| (*v as f64).powi(2)).sum::<f64>().sqrt();

        struct Prep {
            launcher: Launcher,
            origin: [f64; 3],
            launch_pos: Option<[f32; 3]>,
            late: bool,
            launch_time_ms: Option<u32>,
            launch_time_source: &'static str,
            kill: Option<(usize, f64)>,
            kill_ambiguous: bool,
            path_m: f64,
        }
        let mut preps: Vec<Prep> = tracks
            .iter()
            .map(|tr| {
                let first = tr.samples[0];
                let created = tr.created.as_ref();
                let (launch_time_ms, launch_time_source) = match (first.time_since_launch_s, created) {
                    (Some(tsl), _) if tsl >= 0.0 => (Some(first.timestamp_ms.saturating_sub((tsl * 1000.0) as u32)), "first_sample_minus_timer"),
                    (_, Some(c)) => (Some(c.t), "entity_creation"),
                    _ => (None, "unavailable"),
                };
                let at = created.map_or(first.timestamp_ms, |c| c.t);
                let late = created.zip(launch_time_ms).is_some_and(|(c, lt)| c.t > lt + LATE_CREATION_MS);
                let launch_pos = created.and_then(|c| c.position).filter(|_| !late);
                let entity = created.and_then(|c| c.owner).and_then(|o| registry.entity(o, at));
                let launcher = match entity {
                    Some(u) => {
                        let state = u.uid.filter(|_| u.kind == "aircraft").and_then(|uid| history.state_at(uid, at));
                        let pos = launch_pos.map(p64);
                        Launcher {
                            uid: u.uid,
                            vehicle: u.vehicle.clone(),
                            loadout: u.loadout.clone(),
                            is_author: u.kind == "aircraft" && u.uid.is_some_and(|i| author_uids.contains(&i)),
                            entity_id: Some(u.eid),
                            kind: Some(u.kind),
                            distance_m: state.as_ref().zip(pos).map(|(s, p)| dist(p, s.0)),
                            aircraft_speed_mps: state.as_ref().map(|s| s.1.iter().map(|v| (*v as f64).powi(2)).sum::<f64>().sqrt()),
                            aircraft_altitude_m: state.as_ref().map(|s| s.0[1]),
                            velocity_mps: state.as_ref().map(|s| s.1),
                            evidence: Evidence::Verified,
                            status: "creation_record",
                        }
                    }
                    None => Launcher {
                        uid: None,
                        vehicle: None,
                        loadout: None,
                        is_author: false,
                        entity_id: created.and_then(|c| c.owner),
                        kind: None,
                        distance_m: None,
                        aircraft_speed_mps: None,
                        aircraft_altitude_m: None,
                        velocity_mps: None,
                        evidence: Evidence::Unknown,
                        status: if created.is_some() { "entity_not_found" } else { "no_creation_record" },
                    },
                };
                let origin = p64(launch_pos.unwrap_or(first.position_m));
                let path_m = tr.samples.windows(2).map(|w| dist(p64(w[0].position_m), p64(w[1].position_m))).sum();
                Prep { launcher, origin, launch_pos, late, launch_time_ms, launch_time_source, kill: None, kill_ambiguous: false, path_m }
            })
            .collect();

        for (ki, k) in self.kills.iter().enumerate() {
            let Some(victim) = k.victim_uid else { continue };
            let Some(killer) = k.killer_uid else { continue };
            // Surface units have no decoded track: their last known position
            // is where they were created.
            let victim_spawn = k.victim_surface.then(|| registry.surface_unit(victim, k.timestamp_ms).and_then(|u| u.spawn_position_m)).flatten();
            if k.weapon.as_deref().is_some_and(is_gun_round) {
                continue;
            }
            let mut best: Option<(usize, f64)> = None;
            let mut runner_up: Option<(usize, f64)> = None;
            for (ti, tr) in tracks.iter().enumerate() {
                let last = tr.samples.last().unwrap();
                if last.timestamp_ms > k.timestamp_ms + KILL_WINDOW_AFTER_MS || last.timestamp_ms + KILL_WINDOW_BEFORE_MS < k.timestamp_ms {
                    continue;
                }
                let p = &preps[ti];
                if p.kill.is_some() {
                    continue;
                }
                // The launcher is exact, so the killer must be that unit.
                if p.launcher.uid != Some(killer) || (p.launcher.kind != Some("aircraft")) != k.killer_surface {
                    continue;
                }
                let (v, limit) = if k.victim_surface {
                    match victim_spawn {
                        Some(sp) => (p64(sp), KILL_MAX_SURFACE_M),
                        None => continue,
                    }
                } else {
                    match history.at(victim, k.timestamp_ms) {
                        Some(v) => (v, KILL_MAX_M),
                        None => continue,
                    }
                };
                let d = dist(p64(last.position_m), v);
                if d > limit {
                    continue;
                }
                // Reject what the weapon could not physically have done.
                let tof = last.time_since_launch_s.unwrap_or(0.0) as f64;
                let start = p.launch_time_ms.and_then(|lt| history.at(victim, lt)).map(|q| dist(p.origin, q));
                if tof < MIN_LINK_FLIGHT_S || start.is_some_and(|s0| s0 - d > p.path_m * MAX_REACH_FACTOR + MAX_REACH_SLACK_M) {
                    self.stats.rejected_impossible_links += 1;
                    continue;
                }
                if best.is_none_or(|b| d < b.1) {
                    runner_up = best;
                    best = Some((ti, d));
                } else if runner_up.is_none_or(|b| d < b.1) {
                    runner_up = Some((ti, d));
                }
            }
            if let Some((ti, d)) = best {
                preps[ti].kill = Some((ki, d));
                self.stats.kills_linked_to_weapon += 1;
                // A second weapon nearly as close: the kill cannot be split.
                if let Some((other, d2)) = runner_up.filter(|r| r.1 <= d * AMBIGUOUS_RATIO + AMBIGUOUS_SLACK_M) {
                    let _ = d2;
                    preps[ti].kill_ambiguous = true;
                    preps[other].kill_ambiguous = true;
                }
            }
        }

        // Target evidence, independent of kill records.
        let targets: Vec<Vec<TargetClaim>> = tracks
            .iter()
            .zip(&preps)
            .map(|(tr, p)| target_claims(tr, p.launcher.uid.filter(|_| p.launcher.kind == Some("aircraft")), p.launch_time_ms, &self.designations, history))
            .collect();
        for ((claims, p), tr) in targets.iter().zip(&preps).zip(&tracks) {
            let _ = tr;
            let Some((ki, _)) = p.kill else { continue };
            let k = &self.kills[ki];
            if k.victim_surface {
                continue;
            }
            for c in claims {
                let e = self.stats.target_eval.entry(c.method).or_default();
                e.0 += 1;
                e.1 += (Some(c.uid) == k.victim_uid) as u64;
            }
        }

        // Do launcher designation points mark where weapons ended?
        for (tr, p) in tracks.iter().zip(&preps) {
            let (Some(u), Some(lt)) = (p.launcher.uid.filter(|_| p.launcher.kind == Some("aircraft")), p.launch_time_ms) else { continue };
            let end = p64(tr.samples.last().unwrap().position_m);
            let mut seen = BTreeSet::new();
            for d in self.designated_points.get(&u).into_iter().flatten().filter(|d| d.0 <= lt + DESIGNATION_AFTER_MS && d.0 + DESIGNATION_BEFORE_MS >= lt) {
                if seen.insert(d.1) {
                    let e = self.stats.designated_point_eval.entry(d.1).or_default();
                    e.0 += 1;
                    e.1 += (dist(p64(d.2), end) <= 300.0) as u64;
                }
            }
        }

        // Weapon naming. A type id shares one launcher vehicle, so a launcher
        // known for any weapon of an id supplies equipment for all of them.
        let mut id_launcher: HashMap<u32, (u16, u32)> = HashMap::new();
        for (tr, p) in tracks.iter().zip(&preps) {
            if let (Some(id), Some(uid)) = (tr.created.as_ref().and_then(|c| c.type_id), p.launcher.uid) {
                id_launcher.entry(id).or_insert((uid, tr.samples[0].timestamp_ms));
            }
        }
        let queries: Vec<NameQuery> = tracks
            .iter()
            .zip(&preps)
            .map(|(tr, p)| {
                let type_id = tr.created.as_ref().and_then(|c| c.type_id);
                let at = tr.samples[0].timestamp_ms;
                let launcher = p.launcher.uid.map(|u| (u, at)).or_else(|| type_id.and_then(|i| id_launcher.get(&i).copied()));
                NameQuery {
                    type_id,
                    template: tr.created.as_ref().map_or_else(|| "unlinked".to_string(), |c| c.template.clone()),
                    equipment: launcher.and_then(|(u, t)| registry.resolve(u, t)).map(|u| weapon_equipment(&u.equipment)).unwrap_or_default(),
                }
            })
            .collect();
        // An ambiguous link does not say which weapon the record names.
        let kill_name = |p: &Prep| p.kill.filter(|_| !p.kill_ambiguous).and_then(|(ki, _)| self.kills[ki].weapon.clone());
        let mut evidence = NameEvidence::default();
        for (p, q) in preps.iter().zip(&queries) {
            if let Some(n) = kill_name(p) {
                evidence.add(q.type_id, &n, &q.template, 1);
            }
        }
        // Hold-out: hide each kill-confirmed name and try to recover it.
        for (p, q) in preps.iter().zip(&queries) {
            let Some(n) = kill_name(p) else { continue };
            let truth = base_name(&n).to_string();
            evidence.add(q.type_id, &n, &q.template, -1);
            if let Ok(Some(guess)) = evidence.by_type_id(q.type_id) {
                self.stats.holdout_type_id_attempted += 1;
                self.stats.holdout_type_id_correct += (guess == truth) as u64;
            }
            if let [one] = evidence.candidates(q).as_slice() {
                self.stats.holdout_equipment_attempted += 1;
                self.stats.holdout_equipment_correct += (*one == truth) as u64;
            }
            evidence.add(q.type_id, &n, &q.template, 1);
        }

        let mut events = Vec::with_capacity(tracks.len());
        let mut samples_out = Vec::with_capacity(tracks.len());
        for (((tr, p), query), claims) in tracks.into_iter().zip(preps).zip(queries).zip(targets) {
            let s = &tr.samples;
            let (first, last) = (s[0], *s.last().unwrap());
            let created = tr.created.as_ref();
            let launch_pos = p.launch_pos;
            let end = p64(last.position_m);
            let mut path = 0.0;
            let mut max_gap = 0u32;
            for w in s.windows(2) {
                path += dist(p64(w[0].position_m), p64(w[1].position_m));
                max_gap = max_gap.max(w[1].timestamp_ms - w[0].timestamp_ms);
            }
            let tof = last.time_since_launch_s.filter(|t| *t >= 0.0).map(|t| t as f64);
            let near = history.nearest(last.timestamp_ms, end);

            // Target: seeker observation first, then the kill record.
            let seeker_claims: Vec<&TargetClaim> = claims.iter().filter(|c| c.method != "launcher_designation").collect();
            let designated = claims.iter().find(|c| c.method == "launcher_designation").map(|c| c.uid);
            let first_claim = seeker_claims.first().copied();
            let agree = seeker_claims.iter().all(|c| Some(c.uid) == first_claim.map(|f| f.uid));
            let kill_ref = p.kill.map(|(ki, _)| &self.kills[ki]);
            let mut target = Target {
                status: "unavailable",
                uid: None,
                vehicle: None,
                kind: None,
                method: None,
                support: None,
                observations: None,
                designated_candidate_uid: designated,
                evidence: Evidence::Unknown,
            };
            if let (Some(c), true) = (first_claim, agree) {
                target.status = "seeker";
                target.uid = Some(c.uid);
                target.vehicle = registry.resolve(c.uid, first.timestamp_ms).and_then(|u| u.vehicle.clone());
                target.kind = Some("aircraft");
                target.method = Some(seeker_claims.iter().map(|c| c.method).collect::<Vec<_>>().join("+"));
                target.support = Some(c.support);
                target.observations = Some(c.observations);
                target.evidence = Evidence::Derived;
                self.stats.targets_seeker += 1;
            } else if let Some(k) = kill_ref {
                target.status = "kill_record";
                target.uid = k.victim_uid;
                target.kind = Some(if k.victim_surface { "surface" } else { "aircraft" });
                target.vehicle = k.victim_uid.and_then(|u| {
                    if k.victim_surface {
                        registry.surface_unit(u, k.timestamp_ms).and_then(|e| e.vehicle.clone())
                    } else {
                        registry.resolve(u, k.timestamp_ms).and_then(|e| e.vehicle.clone())
                    }
                });
                target.method = Some("kill_record".to_string());
                target.evidence = Evidence::Derived;
                self.stats.targets_kill_record += 1;
            }

            // Geometry needs a tracked aircraft target.
            let target_track = target.uid.filter(|_| target.kind == Some("aircraft"));
            let geometry = target_track.zip(p.launch_time_ms).filter(|_| !p.late).and_then(|(u, lt)| launch_geometry(p.origin, p.launcher.velocity_mps, history.state_at(u, lt)?));
            self.stats.geometry_available += geometry.is_some() as u64;

            // Outcome evidence other than a kill.
            let window = |t: u32| t + KILL_WINDOW_BEFORE_MS >= last.timestamp_ms && t <= last.timestamp_ms + KILL_WINDOW_BEFORE_MS;
            let near_end = |uid: Option<u16>, surface: bool| !surface && uid.and_then(|u| history.at(u, last.timestamp_ms)).is_some_and(|q| dist(end, q) <= KILL_MAX_M);
            let damaged = self.damage.iter().find(|d| {
                d.attacker_uid == p.launcher.uid && d.attacker_uid.is_some() && (p.launcher.kind != Some("aircraft")) == d.attacker_surface && window(d.timestamp_ms) && near_end(d.victim_uid, d.victim_surface)
            });
            let hit = if p.launcher.is_author { self.hits.iter().find(|h| window(h.timestamp_ms) && near_end(h.target_uid, h.target_surface)) } else { None };
            let (class, basis): (&'static str, Option<String>) = if p.kill_ambiguous {
                ("ambiguous", Some("another weapon from this launcher ended about as close to the victim".to_string()))
            } else if p.kill.is_some() {
                ("confirmed_kill", Some("kill record: same launcher, weapon ended at the victim".to_string()))
            } else if let Some(d) = damaged {
                ("confirmed_damage", Some(format!("{} damage record from this launcher, victim at the weapon end", d.kind)))
            } else if hit.is_some() {
                ("confirmed_hit", Some("author hit marker, target at the weapon end".to_string()))
            } else {
                ("unknown", None)
            };
            *self.stats.outcome_classes.entry(class).or_default() += 1;
            self.stats.launchers_exact += (p.launcher.status == "creation_record") as u64;

            let outcome = match p.kill {
                Some((ki, d)) => {
                    let k = &self.kills[ki];
                    let target_then = k.victim_uid.zip(p.launch_time_ms).and_then(|(u, lt)| history.at(u, lt));
                    Outcome {
                        class,
                        basis: basis.clone(),
                        status: "kill_linked",
                        target_distance_at_launch_m: target_then.map(|q| dist(p.origin, q)),
                        target_altitude_at_launch_m: target_then.map(|q| q[1]),
                        victim_uid: k.victim_uid,
                        victim_vehicle: k.victim_uid.and_then(|u| {
                            if k.victim_surface {
                                registry.surface_unit(u, k.timestamp_ms).and_then(|e| e.vehicle.clone())
                            } else {
                                registry.resolve(u, k.timestamp_ms).and_then(|e| e.vehicle.clone())
                            }
                        }),
                        kill_timestamp_ms: Some(k.timestamp_ms),
                        kill_packet_index: Some(k.packet_index),
                        distance_to_victim_m: Some(d),
                        nearest_aircraft_uid: near.map(|n| n.0),
                        nearest_aircraft_m: near.map(|n| n.1),
                        evidence: Evidence::Derived,
                    }
                }
                None => Outcome {
                    class,
                    basis: basis.clone(),
                    status: "unknown",
                    target_distance_at_launch_m: None,
                    target_altitude_at_launch_m: None,
                    victim_uid: None,
                    victim_vehicle: None,
                    kill_timestamp_ms: None,
                    kill_packet_index: None,
                    distance_to_victim_m: None,
                    nearest_aircraft_uid: near.map(|n| n.0),
                    nearest_aircraft_m: near.map(|n| n.1),
                    evidence: if class == "unknown" { Evidence::Unknown } else { Evidence::Derived },
                },
            };
            let killed_with = p.kill.filter(|_| !p.kill_ambiguous).and_then(|(ki, _)| self.kills[ki].weapon.clone());
            let (weapon_name, weapon_name_source, weapon_name_evidence, weapon_candidates) = if let Some(n) = killed_with {
                self.stats.named_by_kill_record += 1;
                (Some(n), "kill_record", Evidence::Derived, Vec::new())
            } else {
                match evidence.by_type_id(query.type_id) {
                    Ok(Some(n)) => {
                        self.stats.named_by_type_id += 1;
                        (Some(n), "replay_type_id", Evidence::Derived, Vec::new())
                    }
                    Err(disputed) => (None, "unavailable", Evidence::Unknown, disputed),
                    Ok(None) => (None, "unavailable", Evidence::Unknown, evidence.candidates(&query)),
                }
            };

            self.stats.weapons_tracked += 1;
            self.stats.weapons_linked_to_creation += created.is_some() as u64;
            self.stats.weapons_with_launcher += (p.launcher.status == "creation_record") as u64;
            self.stats.weapons_by_author += p.launcher.is_author as u64;
            self.stats.weapons_named += weapon_name.is_some() as u64;

            let weapon_id = if tr.reuse == 0 { format!("w{}.{}", tr.key.0, tr.key.1) } else { format!("w{}.{}r{}", tr.key.0, tr.key.1, tr.reuse) };
            events.push(WeaponEvent {
                weapon_id: weapon_id.clone(),
                template: created.map(|c| c.template.clone()).unwrap_or_else(|| "unlinked".into()),
                list: tr.list,
                creation_prefix: created.and_then(|c| c.prefix.clone()),
                type_id: query.type_id,
                has_seeker_block: tr.seeker_block,
                weapon_name,
                weapon_name_source,
                weapon_name_evidence,
                weapon_candidates,
                launch_time_ms: p.launch_time_ms,
                launch_time_source: p.launch_time_source,
                launcher: p.launcher,
                target,
                geometry,
                trajectory: Trajectory {
                    samples: s.len() as u64,
                    first_ms: first.timestamp_ms,
                    last_ms: last.timestamp_ms,
                    time_of_flight_s: tof,
                    time_of_flight_source: if tof.is_some() { "last_sample_timer" } else { "unavailable" },
                    max_sample_interval_s: (s.len() > 1).then_some(max_gap as f64 / 1000.0),
                    launch_late: p.late,
                    launch_position_m: launch_pos,
                    launch_altitude_m: launch_pos.map(|q| q[1] as f64),
                    first_speed_mps: speed(&first),
                    max_speed_mps: s.iter().map(speed).fold(0.0, f64::max),
                    max_altitude_m: s.iter().map(|w| w.position_m[1] as f64).fold(f64::MIN, f64::max),
                    last_position_m: last.position_m,
                    last_altitude_m: last.position_m[1] as f64,
                    last_speed_mps: speed(&last),
                    ground_range_m: (end[0] - p.origin[0]).hypot(end[2] - p.origin[2]),
                    path_length_m: path,
                },
                outcome,
                first_packet_index: first.packet_index,
            });
            samples_out.push((weapon_id, tr.samples));
        }
        (events, samples_out, self.stats)
    }
}

/// A target claim: which aircraft a weapon was aimed at, by one method.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetClaim {
    pub method: &'static str,
    pub uid: u16,
    /// Fraction of usable observations naming this uid.
    pub support: f64,
    pub observations: u32,
}

/// Radar estimate within this of an aircraft identifies it.
pub const SEEKER_MATCH_M: f64 = 300.0;
/// Line of sight within this of an aircraft, clearly nearer than any other.
pub const LOS_MATCH_DEG: f64 = 3.0;
pub const LOS_CLEAR_RATIO: f64 = 2.0;
/// A claim needs this many observations and this share of them.
pub const MIN_OBSERVATIONS: u32 = 2;
pub const MIN_SUPPORT: f64 = 0.6;
/// Designations are read from this long before launch until just after.
pub const DESIGNATION_BEFORE_MS: u32 = 3_000;
pub const DESIGNATION_AFTER_MS: u32 = 500;

fn majority(votes: &HashMap<u16, u32>, total: u32) -> Option<(u16, f64, u32)> {
    let (uid, n) = votes.iter().max_by_key(|x| *x.1)?;
    let support = *n as f64 / total as f64;
    (total >= MIN_OBSERVATIONS && support >= MIN_SUPPORT).then_some((*uid, support, total))
}

fn target_claims(tr: &Track, launcher: Option<u16>, launch: Option<u32>, designations: &HashMap<u16, Vec<(u32, u16)>>, history: &AircraftHistory) -> Vec<TargetClaim> {
    let mut out = Vec::new();
    if let (Some(l), Some(t)) = (launcher, launch) {
        let near: Vec<u16> = designations
            .get(&l)
            .into_iter()
            .flatten()
            .filter(|d| d.0 + DESIGNATION_BEFORE_MS >= t && d.0 <= t + DESIGNATION_AFTER_MS)
            .map(|d| d.1)
            .collect();
        if let Some(last) = near.last() {
            out.push(TargetClaim { method: "launcher_designation", uid: *last, support: near.iter().filter(|u| *u == last).count() as f64 / near.len() as f64, observations: near.len() as u32 });
        }
    }
    let (mut est, mut est_n, mut los, mut los_n) = (HashMap::new(), 0u32, HashMap::new(), 0u32);
    for s in &tr.seeker {
        if let Some(e) = s.obs.target_estimate_m {
            est_n += 1;
            if let Some((uid, d)) = history.nearest_excluding(s.t, p64(e), launcher) {
                if d <= SEEKER_MATCH_M {
                    *est.entry(uid).or_insert(0) += 1;
                }
            }
        }
        if let Some(l) = s.obs.line_of_sight {
            los_n += 1;
            if let Some((uid, a, next)) = history.along_line(s.t, s.position, l, launcher) {
                if a <= LOS_MATCH_DEG && next >= a.max(0.25) * LOS_CLEAR_RATIO {
                    *los.entry(uid).or_insert(0) += 1;
                }
            }
        }
    }
    if let Some((uid, support, observations)) = majority(&est, est_n) {
        out.push(TargetClaim { method: "seeker_estimate", uid, support, observations });
    }
    if let Some((uid, support, observations)) = majority(&los, los_n) {
        out.push(TargetClaim { method: "seeker_line_of_sight", uid, support, observations });
    }
    out
}

fn len3(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn angle_deg(a: [f64; 3], b: [f64; 3]) -> Option<f64> {
    let (na, nb) = (len3(a), len3(b));
    (na > 1.0 && nb > 1.0).then(|| ((a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) / (na * nb)).clamp(-1.0, 1.0).acos().to_degrees())
}

/// Geometry at launch from the launch point, launcher velocity and target.
pub fn launch_geometry(origin: [f64; 3], launcher_v: Option<[f32; 3]>, target: ([f64; 3], [f32; 3])) -> Option<LaunchGeometry> {
    let (tp, tv) = target;
    let tv = tv.map(|v| v as f64);
    let los = [tp[0] - origin[0], tp[1] - origin[1], tp[2] - origin[2]];
    let range = len3(los);
    if range < 1.0 {
        return None;
    }
    let lv = launcher_v.map(|v| v.map(|x| x as f64));
    let rel = lv.map(|l| [tv[0] - l[0], tv[1] - l[1], tv[2] - l[2]]);
    Some(LaunchGeometry {
        target_distance_m: range,
        target_altitude_m: tp[1],
        altitude_difference_m: tp[1] - origin[1],
        relative_bearing_deg: lv.and_then(|l| angle_deg(l, los)),
        target_speed_mps: len3(tv),
        relative_speed_mps: rel.map(len3),
        closing_speed_mps: rel.map(|r| -(r[0] * los[0] + r[1] * los[1] + r[2] * los[2]) / range),
        target_aspect_deg: angle_deg(tv, [-los[0], -los[1], -los[2]]),
        evidence: Evidence::Derived,
    })
}

/// Per-uid position history (sync order) for kill linkage.
#[derive(Default)]
pub struct AircraftHistory {
    by_uid: HashMap<u16, Vec<(u32, [f32; 3], [f32; 3])>>,
}

impl AircraftHistory {
    pub fn push(&mut self, uid: u16, t: u32, p: [f32; 3], v: [f32; 3]) {
        self.by_uid.entry(uid).or_default().push((t, p, v));
    }

    /// Position and velocity of `uid` nearest in time, within 1.5 s, carried
    /// to `t` with its velocity.
    pub fn state_at(&self, uid: u16, t: u32) -> Option<([f64; 3], [f32; 3])> {
        let v = self.by_uid.get(&uid)?;
        let i = v.partition_point(|x| x.0 < t);
        let s = [i.checked_sub(1), Some(i)].into_iter().flatten().filter_map(|j| v.get(j)).min_by_key(|x| x.0.abs_diff(t)).filter(|x| x.0.abs_diff(t) <= 1_500)?;
        let dt = (t as f64 - s.0 as f64) / 1000.0;
        let p = [0, 1, 2].map(|k| s.1[k] as f64 + s.2[k] as f64 * dt);
        Some((p, s.2))
    }

    /// Position of `uid` nearest in time, within 1.5 s.
    pub fn at(&self, uid: u16, t: u32) -> Option<[f64; 3]> {
        self.state_at(uid, t).map(|s| s.0)
    }

    /// Uids with a record near `t`.
    pub fn uids(&self) -> impl Iterator<Item = u16> + '_ {
        self.by_uid.keys().copied()
    }

    /// Nearest aircraft to `p` at `t`, other than `skip`.
    pub fn nearest_excluding(&self, t: u32, p: [f64; 3], skip: Option<u16>) -> Option<(u16, f64)> {
        self.by_uid
            .keys()
            .filter(|u| Some(**u) != skip)
            .filter_map(|u| self.at(*u, t).map(|q| (*u, dist(p, q))))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
    }

    /// The aircraft closest in angle to a line of sight: `(uid, angle, next
    /// angle)` in degrees.
    pub fn along_line(&self, t: u32, from: [f64; 3], los: [f32; 3], skip: Option<u16>) -> Option<(u16, f64, f64)> {
        let mut v: Vec<(u16, f64)> = self
            .by_uid
            .keys()
            .filter(|u| Some(**u) != skip)
            .filter_map(|u| {
                let q = self.at(*u, t)?;
                let d = [q[0] - from[0], q[1] - from[1], q[2] - from[2]];
                let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                (n > 30.0).then(|| {
                    let c = (0..3).map(|k| d[k] * los[k] as f64).sum::<f64>() / n;
                    (*u, c.clamp(-1.0, 1.0).acos().to_degrees())
                })
            })
            .collect();
        v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        let first = *v.first()?;
        Some((first.0, first.1, v.get(1).map_or(180.0, |x| x.1)))
    }

    /// Nearest aircraft to `p` at time `t`.
    pub fn nearest(&self, t: u32, p: [f64; 3]) -> Option<(u16, f64)> {
        self.by_uid
            .keys()
            .filter_map(|u| self.at(*u, t).map(|q| (*u, dist(p, q))))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
    }
}
