//! Cross-replay research analysis over per-replay datasets.

use crate::manifest::{DECODER_NAME, DECODER_VERSION};
use crate::measure::{self, round3, robust_z, At, Stat};
use crate::research::ResearchData;
use crate::strikemaster::Proposal;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub const SUMMARY_SCHEMA: &str = "static57.sdl.research-summary/0.2";
/// Robust z-score beyond which a replay is listed as an outlier.
pub const OUTLIER_Z: f64 = 3.5;
/// Fewest samples for a shot to enter flight statistics: shorter tracks are
/// glimpses, not flights.
pub const MIN_SHOT_SAMPLES: u64 = 5;
/// Fewest comparisons for a replay to enter a consistency range.
pub const MIN_COMPARISONS: u64 = 200;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Totals {
    pub replays: u64,
    pub replays_ok: u64,
    pub flight_samples: u64,
    pub tracks: u64,
    pub replays_with_author: u64,
    pub replays_with_cockpit: u64,
    pub author_tracked_seconds: f64,
    pub weapons_tracked: u64,
    pub weapons_by_author: u64,
    pub weapons_named: u64,
    pub kills: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayRow {
    pub file_name: String,
    pub sha256: String,
    pub game_version: String,
    pub mission: Option<String>,
    pub status: &'static str,
    pub author_vehicles: Vec<String>,
    pub author_tracked_seconds: Option<f64>,
    pub author_max_speed_mps: Option<f64>,
    pub author_max_altitude_m: Option<f64>,
    pub author_max_load_factor_g: Option<f64>,
    pub weapons: u64,
    pub weapons_by_author: u64,
    pub report_dir: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VehicleSummary {
    pub vehicle: String,
    pub replays: u64,
    pub tracked_seconds: f64,
    pub max_altitude_m: Option<f64>,
    pub max_speed_mps: Option<f64>,
    /// Smallest per-replay maximum speed: shows the spread of the sorties.
    pub max_speed_low_mps: Option<f64>,
    pub speed_saturated: bool,
    pub max_load_factor_g: Option<f64>,
    pub max_climb_10s_mps: Option<f64>,
    pub max_turn_rate_deg_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeaponSummary {
    pub weapon_name: String,
    pub s57_weapon_id: Option<String>,
    pub shots: u64,
    pub kill_linked: u64,
    pub launchers: Vec<String>,
    pub time_of_flight_s: Stat,
    pub ground_range_m: Stat,
    pub max_speed_mps: Stat,
    pub launch_altitude_m: Stat,
    pub target_distance_at_launch_m: Stat,
    pub caution: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Consistency {
    pub check: &'static str,
    pub unit: &'static str,
    pub replays: u64,
    pub best: Option<f64>,
    pub median: Option<f64>,
    pub worst: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outlier {
    pub file_name: String,
    pub measure: &'static str,
    pub value: f64,
    pub robust_z: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub title: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrossReplay {
    pub schema: &'static str,
    pub decoder_name: &'static str,
    pub decoder_version: &'static str,
    pub totals: Totals,
    pub game_versions: Vec<String>,
    pub replays: Vec<ReplayRow>,
    pub vehicles: Vec<VehicleSummary>,
    pub weapons: Vec<WeaponSummary>,
    pub consistency: Vec<Consistency>,
    pub outliers: Vec<Outlier>,
    pub anomaly_totals: BTreeMap<String, u64>,
    pub recommendations: Vec<Recommendation>,
}

fn range(check: &'static str, unit: &'static str, data: &[ResearchData], f: impl Fn(&ResearchData) -> Option<(u64, f64)>) -> Consistency {
    let mut v: Vec<f64> = data.iter().filter_map(&f).filter(|(n, _)| *n >= MIN_COMPARISONS).map(|(_, x)| x).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Consistency {
        check,
        unit,
        replays: v.len() as u64,
        best: v.first().map(|x| round3(*x)),
        median: v.get(v.len() / 2).map(|x| round3(*x)),
        worst: v.last().map(|x| round3(*x)),
    }
}

fn plain_stat(v: Vec<f64>) -> Stat {
    measure::stat(v.into_iter().map(|x| (x, At { timestamp_ms: 0, packet_index: 0 })).collect())
}

/// Each row's measurement list contains only the rows that have the value.
/// `floor` is the practical size below which a statistical outlier is noise.
fn outliers_for(rows: &[(String, f64)], measure: &'static str, floor: f64, out: &mut Vec<Outlier>) {
    let values: Vec<f64> = rows.iter().map(|r| r.1).collect();
    if let Some(z) = robust_z(&values) {
        for (r, z) in rows.iter().zip(z) {
            if z.abs() > OUTLIER_Z && r.1 > floor {
                out.push(Outlier { file_name: r.0.clone(), measure, value: round3(r.1), robust_z: round3(z) });
            }
        }
    }
}

/// `dirs[i]` is the report folder name of `data[i]`.
pub fn analyze(data: &[ResearchData], dirs: &[String]) -> CrossReplay {
    let mut totals = Totals { replays: data.len() as u64, ..Default::default() };
    let mut versions = BTreeSet::new();
    let mut replays = Vec::with_capacity(data.len());
    let mut vehicles: BTreeMap<String, Vec<&ResearchData>> = BTreeMap::new();
    let mut anomaly_totals: BTreeMap<String, u64> = BTreeMap::new();
    for (d, dir) in data.iter().zip(dirs) {
        versions.insert(d.replay.game_version.clone());
        totals.replays_ok += (d.replay.status == "ok") as u64;
        totals.flight_samples += d.flight_coverage.state_records;
        totals.tracks += d.tracks.len() as u64;
        totals.replays_with_author += d.author.is_some() as u64;
        totals.replays_with_cockpit += (d.cockpit_validation.altitude_m.n > 0) as u64;
        totals.weapons_tracked += d.weapons.stats.weapons_tracked;
        totals.weapons_by_author += d.weapons.stats.weapons_by_author;
        totals.weapons_named += d.weapons.stats.weapons_named;
        totals.kills += d.combat.kills_seen;
        if let Some(a) = &d.author {
            totals.author_tracked_seconds += a.tracked_seconds;
            for v in &a.vehicles {
                vehicles.entry(v.clone()).or_default().push(d);
            }
        }
        for a in &d.anomalies {
            *anomaly_totals.entry(a.kind.to_string()).or_default() += a.count;
        }
        replays.push(ReplayRow {
            file_name: d.replay.file_name.clone(),
            sha256: d.replay.sha256.clone(),
            game_version: d.replay.game_version.clone(),
            mission: d.replay.mission.clone(),
            status: d.replay.status,
            author_vehicles: d.author.as_ref().map(|a| a.vehicles.clone()).unwrap_or_default(),
            author_tracked_seconds: d.author.as_ref().map(|a| a.tracked_seconds),
            author_max_speed_mps: d.author.as_ref().and_then(|a| a.max_speed_mps),
            author_max_altitude_m: d.author.as_ref().and_then(|a| a.max_altitude_m),
            author_max_load_factor_g: d.author.as_ref().and_then(|a| a.max_load_factor_g),
            weapons: d.weapons.stats.weapons_tracked,
            weapons_by_author: d.weapons.stats.weapons_by_author,
            report_dir: dir.clone(),
        });
    }
    totals.author_tracked_seconds = round3(totals.author_tracked_seconds);

    let mut vehicle_rows = Vec::new();
    for (vehicle, ds) in &vehicles {
        // Per-vehicle figures use only that vehicle's tracks when a replay
        // has more than one author vehicle.
        let tracks = || ds.iter().flat_map(|d| d.tracks.iter()).filter(|t| t.role == "author" && t.identity.vehicle.as_deref() == Some(vehicle.as_str()));
        let max = |f: &dyn Fn(&crate::research::TrackResearch) -> Option<f64>| tracks().filter_map(f).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
        let per_replay_speed: Vec<f64> = ds
            .iter()
            .filter_map(|d| {
                d.tracks
                    .iter()
                    .filter(|t| t.role == "author" && t.identity.vehicle.as_deref() == Some(vehicle.as_str()))
                    .filter_map(|t| t.speed_mps.max)
                    .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))))
            })
            .collect();
        vehicle_rows.push(VehicleSummary {
            vehicle: vehicle.clone(),
            replays: ds.len() as u64,
            tracked_seconds: round3(tracks().map(|t| t.duration_s).sum()),
            max_altitude_m: max(&|t| t.altitude_m.max),
            max_speed_mps: max(&|t| t.speed_mps.max),
            max_speed_low_mps: per_replay_speed.iter().copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v)))),
            speed_saturated: tracks().any(|t| t.saturated_speed_samples > 0),
            max_load_factor_g: max(&|t| t.maneuver.as_ref().and_then(|m| m.load_factor_g.max)),
            max_climb_10s_mps: max(&|t| t.maneuver.as_ref().and_then(|m| m.climb_rate_10s_mps.max)),
            max_turn_rate_deg_s: max(&|t| t.maneuver.as_ref().and_then(|m| m.turn_rate_deg_s.max)),
        });
    }

    let mut by_weapon: BTreeMap<String, Vec<&Proposal>> = BTreeMap::new();
    for p in data.iter().flat_map(|d| d.strike_master.proposals.iter()) {
        by_weapon.entry(p.weapon_name.clone()).or_default().push(p);
    }
    let mut weapons: Vec<WeaponSummary> = by_weapon
        .into_iter()
        .map(|(weapon_name, ps)| {
            let launchers: BTreeSet<String> = ps.iter().filter_map(|p| p.vehicle.clone()).collect();
            let full = || ps.iter().filter(|p| p.samples >= MIN_SHOT_SAMPLES);
            WeaponSummary {
                s57_weapon_id: ps.iter().find_map(|p| p.s57_weapon_id.clone()),
                shots: ps.len() as u64,
                kill_linked: ps.iter().filter(|p| p.outcome == "kill_linked").count() as u64,
                launchers: launchers.into_iter().collect(),
                time_of_flight_s: plain_stat(full().filter_map(|p| p.time_of_flight_s).collect()),
                ground_range_m: plain_stat(full().map(|p| p.ground_range_m).collect()),
                max_speed_mps: plain_stat(full().map(|p| p.max_speed_mps).collect()),
                launch_altitude_m: plain_stat(ps.iter().filter_map(|p| p.launch_altitude_m).collect()),
                target_distance_at_launch_m: plain_stat(ps.iter().filter_map(|p| p.target_distance_at_launch_m).collect()),
                caution: crate::strikemaster::PROPOSAL_CAUTION,
                weapon_name,
            }
        })
        .collect();
    weapons.sort_by_key(|w| std::cmp::Reverse(w.shots));

    let consistency = vec![
        range("Altitude vs cockpit", "m", data, |d| d.cockpit_validation.altitude_m.abs_error.p50.map(|v| (d.cockpit_validation.altitude_m.n, v))),
        range("Vertical speed vs cockpit", "m/s", data, |d| d.cockpit_validation.vertical_speed_mps.abs_error.p50.map(|v| (d.cockpit_validation.vertical_speed_mps.n, v))),
        range("Roll vs cockpit", "deg", data, |d| d.cockpit_validation.roll_deg.abs_error.p50.map(|v| (d.cockpit_validation.roll_deg.n, v))),
        range("Pitch vs cockpit", "deg", data, |d| d.cockpit_validation.pitch_deg.abs_error.p50.map(|v| (d.cockpit_validation.pitch_deg.n, v))),
        range("IAS (ISA from TAS) vs cockpit", "m/s", data, |d| d.cockpit_validation.ias_isa_mps.abs_error.p50.map(|v| (d.cockpit_validation.ias_isa_mps.n, v))),
        range("Author displacement vs velocity", "m", data, |d| {
            let r = &d.kinematic_validation.author_displacement_residual_m;
            r.p50.map(|v| (r.n, v))
        }),
        range("All-aircraft displacement vs velocity", "m", data, |d| {
            let r = &d.kinematic_validation.displacement_residual_m;
            r.p50.map(|v| (r.n, v))
        }),
    ];

    let mut outliers = Vec::new();
    let series = |f: &dyn Fn(&ResearchData) -> Option<f64>| -> Vec<(String, f64)> { data.iter().filter_map(|d| f(d).map(|v| (d.replay.file_name.clone(), v))).collect() };
    outliers_for(&series(&|d| d.cockpit_validation.altitude_m.abs_error.p50), "altitude error p50 (m)", 2.0, &mut outliers);
    outliers_for(&series(&|d| d.cockpit_validation.roll_deg.abs_error.p50), "roll error p50 (deg)", 0.5, &mut outliers);
    outliers_for(&series(&|d| d.kinematic_validation.displacement_residual_m.p50), "all-aircraft displacement residual p50 (m)", 10.0, &mut outliers);
    outliers_for(&series(&|d| d.author.as_ref().and_then(|a| a.max_load_factor_g)), "author peak load factor (g)", 12.0, &mut outliers);

    let mut recommendations = vec![
        Recommendation {
            title: "Controlled LAR shots",
            detail: "Fly a test mission firing one missile type at a non-maneuvering target across a grid of launch range, altitude and aspect, including deliberate misses. Combat shots have no established target unless they killed, so only a grid gives the envelope edge.".to_string(),
        },
        Recommendation {
            title: "Synchronized 8111 recording",
            detail: "One short flight with 8111 capture and MARK events, following docs/TELEMETRY_8111.md. This fixes map north for X/Z and gives an independent instrument check.".to_string(),
        },
        Recommendation {
            title: "Level acceleration and sustained-turn runs",
            detail: "For each aircraft of interest: full-power level acceleration at fixed altitude and a sustained turn at fixed speed. SDL measures these directly, but combat sorties do not separate them.".to_string(),
        },
    ];
    let unnamed = totals.weapons_tracked.saturating_sub(totals.weapons_named);
    if unnamed > 0 {
        recommendations.push(Recommendation {
            title: "Decode the weapon type",
            detail: format!("{unnamed} of {} tracked weapons have no name because their type id has no kill in that replay. Decoding ECS rocket components would name them directly.", totals.weapons_tracked),
        });
    }
    if totals.weapons_by_author > 0 {
        recommendations.push(Recommendation {
            title: "TEST WPN cross-check",
            detail: format!("{} author releases are attributed. Compare their launch conditions with Strike Master TEST WPN predictions before any LAR change.", totals.weapons_by_author),
        });
    }
    if anomaly_totals.contains_key("speed_saturation") {
        recommendations.push(Recommendation {
            title: "Investigate speed above 500 m/s",
            detail: "The aircraft speed field saturates at 500 m/s. For fast flight use position differences, or find another speed source.".to_string(),
        });
    }

    CrossReplay {
        schema: SUMMARY_SCHEMA,
        decoder_name: DECODER_NAME,
        decoder_version: DECODER_VERSION,
        totals,
        game_versions: versions.into_iter().collect(),
        replays,
        vehicles: vehicle_rows,
        weapons,
        consistency,
        outliers,
        anomaly_totals,
        recommendations,
    }
}
