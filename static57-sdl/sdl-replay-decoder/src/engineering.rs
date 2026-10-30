//! Strike Master engineering findings: what one replay's evidence means for
//! S57-001. Findings are recommendations for review. Nothing here changes
//! Strike Master or its databases.
//!
//! Each rule below fires only when the replay holds evidence for it, so a
//! category with nothing to say is absent. Ids are built from the rule and the
//! replay hash, so they stay the same when a report is regenerated.
//!
//! Language rules, tested in `tests/engineering.rs`: replay speed is ground
//! speed, combat peaks are not limits, an unknown outcome is not a miss, and
//! no finding states a launch-envelope boundary or a success rate.

use crate::evidence::Evidence;
use crate::measure::round3;
use crate::research::{ResearchData, SourceRef};
use crate::strikemaster::Databases;
use crate::weapons::WeaponEvent;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;

pub const ENGINEERING_SCHEMA: &str = "static57.sdl.engineering-findings/0.1";
/// Fewest samples before a compass or instrument figure means anything.
const MIN_CHECKS: u64 = 100;
/// Clock rate error beyond this is worth a look: it affects alignment.
const CLOCK_RATE_TOLERANCE: f64 = 1e-4;
/// Burn rates this far apart on one aircraft need an explanation.
const FUEL_SPREAD_RATIO: f64 = 1.5;
/// A nibble needs this many non-zero samples to count as exercised.
const MIN_ACTIVE_NIBBLE: u64 = 50;
/// Fewest shots of a type before its trajectories are a research opportunity.
const MIN_TYPE_SHOTS: usize = 3;
/// Airborne stretches of fuel use shorter than this are too noisy to compare.
const MIN_FUEL_SECONDS: f64 = 120.0;
/// A stretch this much in the air counts as flight, not ground time.
const MIN_AIRBORNE_FRACTION: f64 = 0.5;

const LIMIT_NO_8111: &str = "No synchronized 8111 capture covers this flight, so this has no check from outside the replay file.";
const LIMIT_COCKPIT: &str = "The cockpit message comes from the same replay file, so agreement shows internal consistency, not independent validation.";
const LIMIT_REALISM: &str = "Agreement between replay and telemetry validates decoding, not real-world aircraft realism.";
const LIMIT_COMBAT: &str = "Combat peaks are lower bounds on capability, not aircraft performance limits. Configuration, weather and pilot intent are unknown.";
const LIMIT_OUTCOME: &str = "An unknown outcome means no record supports a conclusion. It is not a miss, and it says nothing about tracking.";
const LIMIT_SAMPLE: &str = "Weapon sample spacing varies with network relevance, and the last sample is not an established impact.";
const LIMIT_SELECTION: &str = "Linked shots favour weapons that killed, so these shots are not a representative sample.";

/// Priority by evidence quality and engineering impact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Priority {
    High,
    Medium,
    Low,
}

impl Priority {
    pub fn label(self) -> &'static str {
        match self {
            Priority::High => "HIGH",
            Priority::Medium => "MEDIUM",
            Priority::Low => "LOW",
        }
    }
}

pub fn evidence_label(e: Evidence) -> &'static str {
    match e {
        Evidence::Verified => "VERIFIED",
        Evidence::Reported => "REPORTED",
        Evidence::Derived => "DERIVED",
        Evidence::Experimental => "EXPERIMENTAL",
        Evidence::Unknown => "UNKNOWN",
    }
}

/// What kind of thing a finding is. Only `software_error` calls something a
/// bug, and it needs evidence that SDL itself failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Nature {
    Measurement,
    ResearchOpportunity,
    DataGap,
    UnresolvedDiscrepancy,
    SoftwareError,
}

impl Nature {
    pub fn label(self) -> &'static str {
        match self {
            Nature::Measurement => "measurement",
            Nature::ResearchOpportunity => "research opportunity",
            Nature::DataGap => "data gap",
            Nature::UnresolvedDiscrepancy => "unresolved discrepancy",
            Nature::SoftwareError => "software error",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayReference {
    pub file_name: String,
    pub sha256: String,
    pub session_id_hex: Option<String>,
    pub start_time_utc: Option<String>,
    pub mission: Option<String>,
    pub events: Vec<SourceRef>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelemetryReference {
    pub validation_file: &'static str,
    pub capture_dir: Option<String>,
    pub capture_sha256: Option<String>,
    pub independent_validation: bool,
    pub alignment_quality: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineeringFinding {
    pub finding_id: String,
    pub subsystem: &'static str,
    pub title: String,
    pub aircraft_id: Option<String>,
    pub s57_aircraft_id: Option<String>,
    pub weapon_id: Option<String>,
    pub s57_weapon_id: Option<String>,
    pub observation: String,
    pub derived_analysis: String,
    /// `null` for a measurement this replay does not have.
    pub measured_values: BTreeMap<String, Option<f64>>,
    pub units: BTreeMap<String, String>,
    #[serde(serialize_with = "evidence_field")]
    pub evidence_class: Evidence,
    pub confidence: &'static str,
    pub nature: Nature,
    pub limitations: Vec<String>,
    pub engineering_relevance: String,
    pub priority: Priority,
    pub recommended_follow_up: Option<String>,
    pub replay_reference: ReplayReference,
    pub telemetry_reference: Option<TelemetryReference>,
    pub game_version: Option<String>,
}

fn evidence_field<S: Serializer>(e: &Evidence, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(evidence_label(*e))
}

/// The findings of one replay, plus the summary lines built from them.
#[derive(Debug, Clone, Serialize)]
pub struct EngineeringSection {
    pub schema: &'static str,
    pub status: &'static str,
    pub executive_summary: Vec<String>,
    pub findings: Vec<EngineeringFinding>,
}

impl Default for EngineeringSection {
    fn default() -> Self {
        EngineeringSection { schema: ENGINEERING_SCHEMA, status: "proposed_for_review", executive_summary: Vec::new(), findings: Vec::new() }
    }
}

struct Draft {
    code: String,
    subsystem: &'static str,
    title: String,
    weapon: Option<String>,
    observation: String,
    analysis: String,
    values: Vec<(&'static str, Option<f64>, &'static str)>,
    evidence: Evidence,
    confidence: &'static str,
    nature: Nature,
    limitations: Vec<String>,
    relevance: String,
    priority: Priority,
    follow_up: Option<String>,
    events: Vec<SourceRef>,
}

impl Draft {
    fn new(code: impl Into<String>, subsystem: &'static str, priority: Priority, title: impl Into<String>) -> Self {
        Draft {
            code: code.into(),
            subsystem,
            title: title.into(),
            weapon: None,
            observation: String::new(),
            analysis: String::new(),
            values: Vec::new(),
            evidence: Evidence::Derived,
            confidence: "medium",
            nature: Nature::Measurement,
            limitations: Vec::new(),
            relevance: String::new(),
            priority,
            follow_up: None,
            events: Vec::new(),
        }
    }
}

struct Ctx<'a> {
    d: &'a ResearchData,
    db: Option<&'a Databases>,
    validation: Option<&'a serde_json::Value>,
    vehicle: Option<String>,
    s57_aircraft: Option<String>,
    out: Vec<EngineeringFinding>,
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

fn f3(v: Option<f64>) -> String {
    v.map_or("unavailable".to_string(), |x| format!("{}", round3(x)))
}

fn range_of(v: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    v.fold(None, |a: Option<(f64, f64)>, x| Some(a.map_or((x, x), |(l, h)| (l.min(x), h.max(x)))))
}

fn range_text(r: Option<(f64, f64)>, unit: &str) -> String {
    r.map_or("unavailable".to_string(), |(a, b)| format!("{} to {} {unit}", round3(a), round3(b)).trim_end().to_string())
}

impl Ctx<'_> {
    fn independent(&self) -> bool {
        self.validation.and_then(|v| v["independent_validation"].as_bool()).unwrap_or(false)
    }

    fn comparison(&self, name: &str) -> Option<&serde_json::Value> {
        self.validation?["comparisons"].as_array()?.iter().find(|c| c["name"] == name)
    }

    fn no_8111(&self) -> Vec<String> {
        if self.validation.is_some() { Vec::new() } else { vec![LIMIT_NO_8111.to_string()] }
    }

    fn add(&mut self, f: Draft) {
        let r = &self.d.replay;
        let (mut measured_values, mut units) = (BTreeMap::new(), BTreeMap::new());
        for (k, v, u) in &f.values {
            measured_values.insert(k.to_string(), v.map(round3));
            if !u.is_empty() {
                units.insert(k.to_string(), u.to_string());
            }
        }
        let telemetry_reference = self.validation.map(|v| TelemetryReference {
            validation_file: "validation-data.json",
            capture_dir: v["provenance"]["capture_dir"].as_str().map(String::from),
            capture_sha256: v["provenance"]["capture_sha256"].as_str().map(String::from),
            independent_validation: self.independent(),
            alignment_quality: v["sync"]["quality"].as_str().map(String::from),
        });
        let s57_weapon_id = f.weapon.as_deref().and_then(|w| self.db.and_then(|d| d.weapon_id(w))).map(String::from);
        self.out.push(EngineeringFinding {
            finding_id: format!("{}-{}", f.code, &r.sha256[..8.min(r.sha256.len())]),
            subsystem: f.subsystem,
            title: f.title,
            aircraft_id: self.vehicle.clone(),
            s57_aircraft_id: self.s57_aircraft.clone(),
            weapon_id: f.weapon,
            s57_weapon_id,
            observation: f.observation,
            derived_analysis: f.analysis,
            measured_values,
            units,
            evidence_class: f.evidence,
            confidence: f.confidence,
            nature: f.nature,
            limitations: f.limitations,
            engineering_relevance: f.relevance,
            priority: f.priority,
            recommended_follow_up: f.follow_up,
            replay_reference: ReplayReference { file_name: r.file_name.clone(), sha256: r.sha256.clone(), session_id_hex: r.session_id_hex.clone(), start_time_utc: r.start_time_utc.clone(), mission: r.mission.clone(), events: f.events },
            telemetry_reference,
            game_version: (!r.game_version.is_empty()).then(|| r.game_version.clone()),
        });
    }

    fn author_events(&self, kind: &str) -> Vec<SourceRef> {
        self.d.tracks.iter().filter(|t| t.role == "author").flat_map(|t| t.transitions.iter().filter(|x| x.kind == kind).map(|x| SourceRef { subject: t.track_id.clone(), packet_index: Some(x.packet_index), timestamp_ms: Some(x.timestamp_ms) })).collect()
    }

    /// Decoding that itself failed. This is the only rule that says "error".
    fn decode_status(&mut self) {
        let r = &self.d.replay;
        let failures = self.d.flight_coverage.structural_failures;
        if r.status == "ok" && failures == 0 {
            return;
        }
        let mut f = Draft::new("SDL-DECODE", "FLT", Priority::High, "Replay did not decode completely");
        f.nature = Nature::SoftwareError;
        f.evidence = Evidence::Verified;
        f.confidence = "high";
        f.observation = format!("Decode status is {}, with {failures} aircraft packets failing structural parsing.", r.status);
        f.analysis = "SDL's own structural accounting reports the failure, so figures from this replay may be incomplete.".into();
        f.values = vec![("structural_failures", Some(failures as f64), "")];
        f.relevance = "Strike Master should not use figures from a partly decoded replay.".into();
        f.follow_up = Some("Inspect the replay's diagnostics and check its format against docs/COMPATIBILITY.md.".into());
        self.add(f);
    }

    fn map_frame(&mut self) {
        let c = &self.d.systems.compass;
        if c.n < MIN_CHECKS {
            return;
        }
        let horizontal = self.validation.map(|v| &v["horizontal"]).filter(|h| h["accepted"].as_bool() == Some(true));
        let mut f = Draft::new("MAP-FRAME", "MAP", Priority::Low, "World frame against compass and map");
        f.observation = format!("Cockpit compass minus (decoded heading + 90) has a median of {} deg over {} samples.", f3(c.median_error_deg), c.n);
        f.values = vec![("compass_error_median", c.median_error_deg, "deg"), ("compass_abs_error_p50", c.abs_error_deg.p50, "deg"), ("compass_abs_error_p99", c.abs_error_deg.p99, "deg"), ("map_fit_rms", horizontal.and_then(|h| h["best_rms_m"].as_f64()), "m")];
        if let Some(h) = horizontal {
            f.analysis = format!("A real 8111 capture accepted the transform `{}` at {} m RMS on this flight, so +X is east and +Z is north here.", h["best"].as_str().unwrap_or("?"), f3(h["best_rms_m"].as_f64()));
            f.confidence = "high";
            f.limitations = vec![LIMIT_REALISM.to_string()];
        } else {
            f.analysis = "A heading offset of exactly 90 deg agrees with +X east and +Z north, the frame already confirmed on other flights. Without 8111 this replay cannot confirm its own map scale.".into();
            f.limitations = self.no_8111();
            f.limitations.push(LIMIT_COCKPIT.to_string());
        }
        f.relevance = "Moving map and bearing calculations in Strike Master assume x east and y down in map coordinates.".into();
        self.add(f);
    }

    fn hud_instruments(&mut self) {
        let c = &self.d.cockpit_validation;
        if c.matched < MIN_CHECKS {
            return;
        }
        let ids = &self.d.systems.cockpit_id_samples;
        let names = [("vertical speed", crate::cockpit::id::VERTICAL_SPEED, c.vertical_speed_mps.n), ("indicated airspeed", crate::cockpit::id::AIRSPEED, c.ias_isa_mps.n), ("roll", crate::cockpit::id::ROLL, c.roll_deg.n), ("pitch", crate::cockpit::id::PITCH, c.pitch_deg.n)];
        let missing: Vec<&str> = names.iter().filter(|x| x.2 == 0 && !ids.contains_key(&x.1)).map(|x| x.0).collect();
        let unmatched: Vec<&str> = names.iter().filter(|x| x.2 == 0 && ids.contains_key(&x.1)).map(|x| x.0).collect();
        let mut f = Draft::new("HUD-INSTR", "HUD", Priority::Low, "Replay against the cockpit instruments");
        f.observation = format!("Over {} matched samples, altitude agrees to {} m, pitch to {} deg and roll to {} deg (median absolute error).", c.matched, f3(c.altitude_m.abs_error.p50), f3(c.pitch_deg.abs_error.p50), f3(c.roll_deg.abs_error.p50));
        f.analysis = if missing.is_empty() && unmatched.is_empty() { "Every instrument SDL checks was compared.".to_string() } else if unmatched.is_empty() { format!("No comparison exists for {}: the cockpit ids for those instruments never appear in this replay.", missing.join(" and ")) } else { format!("{} present but produced no comparison, and the cause is not established. Absent: {}.", unmatched.join(" and "), if missing.is_empty() { "none".to_string() } else { missing.join(" and ") }) };
        f.values = vec![("altitude_abs_error_p50", c.altitude_m.abs_error.p50, "m"), ("altitude_abs_error_p99", c.altitude_m.abs_error.p99, "m"), ("pitch_abs_error_p50", c.pitch_deg.abs_error.p50, "deg"), ("roll_abs_error_p50", c.roll_deg.abs_error.p50, "deg"), ("vertical_speed_abs_error_p50", c.vertical_speed_mps.abs_error.p50, "m/s"), ("ias_estimate_abs_error_p50", c.ias_isa_mps.abs_error.p50, "m/s")];
        f.limitations = vec![LIMIT_COCKPIT.to_string()];
        f.limitations.extend(self.no_8111());
        f.relevance = "Decoded altitude and attitude can act as reference values when checking HUD indications.".into();
        if !missing.is_empty() || !unmatched.is_empty() {
            f.nature = Nature::DataGap;
            f.relevance.push_str(" Strike Master should keep treating these instruments as optional per aircraft.");
        }
        self.add(f);
    }

    fn hud_feet(&mut self) {
        let c = &self.d.cockpit_validation;
        if c.altimeter_unit != Some("ft") || c.altitude_m.n < MIN_CHECKS {
            return;
        }
        let mut f = Draft::new("HUD-ALTFT", "HUD", Priority::Medium, "Feet altimeter uses the game's foot");
        f.nature = Nature::UnresolvedDiscrepancy;
        f.observation = format!("This aircraft's cockpit altimeter reads feet (parameter {}). Converted at 3.28 ft/m it agrees with decoded altitude to {} m (median).", c.altimeter_param_id.map_or("?".into(), |i| i.to_string()), f3(c.altitude_m.abs_error.p50));
        f.analysis = "At 3.28 ft/m against the exact 3.28084 ft/m, a feet reading differs by about 8 ft at 30,000 ft. This is a units difference to settle, and nothing here shows either side is wrong.".into();
        f.values = vec![("altitude_abs_error_p50", c.altitude_m.abs_error.p50, "m"), ("game_feet_per_metre", Some(3.28), "ft/m"), ("exact_feet_per_metre", Some(3.28084), "ft/m")];
        f.limitations = self.no_8111();
        f.limitations.push(LIMIT_COCKPIT.to_string());
        f.relevance = "Strike Master's `src/shared/units.ts` uses 3.28084 ft/m. A HUD in feet may differ slightly from this aircraft's own altimeter.".into();
        f.follow_up = Some("Compare 8111 `altitude_*` with `H, m` the next time a feet-altimeter aircraft is captured, then decide whether the HUD should match the cockpit or exact units.".into());
        self.add(f);
    }

    fn hud_airspeed(&mut self) {
        let (Some(ias), Some(gs)) = (self.comparison("indicated_airspeed"), self.comparison("ground_speed_minus_tas")) else { return };
        let (ias_p50, gs_bias, gs_p99) = (ias["abs_error"]["p50"].as_f64(), gs["bias"].as_f64(), gs["abs_error"]["p99"].as_f64());
        let mut f = Draft::new("HUD-SPEED", "HUD", Priority::Medium, "IAS, TAS and ground speed measured separately");
        f.confidence = "high";
        f.observation = format!("Against real 8111 data, replay cockpit IAS agrees to {} m/s (median). Replay ground speed minus 8111 TAS has a bias of {} m/s.", f3(ias_p50), f3(gs_bias));
        f.analysis = "The ground speed and TAS difference is an observed difference between two quantities, not a decoding error. Wind is one candidate cause and has not been measured.".into();
        f.values = vec![("ias_abs_error_p50", ias_p50, "m/s"), ("ground_speed_minus_tas_bias", gs_bias, "m/s"), ("ground_speed_minus_tas_abs_p99", gs_p99, "m/s")];
        f.limitations = vec![LIMIT_REALISM.to_string(), "8111 speeds are whole km/h, about 0.28 m/s.".to_string()];
        f.relevance = "HUD and mission timing should keep IAS, TAS and ground speed as separate labelled values.".into();
        f.follow_up = Some("Repeat on other maps to see whether the bias follows wind or altitude.".into());
        self.add(f);
    }

    fn hud_clock(&mut self) {
        let c = &self.d.systems.clock;
        let Some(rate) = c.rate.filter(|r| (r - 1.0).abs() > CLOCK_RATE_TOLERANCE && c.span_s >= 300.0) else { return };
        let mut f = Draft::new("HUD-CLOCK", "HUD", Priority::Low, "Cockpit clock rate differs from replay time");
        f.nature = Nature::UnresolvedDiscrepancy;
        f.observation = format!("The cockpit clock advanced at {rate:.6} clock seconds per replay second over {} s.", round3(c.span_s));
        f.analysis = "Most replays read 1.000000. A rate this far off may come from a clock step during the flight. The cause is not established.".into();
        f.values = vec![("clock_rate", Some(rate), "s/s"), ("span", Some(c.span_s), "s")];
        f.limitations = vec![LIMIT_COCKPIT.to_string()];
        f.relevance = "Clock-based alignment with 8111 depends on this clock running steadily.".into();
        self.add(f);
    }

    fn fuel(&mut self) {
        let vehicle = self.vehicle.clone().unwrap_or_default();
        let validated = self.independent() || crate::systems::FUEL_MASS_MATCHED.contains(&vehicle.as_str());
        let flights: Vec<_> = self.d.systems.fuel.iter().filter(|s| s.duration_s >= MIN_FUEL_SECONDS && s.airborne_fraction >= MIN_AIRBORNE_FRACTION && s.rate_per_s < 0.0).cloned().collect();
        if flights.is_empty() {
            return;
        }
        let burns: Vec<f64> = flights.iter().map(|s| -s.rate_per_s).collect();
        let (lo, hi) = range_of(burns.iter().copied()).unwrap();
        let spread = (lo > 0.0).then(|| hi / lo);
        let varies = flights.len() >= 2 && spread.is_some_and(|s| s >= FUEL_SPREAD_RATIO);
        let mut f = Draft::new("FLT-FUEL", "FLT", if varies { Priority::Medium } else { Priority::Low }, if varies { "Fuel burn varied between stretches" } else { "Fuel burn observed" });
        f.nature = if varies { Nature::ResearchOpportunity } else { Nature::Measurement };
        f.evidence = if validated { Evidence::Derived } else { Evidence::Experimental };
        let rows: Vec<String> = flights.iter().map(|s| format!("{} kg/s over {:.0} s (mean throttle {}, {:.0}% of samples above 100% throttle, mean altitude {} m, mean ground speed {} m/s)", round3(-s.rate_per_s), s.duration_s, f3(s.mean_throttle), s.above_100_percent_fraction * 100.0, s.mean_altitude_m.map_or("unavailable".to_string(), |x| format!("{x:.0}")), s.mean_speed_mps.map_or("unavailable".to_string(), |x| format!("{x:.0}")))).collect();
        f.observation = format!("{} flight stretches burned {}.", flights.len(), rows.join("; "));
        let above: Vec<(f64, f64)> = flights.iter().map(|s| (s.above_100_percent_fraction, -s.rate_per_s)).collect();
        let r = crate::systems::pearson(&above).filter(|_| above.len() >= 3);
        f.analysis = if varies { format!("Burn differs by a factor of {} between stretches. Time above 100% throttle, altitude and speed all varied{}, so no cause is established and no burn model is implied.", f3(spread), r.map_or(String::new(), |r| format!(" (r {} across only {} stretches)", f3(Some(r)), above.len()))) } else { "One figure for the sortie. It does not show how burn depends on any variable.".to_string() };
        f.values = vec![("burn_min", Some(lo), "kg/s"), ("burn_max", Some(hi), "kg/s"), ("burn_ratio", spread, ""), ("stretches", Some(flights.len() as f64), ""), ("above_100_share_max", range_of(flights.iter().map(|s| s.above_100_percent_fraction)).map(|x| x.1), "")];
        f.limitations = vec!["Averages over combat flying, including time on the ground, with changing altitude and throttle.".to_string()];
        if !validated {
            f.limitations.push("Cockpit id 195 was matched to 8111 fuel mass on other aircraft, not this one, so kg is assumed here.".to_string());
        }
        f.limitations.extend(self.no_8111());
        f.relevance = "Fuel prediction needs burn as a function of throttle, altitude and speed, and every replay holds a fuel history.".into();
        f.follow_up = Some("Pool this aircraft's replays, separate time above 100% throttle, and compare burn with Strike Master's fuel logic.".into());
        self.add(f);
    }

    fn engine(&mut self) {
        let y = &self.d.systems;
        if y.throttle_nibble_agree.n < MIN_CHECKS {
            return;
        }
        let share = (y.samples > 0).then(|| y.throttle_boost_samples as f64 / y.samples as f64);
        let agree = y.throttle_nibble_agree.agree as f64 / y.throttle_nibble_agree.n as f64;
        let states: Vec<String> = y.engine_states.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        let mut f = Draft::new("FLT-ENGINE", "FLT", Priority::Low, "Throttle and engine state");
        f.observation = format!("The throttle nibble matches cockpit throttle in {:.1}% of {} samples. Engine state samples are {}.", agree * 100.0, y.throttle_nibble_agree.n, states.join(", "));
        f.analysis = format!("Throttle above 100% appears in {} of {} author samples, with engine-field error of {} (p99) against the cockpit value.", y.throttle_boost_samples, y.samples, f3(y.throttle_abs_error.p99));
        f.values = vec![("throttle_nibble_agreement", Some(agree), ""), ("above_100_percent_share", share, ""), ("boost_abs_error_p99", y.throttle_abs_error.p99, "")];
        f.limitations = vec!["Meanings are DERIVED from three aircraft compared with 8111, so this aircraft is an extrapolation unless it is one of them.".to_string()];
        f.limitations.extend(self.no_8111());
        f.relevance = "Throttle history supports fuel and performance work, and needs no 8111 capture once its meaning is established.".into();
        self.add(f);
    }

    fn unidentified_fields(&mut self) {
        let y = &self.d.systems;
        let active: Vec<(&str, u64)> = [("n5", "nibble 5"), ("n8", "nibble 8"), ("n10", "nibble 10")].iter().filter_map(|(k, n)| y.nibble_nonzero.get(*k).filter(|v| **v >= MIN_ACTIVE_NIBBLE).map(|v| (*n, *v))).collect();
        if active.is_empty() {
            return;
        }
        let list: Vec<String> = active.iter().map(|(n, v)| format!("{n} ({v} samples)")).collect();
        let captured = self.validation.is_some();
        let mut f = Draft::new("FLT-FIELDS", "FLT", Priority::Medium, "Unidentified control fields were exercised");
        f.nature = Nature::ResearchOpportunity;
        f.evidence = Evidence::Unknown;
        f.observation = format!("Control fields with no established meaning were active: {}.", list.join(", "));
        f.analysis = if captured { "A capture covers this flight, so the field scan in `validation-data.json` can test these fields.".to_string() } else { "These fields changed in this flight, but no independent signal exists to identify them, so no meaning is assigned.".to_string() };
        f.values = vec![("nibble_5_samples", y.nibble_nonzero.get("n5").map(|v| *v as f64), ""), ("nibble_8_samples", y.nibble_nonzero.get("n8").map(|v| *v as f64), ""), ("nibble_10_samples", y.nibble_nonzero.get("n10").map(|v| *v as f64), "")];
        f.limitations = self.no_8111();
        f.relevance = "If these are airbrake or similar configuration fields, replays could give configuration history for performance analysis.".into();
        f.follow_up = Some("Capture 8111 during the next flight of this aircraft: `telemetry-validate` then scores these fields automatically.".into());
        self.add(f);
    }

    fn aircraft_performance(&mut self) {
        let (Some(a), Some(v)) = (self.d.author.clone(), self.vehicle.clone()) else { return };
        let y = &self.d.systems;
        if y.airborne_seconds < MIN_FUEL_SECONDS {
            return;
        }
        let mut f = Draft::new("ACDB-PERF", "AIRCRAFT_DB", Priority::Low, "Observed combat flight envelope");
        f.observation = format!("Over {:.0} s above 60 m/s, `{v}` flew at a median {} m/s ground speed and {} m altitude, and reached {} m/s ground speed, {} m, {} g (1 s) and {} deg/s.", y.airborne_seconds, f3(y.airborne_speed_mps.p50), f3(y.airborne_altitude_m.p50), f3(a.max_speed_mps), f3(a.max_altitude_m), f3(a.max_load_factor_g), f3(a.max_turn_rate_deg_s));
        f.analysis = "The p10 to p90 band shows where this sortie was flown. The maximums show values reached, and they do not bound what the aircraft can do.".into();
        f.values = vec![("airborne_time", Some(y.airborne_seconds), "s"), ("ground_speed_p10", y.airborne_speed_mps.p10, "m/s"), ("ground_speed_p50", y.airborne_speed_mps.p50, "m/s"), ("ground_speed_p90", y.airborne_speed_mps.p90, "m/s"), ("altitude_p50", y.airborne_altitude_m.p50, "m"), ("max_ground_speed", a.max_speed_mps, "m/s"), ("max_altitude", a.max_altitude_m, "m"), ("max_load_factor_1s", a.max_load_factor_g, "g"), ("max_turn_rate", a.max_turn_rate_deg_s, "deg/s"), ("max_climb_10s", a.max_climb_10s_mps, "m/s")];
        f.limitations = vec![LIMIT_COMBAT.to_string(), "Replay speed is ground speed, not TAS or IAS.".to_string()];
        f.relevance = "Shows which part of the aircraft database envelope real sorties use.".into();
        self.add(f);
    }

    fn aircraft_database(&mut self) {
        let (Some(_), Some(v)) = (self.db, self.vehicle.clone()) else { return };
        if self.s57_aircraft.is_some() {
            return;
        }
        let mut f = Draft::new("ACDB-MATCH", "AIRCRAFT_DB", Priority::Medium, "Aircraft not found in the database");
        f.nature = Nature::UnresolvedDiscrepancy;
        f.evidence = Evidence::Experimental;
        let count = self.db.map_or(0, |d| d.aircraft_count());
        f.observation = format!("`{v}` matches none of the {count} aircraft entries by wiki slug.");
        f.analysis = "SDL matches on the slug in `source_url`, so a differently named entry would also read as absent.".into();
        f.values = vec![("database_aircraft", Some(count as f64), "")];
        f.limitations = vec!["The database may be a deliberate subset, so absence is not a fault in itself.".to_string()];
        f.relevance = "An unmatched aircraft cannot link replay evidence to its database entry.".into();
        f.follow_up = Some("Review whether this aircraft should have an entry or a slug alias.".into());
        self.add(f);
    }

    fn weapons(&mut self) {
        let authored: Vec<WeaponEvent> = self.d.weapons.events.iter().filter(|w| w.launcher.is_author).cloned().collect();
        if authored.is_empty() {
            return;
        }
        let mut by: BTreeMap<String, Vec<WeaponEvent>> = BTreeMap::new();
        for w in authored.iter() {
            by.entry(w.weapon_name.as_deref().map_or("unnamed".to_string(), |n| crate::weapons::base_name(n).to_string())).or_default().push(w.clone());
        }
        for (name, group) in &by {
            self.weapon_type(name, group);
            self.launch_geometry(name, group);
            self.weapon_database(name, group);
        }
        self.targeting(&authored);
    }

    fn weapon_type(&mut self, name: &str, g: &[WeaponEvent]) {
        let count = |c: &str| g.iter().filter(|w| w.outcome.class == c).count();
        let (kills, unknown) = (count("confirmed_kill"), count("unknown"));
        let others = g.len() - kills - unknown;
        let named = name != "unnamed";
        let tof = range_of(g.iter().filter_map(|w| w.trajectory.time_of_flight_s));
        let alt = range_of(g.iter().filter_map(|w| w.trajectory.launch_altitude_m.map(|a| a as f64)));
        let spd = range_of(g.iter().filter_map(|w| w.launcher.aircraft_speed_mps));
        let peak = range_of(g.iter().map(|w| w.trajectory.max_speed_mps));
        let path = range_of(g.iter().map(|w| w.trajectory.path_length_m));
        let mean_speed = range_of(g.iter().filter_map(|w| w.trajectory.time_of_flight_s.filter(|t| *t > 0.0).map(|t| w.trajectory.path_length_m / t)));
        let targets = g.iter().filter(|w| w.target.status != "unavailable").count();
        let priority = if g.len() >= MIN_TYPE_SHOTS { Priority::Medium } else { Priority::Low };
        let mut f = Draft::new(format!("WPN-{}", safe(name)), "WPN", priority, format!("Observed trajectories: {name}"));
        f.weapon = named.then(|| name.to_string());
        f.nature = if priority == Priority::Medium { Nature::ResearchOpportunity } else { Nature::Measurement };
        f.observation = format!("{} author shots: {kills} linked to a kill, {others} other records, {unknown} with an unknown outcome, {targets} with an established target. Launch altitude {}, launcher ground speed {}.", g.len(), range_text(alt, "m"), range_text(spd, "m/s"));
        f.analysis = format!("Time of flight {}, path length {}, mean path speed {}, peak speed {}.", range_text(tof, "s"), range_text(path, "m"), range_text(mean_speed, "m/s"), range_text(peak, "m/s"));
        if unknown == g.len() {
            f.analysis.push_str(" No record links any of these shots to a result, so this says nothing about guidance or tracking.");
        }
        f.values = vec![("shots", Some(g.len() as f64), ""), ("linked_kills", Some(kills as f64), ""), ("unknown_outcomes", Some(unknown as f64), ""), ("established_targets", Some(targets as f64), ""), ("time_of_flight_min", tof.map(|x| x.0), "s"), ("time_of_flight_max", tof.map(|x| x.1), "s"), ("path_length_max", path.map(|x| x.1), "m"), ("peak_speed_max", peak.map(|x| x.1), "m/s")];
        f.limitations = vec![LIMIT_OUTCOME.to_string(), LIMIT_SAMPLE.to_string()];
        if named && g.iter().any(|w| w.weapon_name_source == "replay_type_id") {
            f.limitations.push("Some names come from a shared type id, so they are inferred, not recorded per shot.".to_string());
        }
        f.relevance = "Measured time of flight and speed profiles can be compared with the kinematic model behind Strike Master's weapon predictions.".into();
        f.follow_up = (priority == Priority::Medium).then(|| "Compare these trajectories offline with Strike Master's weapon model at the same launch conditions.".to_string());
        f.events = g.iter().take(4).map(|w| SourceRef { subject: w.weapon_id.clone(), packet_index: Some(w.first_packet_index), timestamp_ms: Some(w.trajectory.first_ms) }).collect();
        self.add(f);
    }

    fn launch_geometry(&mut self, name: &str, g: &[WeaponEvent]) {
        let with: Vec<&WeaponEvent> = g.iter().filter(|w| w.geometry.is_some()).collect();
        if with.len() < 2 {
            return;
        }
        let range = range_of(with.iter().filter_map(|w| w.geometry.as_ref()).map(|x| x.target_distance_m));
        let aspect = range_of(with.iter().filter_map(|w| w.geometry.as_ref().and_then(|x| x.target_aspect_deg)));
        let closing = range_of(with.iter().filter_map(|w| w.geometry.as_ref().and_then(|x| x.closing_speed_mps)));
        let kills = with.iter().filter(|w| w.outcome.class == "confirmed_kill").count();
        let mut f = Draft::new(format!("LAR-{}", safe(name)), "LAR", Priority::Medium, format!("Launch geometry samples: {name}"));
        f.weapon = (name != "unnamed").then(|| name.to_string());
        f.nature = Nature::ResearchOpportunity;
        f.observation = format!("{} shots have launch geometry: target range {}, aspect {}, closing speed {}. {kills} are linked to a kill.", with.len(), range_text(range, "m"), range_text(aspect, "deg"), range_text(closing, "m/s"));
        f.analysis = "These are individual launch conditions and describe only what happened on these shots. They mark no envelope edge.".into();
        f.values = vec![("geometry_shots", Some(with.len() as f64), ""), ("linked_kills", Some(kills as f64), ""), ("target_range_min", range.map(|x| x.0), "m"), ("target_range_max", range.map(|x| x.1), "m"), ("aspect_min", aspect.map(|x| x.0), "deg"), ("aspect_max", aspect.map(|x| x.1), "deg"), ("closing_speed_max", closing.map(|x| x.1), "m/s")];
        f.limitations = vec![LIMIT_SELECTION.to_string(), LIMIT_OUTCOME.to_string(), "Targets are inferred associations, and target manoeuvre after launch is not modelled here.".to_string(), "No launch-envelope boundary or success rate can be drawn from these shots. LAR calibration readiness is not claimed.".to_string()];
        f.relevance = "Each shot is one labelled condition for later comparison with LAR predictions, once a validated outcome method exists.".into();
        f.follow_up = Some("Keep these records as candidate comparison points for an offline LAR model review.".into());
        f.events = with.iter().take(4).map(|w| SourceRef { subject: w.weapon_id.clone(), packet_index: Some(w.first_packet_index), timestamp_ms: Some(w.trajectory.first_ms) }).collect();
        self.add(f);
    }

    fn weapon_database(&mut self, name: &str, g: &[WeaponEvent]) {
        let Some(db) = self.db else { return };
        if name == "unnamed" {
            let mut f = Draft::new("WDB-UNNAMED", "WEAPONS_DB", Priority::Low, "Author weapons with no name");
            f.nature = Nature::DataGap;
            f.observation = format!("{} author weapons have no name, because their type id has no kill record in this replay.", g.len());
            f.analysis = "Their identity remains missing. SDL does not guess it from loadout or trajectory.".into();
            f.values = vec![("unnamed_shots", Some(g.len() as f64), "")];
            f.limitations = vec!["Type ids are per replay, so pooling replays cannot name them.".to_string()];
            f.relevance = "Unnamed shots cannot be linked to the weapons database.".into();
            self.add(f);
            return;
        }
        let id = db.weapon_id(name);
        let carried = id.zip(self.s57_aircraft.as_deref()).and_then(|(w, a)| db.aircraft_carries(a, w));
        if id.is_some() && carried != Some(false) {
            return;
        }
        let vehicle = self.vehicle.clone().unwrap_or_default();
        let linked = g.iter().filter(|w| w.weapon_name_source == "kill_record").count();
        let (title, observed, relevance, priority) = match id {
            None if db.weapon_match_count(name) > 1 => (format!("Weapon matches several database entries: {name}"), format!("`{name}` was fired {} times and matches {} database entries.", g.len(), db.weapon_match_count(name)), "An ambiguous match cannot receive measured data until it is resolved.", Priority::Medium),
            None => (format!("Weapon not found in the database: {name}"), format!("`{name}` was fired {} times and matches no database entry.", g.len()), "A weapon with no database entry cannot receive measured data.", Priority::Medium),
            Some(w) => (format!("Observed weapon not listed for this aircraft: {name}"), format!("`{vehicle}` fired `{name}` ({w}) {} times, but its database entry does not list that weapon.", g.len()), "The aircraft entry drives weapon selection, so a missing compatibility entry could hide a store the aircraft carries.", if linked > 0 { Priority::High } else { Priority::Medium }),
        };
        let mut f = Draft::new(format!("WDB-{}", safe(name)), "WEAPONS_DB", priority, title);
        f.weapon = Some(name.to_string());
        f.nature = Nature::UnresolvedDiscrepancy;
        f.observation = observed;
        f.analysis = format!("{linked} of {} shots carry the name from a kill record, and the rest from a shared type id. SDL matches by designator, so this could also be a naming difference.", g.len());
        f.values = vec![("shots", Some(g.len() as f64), ""), ("named_by_kill_record", Some(linked as f64), "")];
        f.evidence = Evidence::Experimental;
        f.limitations = vec!["Database matching is by name, and no database was changed. The database may be a deliberate subset.".to_string()];
        f.relevance = relevance.to_string();
        f.follow_up = Some("Review the database entry against the wiki loadout. Any change stays the owner's decision.".into());
        self.add(f);
    }

    fn targeting(&mut self, authored: &[WeaponEvent]) {
        if authored.len() < MIN_TYPE_SHOTS {
            return;
        }
        let seeker = authored.iter().filter(|w| w.target.status == "seeker").count();
        let kill = authored.iter().filter(|w| w.target.status == "kill_record").count();
        let none = authored.iter().filter(|w| w.target.status == "unavailable").count();
        let candidates = authored.iter().filter(|w| w.target.designated_candidate_uid.is_some()).count();
        let mut f = Draft::new("STRK-TARGET", "STRK", Priority::Low, "Target evidence for author shots");
        f.nature = if none > 0 { Nature::DataGap } else { Nature::Measurement };
        f.observation = format!("Of {} author shots, {seeker} have a seeker-based target, {kill} a target from a kill record, and {none} none. {candidates} have a designation candidate.", authored.len());
        f.analysis = "A missing target stays missing. A designation candidate is not used as a target.".into();
        f.values = vec![("author_shots", Some(authored.len() as f64), ""), ("seeker_targets", Some(seeker as f64), ""), ("kill_record_targets", Some(kill as f64), ""), ("no_target", Some(none as f64), ""), ("designation_candidates", Some(candidates as f64), "")];
        f.limitations = vec!["Seeker targets are inferred associations, and their accuracy has been measured only on kill-linked shots.".to_string()];
        f.relevance = "Shows how much of a sortie's targeting can be reconstructed for mission review.".into();
        self.add(f);
    }

    fn sortie(&mut self) {
        let d = self.d;
        let Some(a) = &d.author else { return };
        let lifts = self.author_events("liftoff_candidate");
        let downs = self.author_events("touchdown_candidate");
        if lifts.is_empty() && d.combat.author_deaths.is_empty() {
            return;
        }
        let mut times: Vec<(u32, bool)> = lifts.iter().map(|e| (e.timestamp_ms.unwrap_or(0), true)).chain(downs.iter().map(|e| (e.timestamp_ms.unwrap_or(0), false))).collect();
        times.sort();
        let turnaround: Vec<f64> = times.windows(2).filter(|w| !w[0].1 && w[1].1).map(|w| (w[1].0 - w[0].0) as f64 / 1000.0).collect();
        let legs: Vec<f64> = times.windows(2).filter(|w| w[0].1 && !w[1].1).map(|w| (w[1].0 - w[0].0) as f64 / 1000.0).collect();
        let (kills, deaths) = (d.combat.author_kills.len(), d.combat.author_deaths.len());
        let mut f = Draft::new("OPS-SORTIE", "OPS", Priority::Low, "Sortie structure");
        f.observation = format!("{:.0} s tracked. Takeoffs: {}. Landings: {}. Battle log kills: {kills}. Deaths: {deaths}.", a.tracked_seconds, lifts.len(), downs.len());
        f.analysis = match turnaround.first() {
            Some(t) => format!("A landing and takeoff were {} s apart, which bounds the turnaround for this one case.", round3(*t)),
            None => "No landing was followed by a takeoff, so no turnaround is measured.".to_string(),
        };
        f.values = vec![("tracked_time", Some(a.tracked_seconds), "s"), ("takeoffs", Some(lifts.len() as f64), ""), ("landings", Some(downs.len() as f64), ""), ("author_kills", Some(kills as f64), ""), ("author_deaths", Some(deaths as f64), ""), ("turnaround", turnaround.first().copied(), "s")];
        f.limitations = vec!["Takeoff and landing are candidates from speed and altitude changes.".to_string(), "A respawn is not a return to base.".to_string()];
        f.relevance = "Real sortie timing gives a reference for mission planning estimates.".into();
        f.events = lifts.iter().chain(&downs).take(4).cloned().collect();
        self.add(f);

        if let Some(leg) = legs.first().copied() {
            let mut n = Draft::new("NAV-LEG", "NAV", Priority::Low, "Takeoff-to-landing time");
            n.observation = format!("The first takeoff-to-landing leg took {} s.", round3(leg));
            n.analysis = "This is one flown leg with combat in it, so it is not a route time estimate.".into();
            n.values = vec![("leg_time", Some(leg), "s")];
            n.limitations = vec!["The route was not planned or recorded as waypoints.".to_string()];
            n.limitations.extend(self.no_8111());
            n.relevance = "A reference point for RTB time, once paired with fuel and distance.".into();
            n.events = downs.iter().take(1).cloned().collect();
            self.add(n);
        }
    }

    fn author_jumps(&mut self) {
        let Some(a) = self.d.anomalies.iter().find(|a| a.kind == "author_position_jump") else { return };
        let mut f = Draft::new("OPS-JUMP", "OPS", Priority::Low, "Author position jump");
        f.nature = Nature::UnresolvedDiscrepancy;
        f.observation = format!("Author track jumps of 200 m to 2 km between samples: {}.", a.count);
        f.analysis = "The cause is not established. It is reported as observed and is not treated as a decoding fault.".into();
        f.values = vec![("jumps", Some(a.count as f64), "")];
        f.limitations = vec!["Samples around each jump should not be used for performance figures.".to_string()];
        f.relevance = "Flight-path continuity matters when replay paths are used as navigation reference tracks.".into();
        f.events = a.examples.iter().take(3).cloned().collect();
        self.add(f);
    }

    fn summary(&self) -> Vec<String> {
        let d = self.d;
        let mut s = Vec::new();
        let r = &d.replay;
        let duration = r.timeline_span_s.map_or("unknown".to_string(), |x| format!("{x:.0} s"));
        s.push(format!("{} on {}, {duration}, game version {}.", self.vehicle.as_deref().unwrap_or("No author aircraft"), r.mission.as_deref().unwrap_or("an unknown mission"), if r.game_version.is_empty() { "unknown" } else { &r.game_version }));
        if let Some(a) = &d.author {
            s.push(format!("{} kills, {} deaths and {} weapons fired in the battle log and track data.", d.combat.author_kills.len(), d.combat.author_deaths.len(), a.weapons_released));
        }
        s.push(match self.validation {
            Some(_) if self.independent() => "8111 validation: independent comparison available for this flight.".to_string(),
            Some(_) => "8111 validation: a capture exists, but it does not meet the independent standard.".to_string(),
            None => "8111 validation: none. No synchronized capture covers this flight.".to_string(),
        });
        let count = |p: Priority| self.out.iter().filter(|f| f.priority == p).count();
        s.push(format!("Engineering findings: {} HIGH, {} MEDIUM, {} LOW.", count(Priority::High), count(Priority::Medium), count(Priority::Low)));
        if self.out.iter().any(|f| f.nature == Nature::SoftwareError) {
            s.push("A software error was detected: see the HIGH findings.".to_string());
        }
        s
    }
}

/// Validation data applies only to the replay it was measured on.
pub fn matching_validation(v: Option<serde_json::Value>, sha256: &str) -> Option<serde_json::Value> {
    v.filter(|x| x["provenance"]["replay_sha256"].as_str() == Some(sha256))
}

/// Findings for one replay. A rule with no evidence adds nothing.
pub fn analyze(d: &ResearchData, db: Option<&Databases>, validation: Option<&serde_json::Value>) -> EngineeringSection {
    let vehicle = d.author.as_ref().and_then(|a| a.vehicles.first().cloned());
    let s57_aircraft = vehicle.as_deref().and_then(|v| db.and_then(|x| x.aircraft_id(v))).map(String::from);
    let mut c = Ctx { d, db, validation, vehicle, s57_aircraft, out: Vec::new() };
    c.decode_status();
    if d.author.is_some() {
        c.map_frame();
        c.hud_instruments();
        c.hud_feet();
        c.hud_airspeed();
        c.hud_clock();
        c.fuel();
        c.engine();
        c.unidentified_fields();
        c.aircraft_performance();
        c.aircraft_database();
        c.weapons();
        c.sortie();
        c.author_jumps();
    }
    c.out.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.finding_id.cmp(&b.finding_id)));
    let executive_summary = c.summary();
    EngineeringSection { schema: ENGINEERING_SCHEMA, status: "proposed_for_review", executive_summary, findings: c.out }
}
