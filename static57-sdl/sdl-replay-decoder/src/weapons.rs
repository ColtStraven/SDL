//! Weapon reconstruction: joins store creation (ECS), in-flight records
//! (MPI 0xF0DB), aircraft state and the battle log.
//!
//! - Entity link: a `ShellsData` id is `entity_index << 10 | (generation + 2)`.
//! - Launcher: DERIVED by geometry (nearest aircraft to the creation position
//!   at creation time). No launcher field has been decoded.
//! - Weapon name: taken from a kill record linked by geometry and time, so it
//!   exists only for weapons that killed something.
//! - Outcome: a kill link is DERIVED. Any other weapon has an unknown outcome:
//!   the last observed sample is not an established impact.

use crate::combat::KillEvent;
use crate::ecs::EntityCreate;
use crate::evidence::{hex, Evidence};
use crate::shells::ShellsMessage;
use crate::units::Registry;
use serde::Serialize;
use std::collections::HashMap;

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

/// Launcher attribution limit: creation within this of an aircraft.
pub const LAUNCHER_MAX_M: f64 = 150.0;
/// Kill link limits: track end relative to the kill, and distance to victim.
pub const KILL_WINDOW_BEFORE_MS: u32 = 3_000;
pub const KILL_WINDOW_AFTER_MS: u32 = 1_500;
pub const KILL_MAX_M: f64 = 400.0;
/// A killer with a known track must be this near the launch point.
pub const KILLER_LAUNCH_MAX_M: f64 = 500.0;
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
    /// Distance from the creation position to the nearest aircraft.
    pub distance_m: Option<f64>,
    pub aircraft_speed_mps: Option<f64>,
    pub aircraft_altitude_m: Option<f64>,
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
    /// `kill_linked` or `unknown`.
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WeaponEvent {
    pub weapon_id: String,
    /// ECS template, or `unlinked` when no creation record was seen.
    pub template: String,
    /// ShellsData list index (0, 1 or 2).
    pub list: u8,
    /// Opaque bytes from the creation record; meaning not established.
    pub creation_prefix: Option<String>,
    pub weapon_name: Option<String>,
    pub weapon_name_evidence: Evidence,
    pub launch_time_ms: Option<u32>,
    pub launch_time_source: &'static str,
    pub launcher: Launcher,
    pub trajectory: Trajectory,
    pub outcome: Outcome,
    pub first_packet_index: u64,
}

#[derive(Debug, Clone, Copy)]
struct AircraftNow {
    t: u32,
    pos: [f64; 3],
    vel: [f64; 3],
}

impl AircraftNow {
    fn at(&self, t: u32) -> [f64; 3] {
        let dt = (t as f64 - self.t as f64) / 1000.0;
        [self.pos[0] + self.vel[0] * dt, self.pos[1] + self.vel[1] * dt, self.pos[2] + self.vel[2] * dt]
    }
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
    position: Option<[f32; 3]>,
    nearest: Option<(u16, f64, f64, f64)>,
}

struct Track {
    key: (u32, u32),
    /// Launches of this entity id seen before this track.
    reuse: u32,
    list: u8,
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
    pub kills_seen: u64,
    pub kills_linked_to_weapon: u64,
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

/// Bytes before the marker. Not a weapon type: one missile model shows many
/// different prefixes. Preserved raw for later investigation.
fn creation_prefix(d: &[u8]) -> Option<String> {
    Some(hex(&d[3..creation_marker(d)?]))
}

#[derive(Default)]
pub struct Collector {
    aircraft: HashMap<u16, AircraftNow>,
    created: HashMap<(u32, u32), Created>,
    tracks: Vec<Track>,
    /// Index of the current track for an entity id.
    current: HashMap<(u32, u32), usize>,
    kills: Vec<KillEvent>,
    stats: WeaponStats,
}

impl Collector {
    pub fn push_aircraft(&mut self, t: u32, uid: u16, pos: [f64; 3], vel: [f64; 3]) {
        self.aircraft.insert(uid, AircraftNow { t, pos, vel });
    }

    fn nearest(&self, t: u32, p: [f64; 3], max_age_ms: u32) -> Option<(u16, f64, f64, f64)> {
        self.aircraft
            .iter()
            .filter(|(_, a)| a.t.abs_diff(t) <= max_age_ms)
            .map(|(uid, a)| {
                let q = a.at(t);
                (*uid, dist(p, q), (a.vel[0].powi(2) + a.vel[1].powi(2) + a.vel[2].powi(2)).sqrt(), q[1])
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
    }

    pub fn push_create(&mut self, t: u32, e: &EntityCreate) {
        if !is_store_template(&e.template) {
            return;
        }
        self.stats.store_creations += 1;
        let position = creation_position(&e.data);
        let nearest = position.and_then(|p| self.nearest(t, p64(p), 1_000));
        self.created.insert(
            entity_key(e.eid),
            Created { t, template: e.template.clone(), prefix: creation_prefix(&e.data), position, nearest },
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
                        created: self.created.get(&key).cloned(),
                        samples: Vec::new(),
                    });
                    self.current.insert(key, self.tracks.len() - 1);
                    self.tracks.len() - 1
                }
            };
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
            launch_time_ms: Option<u32>,
            launch_time_source: &'static str,
            kill: Option<(usize, f64)>,
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
                let launcher = match created.and_then(|c| c.nearest) {
                    Some((uid, d, spd, alt)) if d <= LAUNCHER_MAX_M => Launcher {
                        uid: Some(uid),
                        vehicle: registry.resolve(uid, created.map_or(first.timestamp_ms, |c| c.t)).and_then(|u| u.vehicle.clone()),
                        loadout: registry.resolve(uid, created.map_or(first.timestamp_ms, |c| c.t)).and_then(|u| u.loadout.clone()),
                        is_author: author_uids.contains(&uid),
                        distance_m: Some(d),
                        aircraft_speed_mps: Some(spd),
                        aircraft_altitude_m: Some(alt),
                        evidence: Evidence::Derived,
                        status: "attributed_by_proximity",
                    },
                    near => Launcher {
                        uid: None,
                        vehicle: None,
                        loadout: None,
                        is_author: false,
                        distance_m: near.map(|n| n.1),
                        aircraft_speed_mps: None,
                        aircraft_altitude_m: None,
                        evidence: Evidence::Unknown,
                        status: if near.is_some() { "no_aircraft_near_launch" } else { "unavailable" },
                    },
                };
                let origin = p64(created.and_then(|c| c.position).unwrap_or(first.position_m));
                Prep { launcher, origin, launch_time_ms, launch_time_source, kill: None }
            })
            .collect();

        for (ki, k) in self.kills.iter().enumerate() {
            let Some(victim) = k.victim_uid else { continue };
            let Some(killer) = k.killer_uid else { continue };
            let mut best: Option<(usize, f64)> = None;
            for (ti, tr) in tracks.iter().enumerate() {
                let last = tr.samples.last().unwrap();
                if last.timestamp_ms > k.timestamp_ms + KILL_WINDOW_AFTER_MS || last.timestamp_ms + KILL_WINDOW_BEFORE_MS < k.timestamp_ms {
                    continue;
                }
                let p = &preps[ti];
                if p.kill.is_some() {
                    continue;
                }
                let killer_ok = match p.launcher.uid {
                    Some(l) => l == killer,
                    None => match history.at(killer, p.launch_time_ms.unwrap_or(tr.samples[0].timestamp_ms)) {
                        Some(q) => dist(p.origin, q) <= KILLER_LAUNCH_MAX_M,
                        None => !history.seen(killer),
                    },
                };
                if !killer_ok {
                    continue;
                }
                let Some(v) = history.at(victim, k.timestamp_ms) else { continue };
                let d = dist(p64(last.position_m), v);
                if d <= KILL_MAX_M && best.is_none_or(|b| d < b.1) {
                    best = Some((ti, d));
                }
            }
            if let Some((ti, d)) = best {
                preps[ti].kill = Some((ki, d));
                self.stats.kills_linked_to_weapon += 1;
            }
        }

        let mut events = Vec::with_capacity(tracks.len());
        let mut samples_out = Vec::with_capacity(tracks.len());
        for (tr, mut p) in tracks.into_iter().zip(preps) {
            let s = &tr.samples;
            let (first, last) = (s[0], *s.last().unwrap());
            let created = tr.created.as_ref();
            let launch_pos = created.and_then(|c| c.position);
            let end = p64(last.position_m);
            let mut path = 0.0;
            let mut max_gap = 0u32;
            for w in s.windows(2) {
                path += dist(p64(w[0].position_m), p64(w[1].position_m));
                max_gap = max_gap.max(w[1].timestamp_ms - w[0].timestamp_ms);
            }
            let tof = last.time_since_launch_s.filter(|t| *t >= 0.0).map(|t| t as f64);
            let near = history.nearest(last.timestamp_ms, end);
            let outcome = match p.kill {
                Some((ki, d)) => {
                    let k = &self.kills[ki];
                    if p.launcher.uid.is_none() {
                        p.launcher.uid = k.killer_uid;
                        p.launcher.vehicle = k.killer_vehicle.clone();
                        p.launcher.is_author = k.killer_uid.is_some_and(|u| author_uids.contains(&u));
                        p.launcher.evidence = Evidence::Derived;
                        p.launcher.status = "attributed_by_kill_record";
                    }
                    let target_then = k.victim_uid.zip(p.launch_time_ms).and_then(|(u, lt)| history.at(u, lt));
                    Outcome {
                        status: "kill_linked",
                        target_distance_at_launch_m: target_then.map(|q| dist(p.origin, q)),
                        target_altitude_at_launch_m: target_then.map(|q| q[1]),
                        victim_uid: k.victim_uid,
                        victim_vehicle: k.victim_uid.and_then(|u| registry.resolve(u, k.timestamp_ms)).and_then(|u| u.vehicle.clone()),
                        kill_timestamp_ms: Some(k.timestamp_ms),
                        kill_packet_index: Some(k.packet_index),
                        distance_to_victim_m: Some(d),
                        nearest_aircraft_uid: near.map(|n| n.0),
                        nearest_aircraft_m: near.map(|n| n.1),
                        evidence: Evidence::Derived,
                    }
                }
                None => Outcome {
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
                    evidence: Evidence::Unknown,
                },
            };
            let weapon_name = p.kill.and_then(|(ki, _)| self.kills[ki].weapon.clone());

            self.stats.weapons_tracked += 1;
            self.stats.weapons_linked_to_creation += created.is_some() as u64;
            self.stats.weapons_with_launcher += p.launcher.uid.is_some() as u64;
            self.stats.weapons_by_author += p.launcher.is_author as u64;
            self.stats.weapons_named += weapon_name.is_some() as u64;

            let weapon_id = if tr.reuse == 0 { format!("w{}.{}", tr.key.0, tr.key.1) } else { format!("w{}.{}r{}", tr.key.0, tr.key.1, tr.reuse) };
            events.push(WeaponEvent {
                weapon_id: weapon_id.clone(),
                template: created.map(|c| c.template.clone()).unwrap_or_else(|| "unlinked".into()),
                list: tr.list,
                creation_prefix: created.and_then(|c| c.prefix.clone()),
                weapon_name_evidence: if weapon_name.is_some() { Evidence::Derived } else { Evidence::Unknown },
                weapon_name,
                launch_time_ms: p.launch_time_ms,
                launch_time_source: p.launch_time_source,
                launcher: p.launcher,
                trajectory: Trajectory {
                    samples: s.len() as u64,
                    first_ms: first.timestamp_ms,
                    last_ms: last.timestamp_ms,
                    time_of_flight_s: tof,
                    time_of_flight_source: if tof.is_some() { "last_sample_timer" } else { "unavailable" },
                    max_sample_interval_s: (s.len() > 1).then_some(max_gap as f64 / 1000.0),
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

/// Per-uid position history (sync order) for kill linkage.
#[derive(Default)]
pub struct AircraftHistory {
    by_uid: HashMap<u16, Vec<(u32, [f32; 3])>>,
}

impl AircraftHistory {
    pub fn push(&mut self, uid: u16, t: u32, p: [f32; 3]) {
        self.by_uid.entry(uid).or_default().push((t, p));
    }

    /// Position of `uid` nearest in time, within 1.5 s.
    pub fn at(&self, uid: u16, t: u32) -> Option<[f64; 3]> {
        let v = self.by_uid.get(&uid)?;
        let i = v.partition_point(|x| x.0 < t);
        [i.checked_sub(1), Some(i)]
            .into_iter()
            .flatten()
            .filter_map(|j| v.get(j))
            .min_by_key(|x| x.0.abs_diff(t))
            .filter(|x| x.0.abs_diff(t) <= 1_500)
            .map(|x| p64(x.1))
    }

    /// Whether `uid` ever appears as an aircraft in this replay.
    pub fn seen(&self, uid: u16) -> bool {
        self.by_uid.contains_key(&uid)
    }

    /// Nearest aircraft to `p` at time `t`.
    pub fn nearest(&self, t: u32, p: [f64; 3]) -> Option<(u16, f64)> {
        self.by_uid
            .keys()
            .filter_map(|u| self.at(*u, t).map(|q| (*u, dist(p, q))))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
    }
}
