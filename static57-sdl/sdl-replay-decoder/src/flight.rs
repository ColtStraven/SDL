//! Flight Dynamics (Milestone 3, first stage): the replay author's own
//! aircraft ("ownship").
//!
//! Two independent sources are decoded and cross-checked:
//!
//! 1. `aircraft_small` (type 2) flight-model sync: the leading record gives
//!    the author's world position, an on-ground flag, a trigger/firing block
//!    and a counter that falls by one per shot.
//! 2. MPI `0xD04A ReplayCockpitParams`: the author's cockpit instruments
//!    (indicated airspeed, altitude, vertical speed, roll, pitch, engines,
//!    fuel, controls; ids match localhost:8111 `/indicators`, see `mpi`).
//!
//! The sync position's vertical component is checked against the cockpit
//! altimeter at the nearest timestamp. Other aircraft in the sync are not
//! decoded yet; their records stay raw.
//!
//! Observed leading-record layout (bits, MSB-first, see [`crate::bits`]):
//!
//! ```text
//! 0..38    constant prefix 11010110 10000001 11110101 10000000 011000
//! 38       on_ground                                  EXPERIMENTAL
//! 39       unknown (always 0 so far)
//! 40..72   u32 value (always 0 so far; REPORTED as sometimes 0x3E555556)
//! 72       firing                                     EXPERIMENTAL
//! 73..77   trigger bits                               EXPERIMENTAL
//! 77..80   group bits (always 001 so far)
//! [8 bits  present only when firing; observed 00000001]
//! u8       counter (falls by one per shot)            EXPERIMENTAL
//! 9 bits   unknown (always 0 so far)
//! 3 x f32  world position x, y (altitude), z          y cross-checked
//! ```

use crate::bits::BitReader;
use crate::evidence::Evidence;
use crate::manifest::{self, Depth, Manifest, Options};
use crate::mpi::{self, CockpitSample, CombatRecord};
use crate::packets::PacketRecord;
use serde::Serialize;
use std::io;

pub const SYNC_PREFIX: u64 = 0b11010110_10000001_11110101_10000000_011000;
pub const SYNC_PREFIX_BITS: usize = 38;

/// Cockpit samples farther than this from a sync are not used to check it.
pub const MATCH_WINDOW_MS: u32 = 300;
/// Altitude agreement threshold for the cross-check.
pub const ALTITUDE_TOLERANCE_M: f32 = 20.0;
/// Derived speeds above this between consecutive syncs are flagged.
pub const MAX_PLAUSIBLE_SPEED_M_S: f64 = 700.0;

#[derive(Debug, Clone, Serialize)]
pub struct OwnshipSync {
    pub on_ground: bool,
    pub unknown_bit39: bool,
    pub value32: u32,
    pub firing: bool,
    /// Four trigger bits, first bit most significant.
    pub triggers: u8,
    pub group_bits: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firing_extra: Option<u8>,
    pub counter: u8,
    pub unknown_tail9: u16,
    /// Native world frame. `y` is height (matches the altimeter).
    pub position: [f32; 3],
    pub position_bit_offset: usize,
    /// Whether the trailing three floats (REPORTED as the author's position)
    /// agree with `position` within 1 cm. An independent second copy.
    pub trailer_matches: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncSkip {
    /// Payload does not start with the leading-record prefix (seen before
    /// the author spawns: 19-byte syncs).
    NoOwnshipRecord,
    Truncated,
    /// Group bits other than `001`: layout not seen before, not interpreted.
    UnexpectedLayout,
}

/// Parse the leading (author) record of an `aircraft_small` payload.
pub fn parse_ownship_sync(payload: &[u8]) -> Result<OwnshipSync, SyncSkip> {
    let mut r = BitReader::new(payload);
    let prefix = r.bits(SYNC_PREFIX_BITS).ok_or(SyncSkip::Truncated)?;
    if prefix != SYNC_PREFIX {
        return Err(SyncSkip::NoOwnshipRecord);
    }
    let t = SyncSkip::Truncated;
    let on_ground = r.bit().ok_or(t)?;
    let unknown_bit39 = r.bit().ok_or(t)?;
    let value32 = r.bits(32).ok_or(t)? as u32;
    let firing = r.bit().ok_or(t)?;
    let triggers = r.bits(4).ok_or(t)? as u8;
    let group_bits = r.bits(3).ok_or(t)? as u8;
    if group_bits != 0b001 {
        return Err(SyncSkip::UnexpectedLayout);
    }
    let firing_extra = if firing { Some(r.u8().ok_or(t)?) } else { None };
    let counter = r.u8().ok_or(t)?;
    let unknown_tail9 = r.bits(9).ok_or(t)? as u16;
    let position_bit_offset = r.pos();
    let position = [r.f32().ok_or(t)?, r.f32().ok_or(t)?, r.f32().ok_or(t)?];
    let trailer_matches = trailer_matches(payload, position);
    Ok(OwnshipSync {
        on_ground,
        unknown_bit39,
        value32,
        firing,
        triggers,
        group_bits,
        firing_extra,
        counter,
        unknown_tail9,
        position,
        position_bit_offset,
        trailer_matches,
    })
}

/// Trailer agreement tolerance per axis, metres.
pub const TRAILER_TOLERANCE_M: f32 = 0.01;

/// Whether the three floats ending within the last byte (after any padding
/// of 0..=7 bits) agree with `position` within [`TRAILER_TOLERANCE_M`].
/// In replay `#2026.11.09 18.50.56` all 2,494 agree (748 bit-identical).
pub fn trailer_matches(payload: &[u8], position: [f32; 3]) -> bool {
    let nb = payload.len() * 8;
    (0..8).any(|pad| {
        let Some(start) = nb.checked_sub(96 + pad) else { return false };
        let mut r = BitReader::new(payload);
        if r.skip(start).is_none() {
            return false;
        }
        (0..3).all(|i| r.f32().is_some_and(|v| (v - position[i]).abs() <= TRAILER_TOLERANCE_M))
    })
}

/// The three floats ending within the last byte (after 0..=7 padding bits),
/// choosing the smallest padding whose values are finite and plausible.
pub fn trailer_position(payload: &[u8]) -> Option<[f32; 3]> {
    let nb = payload.len() * 8;
    (0..8).find_map(|pad| {
        let start = nb.checked_sub(96 + pad)?;
        let mut r = BitReader::new(payload);
        r.skip(start)?;
        let p = [r.f32()?, r.f32()?, r.f32()?];
        p.iter().all(|v| v.is_finite() && v.abs() < 1.0e6 && (*v == 0.0 || v.abs() > 1.0e-3)).then_some(p)
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct OwnshipSample {
    pub packet_index: u64,
    pub timestamp_ms: u32,
    pub on_ground: bool,
    pub position: [f32; 3],
    pub position_evidence: Evidence,
    pub firing: bool,
    pub triggers: u8,
    pub counter: u8,
    /// DERIVED: `position.y` minus the nearest cockpit altitude.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub altimeter_delta_m: Option<f32>,
    /// DERIVED: distance from the previous sample over elapsed time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derived_speed_m_s: Option<f64>,
    pub trailer_matches: bool,
    /// Source provenance.
    pub segment: u32,
    pub payload_offset: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CockpitRecord {
    pub packet_index: u64,
    pub timestamp_ms: u32,
    #[serde(flatten)]
    pub sample: CockpitSample,
    pub segment: u32,
    pub payload_offset: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeaponEvent {
    pub packet_index: u64,
    pub timestamp_ms: u32,
    /// `trigger` (sync firing flag set), `counter_step` (ownship counter
    /// changed) or a combat record kind (`kill`, `damage`, `hit_marker`).
    pub kind: &'static str,
    pub evidence: Evidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triggers: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter_from: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter_to: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub combat: Option<CombatRecord>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CrossCheck {
    pub ownship_samples: u64,
    pub samples_with_cockpit_match: u64,
    pub altitude_within_tolerance: u64,
    pub altitude_tolerance_m: f32,
    pub median_abs_altitude_delta_m: Option<f32>,
    pub max_abs_altitude_delta_m: Option<f32>,
    pub speed_discontinuities: u64,
    pub max_derived_speed_m_s: Option<f64>,
    pub trailer_matches: u64,
    /// DERIVED: median of derived ground-track speed / cockpit IAS (id 0) while
    /// airborne. Exceeds 1 by roughly the TAS/IAS ratio at altitude.
    pub median_ground_speed_to_ias_airborne: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncTally {
    pub aircraft_small_packets: u64,
    pub decoded: u64,
    pub no_ownship_record: u64,
    pub truncated: u64,
    pub unexpected_layout: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlightSummary {
    pub schema: &'static str,
    pub sync: SyncTally,
    pub cockpit_messages: u64,
    pub cockpit_errors: u64,
    pub cockpit_param_ids: Vec<u16>,
    pub cross_check: CrossCheck,
    pub ownship_first_timestamp_ms: Option<u32>,
    pub ownship_last_timestamp_ms: Option<u32>,
    pub airborne_from_ms: Option<u32>,
    pub airborne_until_ms: Option<u32>,
    pub max_altitude_m: Option<f32>,
    pub max_cockpit_ias_m_s: Option<f32>,
    pub trigger_packets: u64,
    pub counter_first: Option<u8>,
    pub counter_last: Option<u8>,
    pub counter_steps_down: u64,
    pub combat_records: u64,
    pub vehicle_names: Vec<String>,
    pub notes: Vec<&'static str>,
}

pub struct FlightDecode {
    pub manifest: Manifest,
    pub ownship: Vec<OwnshipSample>,
    pub cockpit: Vec<CockpitRecord>,
    pub weapons: Vec<WeaponEvent>,
    pub summary: FlightSummary,
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(v[v.len() / 2])
}

/// Run framing and decode ownship flight data, cockpit instruments and
/// weapon events from an in-memory replay.
pub fn decode(file_name: &str, file: &[u8]) -> io::Result<FlightDecode> {
    let mut ownship = Vec::new();
    let mut cockpit = Vec::new();
    let mut weapons = Vec::new();
    let mut tally = SyncTally::default();
    let mut cockpit_errors = 0u64;

    let opts = Options { depth: Depth::Packets, ..Default::default() };
    let manifest = manifest::analyze(file_name, file, opts, |rec: &PacketRecord, payload: &[u8]| {
        let ts = rec.timestamp_ms.unwrap_or(0);
        match rec.type_id {
            Some(2) => {
                tally.aircraft_small_packets += 1;
                match parse_ownship_sync(payload) {
                    Ok(s) => {
                        tally.decoded += 1;
                        ownship.push((rec.index, ts, rec.segment, rec.payload_offset, s));
                    }
                    Err(SyncSkip::NoOwnshipRecord) => tally.no_ownship_record += 1,
                    Err(SyncSkip::Truncated) => tally.truncated += 1,
                    Err(SyncSkip::UnexpectedLayout) => tally.unexpected_layout += 1,
                }
            }
            Some(4) => {
                if payload.starts_with(&mpi::SIG_COCKPIT) {
                    match mpi::parse_cockpit(payload) {
                        Ok(sample) => cockpit.push(CockpitRecord {
                            packet_index: rec.index,
                            timestamp_ms: ts,
                            sample,
                            segment: rec.segment,
                            payload_offset: rec.payload_offset,
                        }),
                        Err(_) => cockpit_errors += 1,
                    }
                } else if let Some(c) = mpi::parse_combat(payload) {
                    weapons.push(WeaponEvent {
                        packet_index: rec.index,
                        timestamp_ms: ts,
                        kind: c.kind,
                        evidence: Evidence::Experimental,
                        triggers: None,
                        counter_from: None,
                        counter_to: None,
                        position: None,
                        combat: Some(c),
                    });
                }
            }
            _ => {}
        }
        Ok(())
    })?;

    // Cross-check against the cockpit altimeter (records are in time order).
    let alt: Vec<(u32, f32, Option<f32>)> =
        cockpit.iter().filter_map(|c| c.sample.get(40).map(|a| (c.timestamp_ms, a, c.sample.get(0)))).collect();
    let mut ci = 0usize;
    let mut cc = CrossCheck { altitude_tolerance_m: ALTITUDE_TOLERANCE_M, ..Default::default() };
    let mut deltas = Vec::new();
    let mut ratios = Vec::new();
    let mut samples: Vec<OwnshipSample> = Vec::with_capacity(ownship.len());
    let mut prev_counter: Option<u8> = None;
    let mut counter_steps_down = 0u64;
    let mut trigger_packets = 0u64;

    for (index, ts, segment, payload_offset, s) in ownship {
        while ci + 1 < alt.len() && alt[ci + 1].0.abs_diff(ts) <= alt[ci].0.abs_diff(ts) {
            ci += 1;
        }
        let near = alt.get(ci).filter(|a| a.0.abs_diff(ts) <= MATCH_WINDOW_MS);
        let altimeter_delta_m = near.map(|a| s.position[1] - a.1);
        let derived_speed_m_s = samples.last().filter(|p| ts > p.timestamp_ms).map(|p| {
            let d: f64 = (0..3).map(|i| (s.position[i] as f64 - p.position[i] as f64).powi(2)).sum::<f64>().sqrt();
            d / ((ts - p.timestamp_ms) as f64 / 1000.0)
        });

        cc.ownship_samples += 1;
        if let Some(d) = altimeter_delta_m {
            cc.samples_with_cockpit_match += 1;
            if d.abs() <= ALTITUDE_TOLERANCE_M {
                cc.altitude_within_tolerance += 1;
            }
            deltas.push(d.abs() as f64);
        }
        if let Some(v) = derived_speed_m_s {
            if v > MAX_PLAUSIBLE_SPEED_M_S {
                cc.speed_discontinuities += 1;
            } else {
                cc.max_derived_speed_m_s = Some(cc.max_derived_speed_m_s.map_or(v, |m: f64| m.max(v)));
                if let Some(ias) = near.and_then(|a| a.2).filter(|v| *v > 30.0 && !s.on_ground) {
                    ratios.push(v / ias as f64);
                }
            }
        }
        if s.trailer_matches {
            cc.trailer_matches += 1;
        }
        if s.firing {
            trigger_packets += 1;
            weapons.push(WeaponEvent {
                packet_index: index,
                timestamp_ms: ts,
                kind: "trigger",
                evidence: Evidence::Experimental,
                triggers: Some(s.triggers),
                counter_from: None,
                counter_to: None,
                position: Some(s.position),
                combat: None,
            });
        }
        if let Some(pc) = prev_counter.filter(|pc| *pc != s.counter) {
            if s.counter < pc {
                counter_steps_down += (pc - s.counter) as u64;
            }
            weapons.push(WeaponEvent {
                packet_index: index,
                timestamp_ms: ts,
                kind: "counter_step",
                evidence: Evidence::Experimental,
                triggers: Some(s.triggers),
                counter_from: Some(pc),
                counter_to: Some(s.counter),
                position: Some(s.position),
                combat: None,
            });
        }
        prev_counter = Some(s.counter);

        let checked = altimeter_delta_m.is_some_and(|d| d.abs() <= ALTITUDE_TOLERANCE_M);
        samples.push(OwnshipSample {
            packet_index: index,
            timestamp_ms: ts,
            on_ground: s.on_ground,
            position: s.position,
            // The altitude component was independently confirmed for this
            // sample; the record layout itself remains an observation.
            position_evidence: if checked { Evidence::Verified } else { Evidence::Experimental },
            firing: s.firing,
            triggers: s.triggers,
            counter: s.counter,
            altimeter_delta_m,
            derived_speed_m_s,
            trailer_matches: s.trailer_matches,
            segment,
            payload_offset,
        });
    }
    weapons.sort_by_key(|w| (w.timestamp_ms, w.packet_index));

    let med = median(deltas.clone());
    cc.median_abs_altitude_delta_m = med.map(|v| v as f32);
    cc.max_abs_altitude_delta_m = deltas.iter().cloned().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v)))).map(|v| v as f32);
    cc.median_ground_speed_to_ias_airborne = median(ratios);

    let mut ids: Vec<u16> = cockpit.iter().flat_map(|c| c.sample.params.iter().map(|p| p.id)).collect();
    ids.sort_unstable();
    ids.dedup();
    let airborne: Vec<&OwnshipSample> = samples.iter().filter(|s| !s.on_ground).collect();
    let mut vehicle_names: Vec<String> =
        weapons.iter().filter_map(|w| w.combat.as_ref()).flat_map(|c| c.vehicle_names.clone()).collect();
    vehicle_names.sort();
    vehicle_names.dedup();

    let summary = FlightSummary {
        schema: "static57.sdl.flight-summary/0.2",
        sync: tally,
        cockpit_messages: cockpit.len() as u64,
        cockpit_errors,
        cockpit_param_ids: ids,
        cross_check: cc,
        ownship_first_timestamp_ms: samples.first().map(|s| s.timestamp_ms),
        ownship_last_timestamp_ms: samples.last().map(|s| s.timestamp_ms),
        airborne_from_ms: airborne.first().map(|s| s.timestamp_ms),
        airborne_until_ms: airborne.last().map(|s| s.timestamp_ms),
        max_altitude_m: samples
            .iter()
            .filter(|s| s.position_evidence == Evidence::Verified)
            .map(|s| s.position[1])
            .fold(None, |m: Option<f32>, v| Some(m.map_or(v, |m| m.max(v)))),
        max_cockpit_ias_m_s: cockpit
            .iter()
            .filter_map(|c| c.sample.get(0))
            .fold(None, |m: Option<f32>, v| Some(m.map_or(v, |m| m.max(v)))),
        trigger_packets,
        counter_first: samples.first().map(|s| s.counter),
        counter_last: samples.last().map(|s| s.counter),
        counter_steps_down,
        combat_records: weapons.iter().filter(|w| w.combat.is_some()).count() as u64,
        vehicle_names,
        notes: vec![
            "Only the replay author's aircraft is decoded. Other aircraft records in aircraft_small stay raw.",
            "Sync record layout is an observation from Static 57 replays (EXPERIMENTAL); a position sample is VERIFIED only when its height matches the cockpit altimeter.",
            "The counter is EXPERIMENTAL: it falls by one per shot during firing, consistent with an ammunition count of one gun group.",
            "Combat record field layout is EXPERIMENTAL; vehicle names are read as length-prefixed strings.",
        ],
    };
    Ok(FlightDecode { manifest, ownship: samples, cockpit, weapons, summary })
}
