//! Aircraft track reconstruction and validation statistics.
//!
//! Identity: an aircraft record is keyed by its 11-bit unit uid. A uid is
//! NOT a vehicle or player identity (uids are reused across lives and the
//! uid-to-vehicle relation needs ECS decoding), so tracks are provisional
//! segments `u<uid>.s<n>`. A new segment starts on the first record of a
//! uid, after a gap longer than `max_gap_ms`, or after a position jump that
//! movement cannot explain. Segments are never merged.

use crate::aircraft::{EntryBody, SyncDecode};
use crate::cockpit::{self, CockpitParams};
use crate::evidence::{hex, Evidence};
use crate::flight::{self, Orientation, Velocity};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TrackConfig {
    /// Gap without a state record that starts a new segment.
    pub max_gap_ms: u32,
    /// `|distance - speed*dt|` beyond which a new segment starts.
    pub split_jump_m: f64,
    /// `|distance - speed*dt|` beyond which a sample is flagged.
    pub flag_jump_m: f64,
    /// Maximum distance between a record and the packet trailer for the
    /// record to be attributed to the recording author.
    pub author_match_m: f64,
    /// A cockpit message is compared when an author sample lies within this
    /// many ms and the bracketing author samples are at most `2x + 50` ms apart.
    pub cockpit_match_ms: u32,
}

impl Default for TrackConfig {
    fn default() -> Self {
        TrackConfig { max_gap_ms: 5_000, split_jump_m: 2_000.0, flag_jump_m: 200.0, author_match_m: 1.0, cockpit_match_ms: 125 }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayRef {
    pub replay_sha256: String,
    pub format_magic: String,
    pub game_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Position {
    pub x_m: f64,
    pub y_m: f64,
    pub z_m: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Derived {
    /// Horizontal component of the decoded velocity.
    pub groundspeed_mps: f64,
    /// Vertical component of the decoded velocity.
    pub vertical_speed_mps: f64,
    /// Direction of horizontal velocity, heading convention.
    pub track_deg: Option<f64>,
    /// Flight-path angle, `asin(vy / speed)`.
    pub flight_path_deg: Option<f64>,
    /// Speed from the displacement since the previous sample of the segment.
    pub fd_speed_mps: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SampleEvidence {
    pub position: Evidence,
    pub altitude: Evidence,
    pub orientation: Evidence,
    pub velocity: Evidence,
    pub derived: Evidence,
    pub track_identity: Evidence,
    pub author: Evidence,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawFields {
    pub euler_hex: String,
    pub velocity_hex: String,
    pub velocity_bit18: bool,
    pub ground_flag: bool,
    pub unk_u32: u32,
    pub unk_bit: bool,
    pub controls_hex: String,
    pub engine_states: Vec<u8>,
    pub position_bit: u64,
    pub entry_bit_offset: u64,
    pub entry_bit_length: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlightSample {
    #[serde(flatten)]
    pub replay: ReplayRef,
    pub packet_index: u64,
    pub entry_index: u32,
    pub timestamp_ms: u32,
    pub track_id: String,
    pub uid: u16,
    pub segment: u32,
    pub is_author: bool,
    pub position: Position,
    pub altitude_msl_m: f64,
    pub orientation: Orientation,
    pub velocity: Velocity,
    pub derived: Derived,
    pub evidence: SampleEvidence,
    pub flags: Vec<&'static str>,
    pub raw: RawFields,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackSummary {
    pub track_id: String,
    pub uid: u16,
    pub segment: u32,
    /// `first_record`, `gap`, or `position_jump`.
    pub start_reason: &'static str,
    pub gap_before_ms: Option<u32>,
    pub start_ms: u32,
    pub end_ms: u32,
    pub samples: u64,
    pub author_samples: u64,
    pub withheld_records: u64,
    pub flagged_jumps: u64,
    pub saturated_speed_samples: u64,
    pub min_altitude_m: f64,
    pub max_altitude_m: f64,
    pub max_speed_mps: f64,
    /// DERIVED: sum of displacements between consecutive samples.
    pub path_length_m: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Dist {
    pub n: u64,
    pub p10: Option<f64>,
    pub p50: Option<f64>,
    pub p90: Option<f64>,
    pub p99: Option<f64>,
    pub max: Option<f64>,
}

impl Dist {
    pub fn of(mut v: Vec<f64>) -> Dist {
        v.retain(|x| x.is_finite());
        if v.is_empty() {
            return Dist::default();
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = |p: f64| Some(v[((v.len() - 1) as f64 * p).round() as usize]);
        Dist { n: v.len() as u64, p10: q(0.10), p50: q(0.50), p90: q(0.90), p99: q(0.99), max: q(1.0) }
    }
}

fn pearson(p: &[(f64, f64)]) -> Option<f64> {
    if p.len() < 3 {
        return None;
    }
    let n = p.len() as f64;
    let (mx, my) = p.iter().fold((0.0, 0.0), |a, (x, y)| (a.0 + x, a.1 + y));
    let (mx, my) = (mx / n, my / n);
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in p {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx).powi(2);
        syy += (y - my).powi(2);
    }
    let r = sxy / (sxx * syy).sqrt();
    r.is_finite().then_some(r)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Comparison {
    pub n: u64,
    pub pearson_r: Option<f64>,
    pub abs_error: Dist,
}

impl Comparison {
    fn of(pairs: &[(f64, f64)], err: impl Fn(f64, f64) -> f64) -> Comparison {
        Comparison {
            n: pairs.len() as u64,
            pearson_r: pearson(pairs),
            abs_error: Dist::of(pairs.iter().map(|&(a, b)| err(a, b).abs()).collect()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct KinematicValidation {
    /// `|displacement - mean decoded speed * dt|` for consecutive samples
    /// with `dt <= 300 ms`, metres.
    pub displacement_residual_m: Dist,
    /// Angle between displacement and mean decoded velocity (speed > 30
    /// m/s, displacement > 5 m), degrees.
    pub displacement_direction_error_deg: Dist,
    /// The same two measures restricted to the recording author (local
    /// flight model, no network replication).
    pub author_displacement_residual_m: Dist,
    pub author_displacement_direction_error_deg: Dist,
    /// `|heading - track angle of decoded velocity|` (speed > 60 m/s), degrees.
    pub heading_minus_track_abs_deg: Dist,
    /// Signed `pitch - flight-path angle` (speed > 60 m/s), degrees.
    pub pitch_minus_flight_path_deg: Dist,
    pub timestamp_reversals: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CockpitValidation {
    pub cockpit_messages: u64,
    pub author_samples: u64,
    pub matched: u64,
    /// Cockpit messages compared (against interpolated author state).
    /// `|cockpit time - nearest author sample|` of each comparison, ms.
    pub pairing_offset_ms: Dist,
    /// Cockpit parameter id (40, 42 or 43) used as the altitude reference.
    /// Decoded values are interpolated to the cockpit time between the
    /// bracketing author samples (DERIVED).
    pub altimeter_param_id: Option<u16>,
    /// `m` or `ft`: the altimeter unit that matches decoded Y most often.
    pub altimeter_unit: Option<&'static str>,
    pub altitude_within_2m_fraction: Option<f64>,
    pub altitude_m: Comparison,
    pub vertical_speed_mps: Comparison,
    pub roll_deg: Comparison,
    pub pitch_deg: Comparison,
    /// Decoded true speed converted to indicated via ISA density (DERIVED)
    /// versus cockpit indicated airspeed; unsaturated samples above 60 m/s.
    pub ias_isa_mps: Comparison,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Coverage {
    pub sync_packets: u64,
    pub structurally_decoded_packets: u64,
    pub structural_failures: u64,
    pub entries: u64,
    pub no_data_entries: u64,
    pub withheld_entries: u64,
    pub state_records: u64,
    pub payload_bits: u64,
    /// Position, orientation and velocity words with established meaning.
    pub physically_decoded_bits: u64,
    /// Bits parsed by structure but whose meaning is unknown or not exported.
    pub structural_only_bits: u64,
    pub trailer_bits: u64,
    /// Bits after the trailer: not parsed.
    pub unparsed_bits: u64,
    pub packets_with_author_match: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlightReport {
    pub schema: &'static str,
    #[serde(flatten)]
    pub replay: ReplayRef,
    pub config: TrackConfig,
    pub coverage: Coverage,
    pub tracks: u64,
    pub uids: u64,
    pub samples: u64,
    pub author_uids: Vec<u16>,
    /// Segment starts by reason (`first_record`, `gap`, `position_jump`).
    pub segment_starts: std::collections::BTreeMap<&'static str, u64>,
    /// Samples flagged `position_jump` without starting a segment.
    pub flagged_jumps: u64,
    pub kinematic_validation: KinematicValidation,
    pub cockpit_validation: CockpitValidation,
    pub track_summaries: Vec<TrackSummary>,
}

pub const FLIGHT_SCHEMA: &str = "static57.sdl.flight-tracks/0.1";

struct Last {
    t: u32,
    pos: [f64; 3],
    vel: Velocity,
}

struct UidState {
    track: usize,
    segment: u32,
    last: Option<Last>,
    last_seen_ms: u32,
}

#[derive(Clone, Copy)]
struct AuthorPoint {
    t: u32,
    y: f64,
    vy: f64,
    speed: f64,
    saturated: bool,
    bank: f64,
    pitch: f64,
}

pub struct Reconstructor {
    cfg: TrackConfig,
    replay: ReplayRef,
    uids: HashMap<u16, UidState>,
    tracks: Vec<TrackSummary>,
    coverage: Coverage,
    samples: u64,
    authors: HashMap<u16, u64>,
    author_points: Vec<AuthorPoint>,
    cockpit: Vec<CockpitParams>,
    disp_res: Vec<f64>,
    disp_dir: Vec<f64>,
    author_disp_res: Vec<f64>,
    author_disp_dir: Vec<f64>,
    heading_track: Vec<f64>,
    pitch_gamma: Vec<f64>,
    reversals: u64,
}

impl Reconstructor {
    pub fn new(replay: ReplayRef, cfg: TrackConfig) -> Self {
        Reconstructor {
            cfg,
            replay,
            uids: HashMap::new(),
            tracks: Vec::new(),
            coverage: Coverage::default(),
            samples: 0,
            authors: HashMap::new(),
            author_points: Vec::new(),
            cockpit: Vec::new(),
            disp_res: Vec::new(),
            disp_dir: Vec::new(),
            author_disp_res: Vec::new(),
            author_disp_dir: Vec::new(),
            heading_track: Vec::new(),
            pitch_gamma: Vec::new(),
            reversals: 0,
        }
    }

    pub fn push_cockpit(&mut self, c: CockpitParams) {
        self.cockpit.push(c);
    }

    fn open_track(&mut self, uid: u16, segment: u32, t: u32, reason: &'static str, gap: Option<u32>) -> usize {
        self.tracks.push(TrackSummary {
            track_id: format!("u{uid}.s{segment}"),
            uid,
            segment,
            start_reason: reason,
            gap_before_ms: gap,
            start_ms: t,
            end_ms: t,
            samples: 0,
            author_samples: 0,
            withheld_records: 0,
            flagged_jumps: 0,
            saturated_speed_samples: 0,
            min_altitude_m: f64::INFINITY,
            max_altitude_m: f64::NEG_INFINITY,
            max_speed_mps: 0.0,
            path_length_m: 0.0,
        });
        self.tracks.len() - 1
    }

    /// Feed one decoded type-2 payload; `sink` receives every sample.
    pub fn push_sync(&mut self, packet_index: u64, t: u32, payload_bits: u64, d: &SyncDecode, mut sink: impl FnMut(&FlightSample)) {
        let c = &mut self.coverage;
        c.sync_packets += 1;
        c.payload_bits += payload_bits;
        if d.terminated && d.error.is_none() {
            c.structurally_decoded_packets += 1;
        } else {
            c.structural_failures += 1;
        }
        let trailer_bits = if d.trailer.is_some() { 96 } else { 0 };
        c.trailer_bits += trailer_bits;
        c.unparsed_bits += payload_bits.saturating_sub(d.consumed_bits);
        let states = d.entries.iter().filter(|e| matches!(e.body, EntryBody::State(_))).count() as u64;
        c.physically_decoded_bits += states * 160;
        c.structural_only_bits += d.consumed_bits.saturating_sub(trailer_bits + states * 160);

        // Author: the state record nearest the trailer, if within tolerance.
        let author_entry = d.trailer.and_then(|tr| {
            d.entries
                .iter()
                .enumerate()
                .filter_map(|(i, e)| match &e.body {
                    EntryBody::State(s) => {
                        let p = s.position;
                        Some((i, ((p[0] - tr[0]) as f64).hypot((p[1] - tr[1]) as f64).hypot((p[2] - tr[2]) as f64)))
                    }
                    _ => None,
                })
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .filter(|(_, dist)| *dist <= self.cfg.author_match_m)
                .map(|(i, _)| i)
        });
        if author_entry.is_some() {
            self.coverage.packets_with_author_match += 1;
        }

        for (i, e) in d.entries.iter().enumerate() {
            self.coverage.entries += 1;
            let s = match &e.body {
                EntryBody::NoData => {
                    self.coverage.no_data_entries += 1;
                    continue;
                }
                EntryBody::Withheld => {
                    self.coverage.withheld_entries += 1;
                    if let Some(u) = self.uids.get(&e.uid) {
                        self.tracks[u.track].withheld_records += 1;
                    }
                    continue;
                }
                EntryBody::State(s) => s,
            };
            self.coverage.state_records += 1;
            let pos = [s.position[0] as f64, s.position[1] as f64, s.position[2] as f64];
            let vel = flight::decode_velocity(s.packed_velocity_raw);
            let ori = flight::decode_orientation(s.euler_raw);
            let mut flags: Vec<&'static str> = Vec::new();
            if pos.iter().any(|v| !v.is_finite()) {
                flags.push("non_finite_position");
            }
            if vel.saturated {
                flags.push("speed_saturated");
            }
            if vel.clamped {
                flags.push("direction_clamped");
            }

            // Segment decision.
            let mut fd_speed = None;
            let mut step = None;
            let (track, segment) = match self.uids.get(&e.uid) {
                None => {
                    let tr = self.open_track(e.uid, 0, t, "first_record", None);
                    self.uids.insert(e.uid, UidState { track: tr, segment: 0, last: None, last_seen_ms: t });
                    flags.push("segment_start");
                    (tr, 0)
                }
                Some(u) => {
                    let last = u.last.as_ref().unwrap();
                    if t < last.t {
                        self.reversals += 1;
                        flags.push("timestamp_reversal");
                    }
                    let dt_ms = t.saturating_sub(last.t);
                    let dist = ((pos[0] - last.pos[0]).powi(2) + (pos[1] - last.pos[1]).powi(2) + (pos[2] - last.pos[2]).powi(2)).sqrt();
                    let expect = (vel.speed_mps + last.vel.speed_mps) / 2.0 * dt_ms as f64 / 1000.0;
                    let residual = (dist - expect).abs();
                    let (new_seg, reason) = if dt_ms > self.cfg.max_gap_ms {
                        (true, "gap")
                    } else if residual > self.cfg.split_jump_m {
                        (true, "position_jump")
                    } else {
                        (false, "")
                    };
                    if new_seg {
                        let seg = u.segment + 1;
                        let tr = self.open_track(e.uid, seg, t, reason, Some(dt_ms));
                        let u = self.uids.get_mut(&e.uid).unwrap();
                        u.track = tr;
                        u.segment = seg;
                        flags.push("segment_start");
                        (tr, seg)
                    } else {
                        if dt_ms == 0 {
                            flags.push("repeated_timestamp");
                        } else {
                            fd_speed = Some(dist / (dt_ms as f64 / 1000.0));
                        }
                        if residual > self.cfg.flag_jump_m {
                            flags.push("position_jump");
                            self.tracks[u.track].flagged_jumps += 1;
                        }
                        step = Some((dt_ms, dist, residual, [pos[0] - last.pos[0], pos[1] - last.pos[1], pos[2] - last.pos[2]], last.vel));
                        (u.track, u.segment)
                    }
                }
            };

            // Validation accumulators.
            let is_author = author_entry == Some(i);
            if let Some((dt_ms, dist, residual, d3, lv)) = step {
                if dt_ms > 0 && dt_ms <= 300 {
                    self.disp_res.push(residual);
                    if is_author {
                        self.author_disp_res.push(residual);
                    }
                    let mv = [(vel.vx_mps + lv.vx_mps) / 2.0, (vel.vy_mps + lv.vy_mps) / 2.0, (vel.vz_mps + lv.vz_mps) / 2.0];
                    let mvn = (mv[0] * mv[0] + mv[1] * mv[1] + mv[2] * mv[2]).sqrt();
                    if vel.speed_mps > 30.0 && dist > 5.0 && mvn > 0.0 {
                        let cos = (d3[0] * mv[0] + d3[1] * mv[1] + d3[2] * mv[2]) / (dist * mvn);
                        let ang = cos.clamp(-1.0, 1.0).acos().to_degrees();
                        self.disp_dir.push(ang);
                        if is_author {
                            self.author_disp_dir.push(ang);
                        }
                    }
                }
            }
            let gs = vel.vx_mps.hypot(vel.vz_mps);
            let track_deg = (gs > 1.0).then(|| flight::track_deg(vel.vx_mps, vel.vz_mps));
            let fpa = (vel.speed_mps > 1.0).then(|| (vel.vy_mps / vel.speed_mps).clamp(-1.0, 1.0).asin().to_degrees());
            if vel.speed_mps > 60.0 {
                if let (Some(tk), Some(g)) = (track_deg, fpa) {
                    self.heading_track.push(flight::angle_diff_deg(ori.heading_deg, tk).abs());
                    self.pitch_gamma.push(ori.pitch_deg - g);
                }
            }

            if is_author {
                *self.authors.entry(e.uid).or_default() += 1;
                self.author_points.push(AuthorPoint {
                    t,
                    y: pos[1],
                    vy: vel.vy_mps,
                    speed: vel.speed_mps,
                    saturated: vel.saturated,
                    bank: ori.bank_deg,
                    pitch: ori.pitch_deg,
                });
            }

            let ts = &mut self.tracks[track];
            ts.end_ms = t;
            ts.samples += 1;
            ts.author_samples += is_author as u64;
            ts.saturated_speed_samples += vel.saturated as u64;
            ts.min_altitude_m = ts.min_altitude_m.min(pos[1]);
            ts.max_altitude_m = ts.max_altitude_m.max(pos[1]);
            ts.max_speed_mps = ts.max_speed_mps.max(vel.speed_mps);
            if let Some((_, dist, ..)) = step {
                ts.path_length_m += dist;
            }

            let u = self.uids.get_mut(&e.uid).unwrap();
            u.last = Some(Last { t, pos, vel });
            u.last_seen_ms = t;
            self.samples += 1;

            let sample = FlightSample {
                replay: self.replay.clone(),
                packet_index,
                entry_index: i as u32,
                timestamp_ms: t,
                track_id: format!("u{}.s{}", e.uid, segment),
                uid: e.uid,
                segment,
                is_author,
                position: Position { x_m: pos[0], y_m: pos[1], z_m: pos[2] },
                altitude_msl_m: pos[1],
                orientation: ori,
                velocity: vel,
                derived: Derived { groundspeed_mps: gs, vertical_speed_mps: vel.vy_mps, track_deg, flight_path_deg: fpa, fd_speed_mps: fd_speed },
                evidence: SampleEvidence {
                    position: Evidence::Verified,
                    altitude: Evidence::Verified,
                    orientation: Evidence::Verified,
                    velocity: Evidence::Verified,
                    derived: Evidence::Derived,
                    track_identity: Evidence::Experimental,
                    author: if is_author { Evidence::Experimental } else { Evidence::Unknown },
                },
                flags,
                raw: RawFields {
                    euler_hex: format!("0x{:08X}", s.euler_raw),
                    velocity_hex: format!("0x{:08X}", s.packed_velocity_raw),
                    velocity_bit18: vel.flag_bit18,
                    ground_flag: s.ground_flag,
                    unk_u32: s.unk_u32,
                    unk_bit: s.unk_bit,
                    controls_hex: hex(&s.controls),
                    engine_states: s.engines.iter().map(|e| e.state).collect(),
                    position_bit: s.position_bit,
                    entry_bit_offset: e.bit_offset,
                    entry_bit_length: e.bit_length,
                },
            };
            sink(&sample);
        }
    }

    fn cockpit_validation(&self) -> CockpitValidation {
        let mut v = CockpitValidation {
            cockpit_messages: self.cockpit.len() as u64,
            author_samples: self.author_points.len() as u64,
            ..Default::default()
        };
        if self.cockpit.is_empty() || self.author_points.is_empty() {
            return v;
        }
        let mut pts = self.author_points.clone();
        pts.sort_by_key(|p| p.t);
        let times: Vec<u32> = pts.iter().map(|p| p.t).collect();
        let (mut alt, mut vy, mut roll, mut pitch, mut ias) = (vec![], vec![], vec![], vec![], vec![]);
        let mut offsets = Vec::new();
        for c in &self.cockpit {
            // Cockpit messages are not simultaneous with sync records: compare
            // against the author state linearly interpolated (DERIVED) between
            // the bracketing sync records.
            let tc = c.timestamp_ms;
            let i = times.partition_point(|&t| t < tc);
            let (lo, hi) = match (i.checked_sub(1).map(|j| pts[j]), pts.get(i).copied()) {
                (_, Some(h)) if h.t == tc => (h, h),
                (Some(l), Some(h)) => (l, h),
                _ => continue,
            };
            let nearest = (tc - lo.t).min(hi.t - tc);
            if nearest > self.cfg.cockpit_match_ms || hi.t - lo.t > 2 * self.cfg.cockpit_match_ms + 50 {
                continue;
            }
            let w = if hi.t > lo.t { (tc - lo.t) as f64 / (hi.t - lo.t) as f64 } else { 0.0 };
            let lerp = |a: f64, b: f64| a + w * (b - a);
            let a = AuthorPoint {
                t: tc,
                y: lerp(lo.y, hi.y),
                vy: lerp(lo.vy, hi.vy),
                speed: lerp(lo.speed, hi.speed),
                saturated: lo.saturated || hi.saturated,
                bank: lo.bank + w * flight::angle_diff_deg(hi.bank, lo.bank),
                pitch: lerp(lo.pitch, hi.pitch),
            };
            v.matched += 1;
            offsets.push(nearest as f64);
            alt.push((a.y, cockpit::id::ALTITUDE_CANDIDATES.map(|id| c.get(id))));
            if let Some(x) = c.get(cockpit::id::VERTICAL_SPEED) {
                vy.push((a.vy, x));
            }
            if let Some(x) = c.get(cockpit::id::ROLL) {
                roll.push((a.bank, x));
            }
            if let Some(x) = c.get(cockpit::id::PITCH) {
                pitch.push((a.pitch, -x));
            }
            if let Some(x) = c.get(cockpit::id::AIRSPEED) {
                if a.speed > 60.0 && !a.saturated {
                    ias.push((a.speed * flight::isa_density_ratio(a.y).sqrt(), x));
                }
            }
        }
        // Altimeter ids and units differ between aircraft: pick the
        // (id, unit) pair that agrees with decoded Y most often.
        let mut best: Option<(usize, &'static str, f64, usize, usize)> = None;
        for (k, _) in cockpit::id::ALTITUDE_CANDIDATES.iter().enumerate() {
            for (unit, scale) in [("m", 1.0), ("ft", cockpit::id::METRES_PER_GAME_FOOT)] {
                let present = alt.iter().filter(|(_, h)| h[k].is_some()).count();
                let hits = alt.iter().filter(|(y, h)| h[k].is_some_and(|h| (y - h * scale).abs() < 2.0)).count();
                if present > 0 && best.is_none_or(|b| hits > b.3) {
                    best = Some((k, unit, scale, hits, present));
                }
            }
        }
        if let Some((k, unit, scale, hits, present)) = best {
            v.altimeter_param_id = Some(cockpit::id::ALTITUDE_CANDIDATES[k]);
            v.altimeter_unit = Some(unit);
            v.altitude_within_2m_fraction = Some(hits as f64 / present as f64);
            let conv: Vec<(f64, f64)> = alt.iter().filter_map(|(y, h)| h[k].map(|h| (*y, h * scale))).collect();
            v.altitude_m = Comparison::of(&conv, |a, b| a - b);
        }
        v.pairing_offset_ms = Dist::of(offsets);
        v.vertical_speed_mps = Comparison::of(&vy, |a, b| a - b);
        v.roll_deg = Comparison::of(&roll, flight::angle_diff_deg);
        v.pitch_deg = Comparison::of(&pitch, |a, b| a - b);
        v.ias_isa_mps = Comparison::of(&ias, |a, b| a - b);
        v
    }

    pub fn finish(self) -> FlightReport {
        let cockpit_validation = self.cockpit_validation();
        let mut author_uids: Vec<u16> = self.authors.keys().copied().collect();
        author_uids.sort();
        let mut track_summaries = self.tracks;
        for t in &mut track_summaries {
            if t.samples == 0 {
                t.min_altitude_m = f64::NAN;
                t.max_altitude_m = f64::NAN;
            }
        }
        FlightReport {
            schema: FLIGHT_SCHEMA,
            replay: self.replay,
            config: self.cfg,
            coverage: self.coverage,
            tracks: track_summaries.len() as u64,
            uids: self.uids.len() as u64,
            samples: self.samples,
            author_uids,
            segment_starts: track_summaries.iter().fold(Default::default(), |mut m, t| {
                *m.entry(t.start_reason).or_default() += 1;
                m
            }),
            flagged_jumps: track_summaries.iter().map(|t| t.flagged_jumps).sum(),
            kinematic_validation: KinematicValidation {
                displacement_residual_m: Dist::of(self.disp_res),
                displacement_direction_error_deg: Dist::of(self.disp_dir),
                author_displacement_residual_m: Dist::of(self.author_disp_res),
                author_displacement_direction_error_deg: Dist::of(self.author_disp_dir),
                heading_minus_track_abs_deg: Dist::of(self.heading_track),
                pitch_minus_flight_path_deg: Dist::of(self.pitch_gamma),
                timestamp_reversals: self.reversals,
            },
            cockpit_validation,
            track_summaries,
        }
    }
}
