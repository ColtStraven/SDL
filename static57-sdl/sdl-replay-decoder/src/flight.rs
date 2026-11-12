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
//! altimeter at the nearest timestamp, after inferring whether that aircraft's
//! altimeter reads metres or feet. Other aircraft in the sync are not decoded
//! yet; their records stay raw.
//!
//! Observed leading-record layout (bits, MSB-first, see [`crate::bits`]),
//! established on the Me 262 A-1a/U4 (2 replays) and F-84B (1 replay):
//!
//! ```text
//! 0..38    constant prefix 11010110 10000001 11110101 10000000 011000
//! 38       on_ground (behaves like gear state)        EXPERIMENTAL
//! 39       unknown (always 0 so far)
//! 40..72   u32 value (always 0 so far; REPORTED as sometimes 0x3E555556)
//! 72..76   flags: bit 72 = firing                     EXPERIMENTAL
//! 76..80   weapon entry count N (Me 262: 1, F-84B: 14) EXPERIMENTAL
//! [8 bits  present only when firing; observed 00000001 (Me 262 only)]
//! weapon block: N entries of weapon-dependent width
//!          (Me 262 cannon: one 16-bit counter; F-84B: 6 x 24 + 8 x 16 bits)
//! 1 bit    closing bit (always 0 so far)
//! 3 x f32  world position x, y (height), z            y cross-checked
//! ...      other aircraft records (raw)
//! 3 x f32  trailing copy of the author position, ending in the last byte
//! ```
//!
//! Because entry widths depend on the weapon type, the position is located
//! structurally: the leading triple that equals the trailing copy (within
//! 1 cm). A single 16-bit entry (the Me 262 layout) is used when no trailer
//! matches and N = 1; otherwise the offset from the previous sync with the
//! same (N, firing) is reused and left to the altimeter and speed checks.

use crate::bits::BitReader;
use crate::evidence::{hex, Evidence};
use crate::manifest::{self, Depth, Manifest, Options};
use crate::mpi::{self, CockpitSample, CombatRecord};
use crate::packets::PacketRecord;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::io;

pub const SYNC_PREFIX: u64 = 0b11010110_10000001_11110101_10000000_011000;
pub const SYNC_PREFIX_BITS: usize = 38;

/// Cockpit samples farther than this from a sync are not used to check it.
pub const MATCH_WINDOW_MS: u32 = 300;
/// Altitude agreement threshold for the cross-check.
pub const ALTITUDE_TOLERANCE_M: f32 = 20.0;
/// Derived speeds above this between consecutive syncs are flagged.
pub const MAX_PLAUSIBLE_SPEED_M_S: f64 = 700.0;
/// Upper bound on one weapon entry's width when searching for the position.
const MAX_ENTRY_BITS: usize = 32;
/// Feet to metres.
pub const FT_TO_M: f32 = 0.3048;

#[derive(Debug, Clone, Serialize)]
pub struct OwnshipSync {
    pub on_ground: bool,
    pub unknown_bit39: bool,
    pub value32: u32,
    /// Four flag bits (72..76), first bit most significant.
    pub flags: u8,
    /// Flag bit 72.
    pub firing: bool,
    /// The three flag bits after `firing` (73..76).
    pub triggers: u8,
    /// Weapon entry count N (bits 76..80).
    pub entry_count: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firing_extra: Option<u8>,
    /// Width of the weapon block in bits (excluding the closing bit).
    pub weapon_block_bits: usize,
    /// Weapon block bits packed MSB-first into bytes (last byte zero-padded).
    pub weapon_block_hex: String,
    /// The single 16-bit entry as `u16le`, when the block is exactly one
    /// 16-bit entry (Me 262 layout): falls by one per shot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter: Option<u16>,
    /// Native world frame. `y` is height (matches the altimeter).
    pub position: [f32; 3],
    pub position_bit_offset: usize,
    /// How the position was located: `trailer_match`, `single_entry_layout`
    /// or `previous_offset`.
    pub position_source: &'static str,
    /// Whether the trailing three floats (REPORTED as the author's position)
    /// agree with `position` within 1 cm. An independent second copy.
    pub trailer_matches: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncSkip {
    /// Payload does not start with the leading-record prefix (seen before
    /// the author spawns: 17- and 19-byte syncs).
    NoOwnshipRecord,
    Truncated,
    /// The position could not be located: no trailer match, not a single
    /// 16-bit entry, and no previous offset for this (N, firing).
    UnexpectedLayout,
}

/// Position offsets last located structurally, keyed by (entry count, firing).
pub type OffsetHints = HashMap<(u8, bool), usize>;

/// Parse the leading (author) record of an `aircraft_small` payload.
pub fn parse_ownship_sync(payload: &[u8]) -> Result<OwnshipSync, SyncSkip> {
    parse_ownship_sync_hinted(payload, &OffsetHints::new())
}

/// A world position: every component exactly 0 or between 1 mm and
/// 1,000 km, and at least 1 m from the origin. Rejects all-zero regions and
/// the tiny-exponent values produced by reading floats at a shifted offset.
fn plausible(p: &[f32; 3]) -> bool {
    p.iter().all(|v| v.is_finite() && (*v == 0.0 || (v.abs() >= 1.0e-3 && v.abs() < 1.0e6)))
        && p.iter().map(|v| v * v).sum::<f32>() > 1.0
}

/// Trailing-copy candidates: the triple ending `pad` bits before the end of
/// the payload, for each `pad` in 0..=7 whose padding bits are all zero.
fn trailer_candidates(payload: &[u8]) -> Vec<[f32; 3]> {
    let nb = payload.len() * 8;
    (0..8)
        .filter_map(|pad| {
            let start = nb.checked_sub(96 + pad)?;
            let mut r = BitReader::new(payload);
            r.skip(start + 96)?;
            if r.bits(pad)? != 0 {
                return None;
            }
            triple_at(payload, start)
        })
        .filter(plausible)
        .collect()
}

fn triple_at(payload: &[u8], bit: usize) -> Option<[f32; 3]> {
    let mut r = BitReader::new(payload);
    r.skip(bit)?;
    Some([r.f32()?, r.f32()?, r.f32()?])
}

fn close(a: &[f32; 3], b: &[f32; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= TRAILER_TOLERANCE_M)
}

/// Parse the leading record, reusing a hint (the position bit offset from a
/// previous sync with the same entry count and firing state) only when the
/// position cannot be located structurally.
pub fn parse_ownship_sync_hinted(payload: &[u8], hints: &OffsetHints) -> Result<OwnshipSync, SyncSkip> {
    let mut r = BitReader::new(payload);
    let prefix = r.bits(SYNC_PREFIX_BITS).ok_or(SyncSkip::Truncated)?;
    if prefix != SYNC_PREFIX {
        return Err(SyncSkip::NoOwnshipRecord);
    }
    let t = SyncSkip::Truncated;
    let on_ground = r.bit().ok_or(t)?;
    let unknown_bit39 = r.bit().ok_or(t)?;
    let value32 = r.bits(32).ok_or(t)? as u32;
    let flags = r.bits(4).ok_or(t)? as u8;
    let entry_count = r.bits(4).ok_or(t)? as u8;
    let firing = flags & 0b1000 != 0;
    let firing_extra = if firing { Some(r.u8().ok_or(t)?) } else { None };
    let block_start = r.pos();
    let nb = payload.len() * 8;

    // 1. Structural: the first leading triple equal to the trailing copy.
    let trailers = trailer_candidates(payload);
    let search_end = (block_start + entry_count as usize * MAX_ENTRY_BITS + 1).min(nb.saturating_sub(192));
    let found = (block_start + 1..=search_end)
        .find(|&o| triple_at(payload, o).is_some_and(|p| plausible(&p) && trailers.iter().any(|tr| close(&p, tr))));

    let (pos_off, source) = match found {
        Some(o) => (o, "trailer_match"),
        None if entry_count == 1 => (block_start + 17, "single_entry_layout"),
        None => match hints.get(&(entry_count, firing)).copied().filter(|&h| h > block_start && h + 96 <= nb) {
            Some(h) => (h, "previous_offset"),
            None => return Err(SyncSkip::UnexpectedLayout),
        },
    };
    let position = triple_at(payload, pos_off).ok_or(t)?;
    if !plausible(&position) {
        return Err(SyncSkip::UnexpectedLayout);
    }

    let weapon_block_bits = pos_off - 1 - block_start;
    let mut br = BitReader::new(payload);
    br.skip(block_start).ok_or(t)?;
    let mut bytes = Vec::with_capacity(weapon_block_bits.div_ceil(8));
    let mut left = weapon_block_bits;
    while left > 0 {
        let n = left.min(8);
        bytes.push((br.bits(n).ok_or(t)? << (8 - n)) as u8);
        left -= n;
    }
    let counter = (weapon_block_bits == 16).then(|| u16::from_le_bytes([bytes[0], bytes[1]]));

    Ok(OwnshipSync {
        on_ground,
        unknown_bit39,
        value32,
        flags,
        firing,
        triggers: flags & 0b0111,
        entry_count,
        firing_extra,
        weapon_block_bits,
        weapon_block_hex: hex(&bytes),
        counter,
        position,
        position_bit_offset: pos_off,
        position_source: source,
        trailer_matches: trailer_matches(payload, position),
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
    pub position_source: &'static str,
    pub firing: bool,
    pub triggers: u8,
    pub entry_count: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter: Option<u16>,
    pub weapon_block_hex: String,
    /// DERIVED: `position.y` minus the nearest cockpit altitude, converted to
    /// metres with the inferred altimeter unit.
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
    /// `trigger` (sync firing flag set), `counter_step` (single-entry
    /// counter changed), `weapon_block_change` (multi-entry weapon block
    /// changed, e.g. a store released) or a combat record kind (`kill`,
    /// `damage`, `hit_marker`).
    pub kind: &'static str,
    pub evidence: Evidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triggers: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter_from: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter_to: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_from_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_to_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub combat: Option<CombatRecord>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CrossCheck {
    /// Cockpit altimeter (id 40) unit, inferred by comparing the sync height
    /// with the altimeter read as metres and as feet: `m`, `ft` or `unknown`.
    pub altimeter_unit: &'static str,
    /// DERIVED: chosen between two discrete hypotheses, not read from data.
    pub altimeter_unit_evidence: Evidence,
    /// Median |height - altimeter| under each hypothesis, metres.
    pub median_abs_delta_if_m: Option<f32>,
    pub median_abs_delta_if_ft: Option<f32>,
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
    /// Weapon entry counts seen in the leading record.
    pub entry_counts: Vec<u8>,
    /// How positions were located, by method.
    pub position_sources: BTreeMap<&'static str, u64>,
    pub trigger_packets: u64,
    pub counter_first: Option<u16>,
    pub counter_last: Option<u16>,
    pub counter_steps_down: u64,
    pub weapon_block_changes: u64,
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
    let mut hints = OffsetHints::new();

    let opts = Options { depth: Depth::Packets, ..Default::default() };
    let manifest = manifest::analyze(file_name, file, opts, |rec: &PacketRecord, payload: &[u8]| {
        let ts = rec.timestamp_ms.unwrap_or(0);
        match rec.type_id {
            Some(2) => {
                tally.aircraft_small_packets += 1;
                match parse_ownship_sync_hinted(payload, &hints) {
                    Ok(s) => {
                        tally.decoded += 1;
                        if s.position_source != "previous_offset" {
                            hints.insert((s.entry_count, s.firing), s.position_bit_offset);
                        }
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
                        block_from_hex: None,
                        block_to_hex: None,
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
    let nearest = |ci: &mut usize, ts: u32| {
        while *ci + 1 < alt.len() && alt[*ci + 1].0.abs_diff(ts) <= alt[*ci].0.abs_diff(ts) {
            *ci += 1;
        }
        alt.get(*ci).filter(|a| a.0.abs_diff(ts) <= MATCH_WINDOW_MS).copied()
    };

    // Altimeter unit: metres or feet, whichever fits the sync height better.
    let mut cc = CrossCheck { altitude_tolerance_m: ALTITUDE_TOLERANCE_M, ..Default::default() };
    {
        let (mut dm, mut dft) = (Vec::new(), Vec::new());
        let mut ci = 0usize;
        for (_, ts, _, _, s) in &ownship {
            if let Some(a) = nearest(&mut ci, *ts) {
                dm.push((s.position[1] - a.1).abs() as f64);
                dft.push((s.position[1] - a.1 * FT_TO_M).abs() as f64);
            }
        }
        cc.median_abs_delta_if_m = median(dm).map(|v| v as f32);
        cc.median_abs_delta_if_ft = median(dft).map(|v| v as f32);
        cc.altimeter_unit_evidence = Evidence::Derived;
        cc.altimeter_unit = match (cc.median_abs_delta_if_m, cc.median_abs_delta_if_ft) {
            (Some(m), Some(f)) if m <= f && m <= ALTITUDE_TOLERANCE_M => "m",
            (Some(m), Some(f)) if f < m && f <= ALTITUDE_TOLERANCE_M => "ft",
            _ => "unknown",
        };
    }
    let alt_factor = match cc.altimeter_unit {
        "m" => Some(1.0f32),
        "ft" => Some(FT_TO_M),
        _ => None,
    };

    let mut ci = 0usize;
    let mut deltas = Vec::new();
    let mut ratios = Vec::new();
    let mut samples: Vec<OwnshipSample> = Vec::with_capacity(ownship.len());
    let mut prev_counter: Option<u16> = None;
    let mut prev_block: Option<String> = None;
    let mut counter_steps_down = 0u64;
    let mut weapon_block_changes = 0u64;
    let mut trigger_packets = 0u64;
    let mut entry_counts: Vec<u8> = Vec::new();
    let mut position_sources: BTreeMap<&'static str, u64> = BTreeMap::new();

    for (index, ts, segment, payload_offset, s) in ownship {
        let near = nearest(&mut ci, ts);
        let altimeter_delta_m = near.zip(alt_factor).map(|(a, k)| s.position[1] - a.1 * k);
        if !entry_counts.contains(&s.entry_count) {
            entry_counts.push(s.entry_count);
        }
        *position_sources.entry(s.position_source).or_default() += 1;
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
                block_from_hex: None,
                block_to_hex: None,
                position: Some(s.position),
                combat: None,
            });
        }
        match s.counter {
            Some(c) => {
                if let Some(pc) = prev_counter.filter(|pc| *pc != c) {
                    if c < pc {
                        counter_steps_down += (pc - c) as u64;
                    }
                    weapons.push(WeaponEvent {
                        packet_index: index,
                        timestamp_ms: ts,
                        kind: "counter_step",
                        evidence: Evidence::Experimental,
                        triggers: Some(s.triggers),
                        counter_from: Some(pc),
                        counter_to: Some(c),
                        block_from_hex: None,
                        block_to_hex: None,
                        position: Some(s.position),
                        combat: None,
                    });
                }
                prev_counter = Some(c);
            }
            None => {
                if let Some(pb) = prev_block.as_ref().filter(|pb| pb.len() == s.weapon_block_hex.len() && **pb != s.weapon_block_hex) {
                    weapon_block_changes += 1;
                    weapons.push(WeaponEvent {
                        packet_index: index,
                        timestamp_ms: ts,
                        kind: "weapon_block_change",
                        evidence: Evidence::Experimental,
                        triggers: Some(s.triggers),
                        counter_from: None,
                        counter_to: None,
                        block_from_hex: Some(pb.clone()),
                        block_to_hex: Some(s.weapon_block_hex.clone()),
                        position: Some(s.position),
                        combat: None,
                    });
                }
                prev_block = Some(s.weapon_block_hex.clone());
            }
        }

        let checked = altimeter_delta_m.is_some_and(|d| d.abs() <= ALTITUDE_TOLERANCE_M);
        samples.push(OwnshipSample {
            packet_index: index,
            timestamp_ms: ts,
            on_ground: s.on_ground,
            position: s.position,
            // The altitude component was independently confirmed for this
            // sample; the record layout itself remains an observation.
            position_evidence: if checked { Evidence::Verified } else { Evidence::Experimental },
            position_source: s.position_source,
            firing: s.firing,
            triggers: s.triggers,
            entry_count: s.entry_count,
            counter: s.counter,
            weapon_block_hex: s.weapon_block_hex,
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
        schema: "static57.sdl.flight-summary/0.3",
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
        entry_counts,
        position_sources,
        trigger_packets,
        counter_first: samples.iter().find_map(|s| s.counter),
        counter_last: samples.iter().rev().find_map(|s| s.counter),
        counter_steps_down,
        weapon_block_changes,
        combat_records: weapons.iter().filter(|w| w.combat.is_some()).count() as u64,
        vehicle_names,
        notes: vec![
            "Only the replay author's aircraft is decoded. Other aircraft records in aircraft_small stay raw.",
            "Sync record layout is an observation from Static 57 replays (EXPERIMENTAL); a position sample is VERIFIED only when its height matches the cockpit altimeter (unit inferred, see cross_check.altimeter_unit).",
            "The counter (single 16-bit weapon entry only) is EXPERIMENTAL: it falls by one per shot, consistent with an ammunition count. Multi-entry weapon blocks are reported raw; weapon_block_change events mark any change.",
            "Combat record field layout is EXPERIMENTAL; vehicle names are read as length-prefixed strings.",
        ],
    };
    Ok(FlightDecode { manifest, ownship: samples, cockpit, weapons, summary })
}
