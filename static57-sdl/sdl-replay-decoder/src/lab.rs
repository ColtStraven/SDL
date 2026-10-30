//! Full-replay extraction pass: flight tracks, unit identity, battle log and
//! weapons, gathered in one read of the packet stream.

use crate::combat::{self, CombatRecord, DamageEvent, KillEvent};
use crate::ecs;
use crate::export::replay_ref;
use crate::manifest::{self, Depth, Manifest, Options};
use crate::packets::{PacketRecord, RecordKind};
use crate::shells;
use crate::tracks::{FlightReport, FlightSample, Reconstructor, TrackConfig};
use crate::units::Registry;
use crate::weapons::{self, AircraftHistory, WeaponEvent, WeaponSample, WeaponStats};
use std::io;

pub struct Extraction {
    pub manifest: Manifest,
    pub flight: FlightReport,
    pub registry: Registry,
    pub kills: Vec<KillEvent>,
    pub damage: Vec<DamageEvent>,
    pub weapons: Vec<WeaponEvent>,
    pub weapon_samples: Vec<(String, Vec<WeaponSample>)>,
    pub weapon_stats: WeaponStats,
    pub ecs_creation_packets: u64,
    pub ecs_creation_failures: u64,
}

/// Read a replay once and extract everything SDL can decode. `sink` receives
/// every flight sample in stream order.
pub fn extract(
    file_name: &str,
    file: &[u8],
    opts: Options,
    cfg: TrackConfig,
    mut sink: impl FnMut(&FlightSample),
) -> io::Result<Extraction> {
    let mut rec = Reconstructor::new(replay_ref(file), cfg);
    let mut templates = ecs::Templates::default();
    let mut registry = Registry::default();
    let mut collector = weapons::Collector::default();
    let mut history = AircraftHistory::default();
    let (mut kills, mut damage) = (Vec::new(), Vec::new());
    let (mut creation_packets, mut creation_failures) = (0u64, 0u64);
    let opts = Options { depth: Depth::Packets, ..opts };

    let manifest = manifest::analyze(file_name, file, opts, |p: &PacketRecord, payload| {
        if p.record != RecordKind::Packet {
            return Ok(());
        }
        let t = p.timestamp_ms.unwrap_or(0);
        match p.type_id {
            Some(2) => {
                let d = crate::aircraft::decode_sync(payload);
                rec.push_sync(p.index, t, payload.len() as u64 * 8, &d, |s| {
                    let v = &s.velocity;
                    let pos = [s.position.x_m, s.position.y_m, s.position.z_m];
                    collector.push_aircraft(t, s.uid, pos, [v.vx_mps, v.vy_mps, v.vz_mps]);
                    history.push(s.uid, t, pos.map(|c| c as f32));
                    sink(s);
                });
            }
            Some(4) => {
                if let Some(c) = crate::cockpit::decode(t, payload) {
                    rec.push_cockpit(c);
                } else {
                    match shells::decode(payload) {
                        Ok(Some(m)) => collector.push_shells(p.index, t, &m),
                        Ok(None) => match combat::decode(t, p.index, payload) {
                            Some(CombatRecord::Kill(k)) => {
                                kills.push(k.clone());
                                collector.push_kill(k);
                            }
                            Some(CombatRecord::Damage(d)) => damage.push(d),
                            None => {}
                        },
                        Err(_) => collector.note_failed_message(),
                    }
                }
            }
            Some(6) => {
                if let Some(r) = ecs::decode_creation(payload, &mut templates) {
                    creation_packets += 1;
                    match r {
                        Ok(v) => {
                            for e in &v {
                                registry.push(t, e);
                                collector.push_create(t, e);
                            }
                        }
                        Err(_) => creation_failures += 1,
                    }
                }
            }
            _ => {}
        }
        Ok(())
    })?;

    let flight = rec.finish();
    let (weapons, weapon_samples, weapon_stats) = collector.finish(&registry, &history, &flight.author_uids);
    Ok(Extraction {
        manifest,
        flight,
        registry,
        kills,
        damage,
        weapons,
        weapon_samples,
        weapon_stats,
        ecs_creation_packets: creation_packets,
        ecs_creation_failures: creation_failures,
    })
}
