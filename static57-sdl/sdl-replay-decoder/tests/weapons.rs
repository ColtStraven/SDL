//! Identity, battle log, weapon decoding and weapon joining (synthetic).
mod common;

use common::*;
use sdl_replay_decoder::combat::{self, CombatRecord, KillEvent};
use sdl_replay_decoder::ecs::{self, EntityCreate, Templates};
use sdl_replay_decoder::evidence::Evidence;
use sdl_replay_decoder::idfield;
use sdl_replay_decoder::shells;
use sdl_replay_decoder::units::{prefixed_strings, Registry};
use sdl_replay_decoder::weapons::{entity_key, shell_key, AircraftHistory, Collector};

const EID: u32 = (3 << 22) | 2888;

fn create(eid: u32, template: &str, data: Vec<u8>) -> EntityCreate {
    EntityCreate { eid, template: template.into(), data }
}

#[test]
fn shells_message_decodes_exactly() {
    let p = shells_payload(EID, [100.0, 2000.0, -50.0], [-261.42, -32.65, 465.04], 1.25);
    let m = shells::decode(&p).unwrap().unwrap();
    assert!(m.exact, "declared {} consumed {}", m.declared_bits, m.consumed_bits);
    assert_eq!(m.records.len(), 1);
    let r = &m.records[0];
    assert_eq!(r.list, 0);
    assert_eq!(r.position_m, [100.0, 2000.0, -50.0]);
    assert!(!r.position_packed);
    let d = r.dynamics.as_ref().unwrap();
    let v = d.velocity_mps();
    assert!((v[0] + 261.42).abs() < 0.07 && (v[2] - 465.04).abs() < 0.07);
    assert_eq!(d.timers_s, vec![1.25, 0.0, 0.0, 0.0]);
    assert_eq!(shell_key(r.eid), entity_key(EID));
}

#[test]
fn shells_rejects_bad_lengths_and_ignores_other_messages() {
    let mut p = shells_payload(EID, [1.0; 3], [1.0; 3], 1.0);
    p.truncate(p.len() - 6);
    assert!(shells::decode(&p).is_err());
    assert_eq!(shells::decode(&[0x02, 0x58, 0x58, 0xF0]).unwrap(), None);
    assert_eq!(shells::decode(&[0x02]).unwrap(), None);
}

#[test]
fn idfield_message_tables() {
    let p = kill_payload(443, 395, "us_m3a3_bradley", "us_agm_65b");
    let fields = idfield::parse(&p[4..]).unwrap();
    assert_eq!(fields.iter().map(|f| f.number).collect::<Vec<_>>(), [1, 2, 3, 4, 10]);
    assert_eq!(fields[0].bits, 32);
    assert_eq!(idfield::read_string(&p[4..], &fields[1]).unwrap(), "us_m3a3_bradley");
    assert!(idfield::parse(&[0xFF, 0xFF, 0x01]).is_err());
}

#[test]
fn kill_and_damage_records() {
    let CombatRecord::Kill(k) = combat::decode(9, 1, &kill_payload(443, 0x7FF, "f-5e", "us_aim9j_sidewinder")).unwrap() else { panic!() };
    assert_eq!(k.victim_uid, Some(443));
    assert_eq!(k.killer_uid, None, "0x7FF means no killer");
    assert_eq!(k.killer_vehicle.as_deref(), Some("f-5e"));
    assert_eq!(k.weapon.as_deref(), Some("us_aim9j_sidewinder"));
    assert_eq!(k.killer_slot, Some(7));
    let CombatRecord::Damage(d) = combat::decode(9, 2, &damage_payload(432, "su_24m", 101)).unwrap() else { panic!() };
    assert_eq!((d.kind, d.victim_uid, d.attacker_uid), ("critical", Some(432), Some(101)));
    assert_eq!(d.victim_vehicle.as_deref(), Some("su_24m"));
    assert!(combat::decode(0, 0, &[0x02, 0x58, 0x4A, 0xD0]).is_none());
}

#[test]
fn ecs_creation_framing_and_template_table() {
    let mut t = Templates::default();
    let a = ecs_create(EID, 4, "rocket", true, &[1, 2, 3]);
    let v = ecs::decode_creation(&a, &mut t).unwrap().unwrap();
    assert_eq!(v[0].eid, EID);
    assert_eq!(v[0].template, "rocket");
    assert_eq!(v[0].data, [1, 2, 3]);
    // The name is sent once: a later record refers to it by id alone.
    let b = ecs_create(EID + 1, 4, "", false, &[9]);
    let v = ecs::decode_creation(&b, &mut t).unwrap().unwrap();
    assert_eq!(v[0].template, "rocket");
    assert_eq!(v[0].data, [9]);
    assert!(ecs::decode_creation(&[0x26, 0], &mut t).is_none());
    assert!(ecs::decode_creation(&[0x24, 0, 1], &mut t).unwrap().is_err());
    // LZ4 variant.
    let mut lz = vec![0x25];
    lz.extend(lz4_literals(&a[1..]));
    let v = ecs::decode_creation(&lz, &mut Templates::default()).unwrap().unwrap();
    assert_eq!(v[0].template, "rocket");
}

#[test]
fn prefixed_strings_accept_short_names_only_before_another_string() {
    let mut d = vec![0x99, 2, b'f', b'1'];
    d.push(13);
    d.extend_from_slice(b"t1_player14_0");
    assert_eq!(prefixed_strings(&d, 3), ["f1", "t1_player14_0"]);
    assert!(prefixed_strings(&[2, b'a', b'b', 0xFF], 3).is_empty());
}

#[test]
fn registry_resolves_by_time_and_skips_placeholders() {
    let mut r = Registry::default();
    r.push(100, &create(1, "aircraft+player_unit", unit_data(50, [0.0; 3], &["dummy_plane", "t1_player01_0", "default"])));
    assert_eq!(r.units, 0);
    r.push(100, &create(2, "aircraft+player_unit", unit_data(50, [1.0, 2.0, 3.0], &["f-5e", "t1_player01_0", "custom1", "cannonM39"])));
    r.push(9000, &create(3, "aircraft+player_unit", unit_data(50, [4.0, 5.0, 6.0], &["mig_23ml", "t2_player02_0", "default"])));
    r.push(100, &create(4, "tank", unit_data(50, [0.0; 3], &["us_m60"])));
    assert_eq!(r.units, 2);
    assert_eq!(r.resolve(50, 50), Some(r.resolve(50, 100).unwrap()), "syncs may slightly precede creation");
    let u = r.resolve(50, 5000).unwrap();
    assert_eq!(u.vehicle.as_deref(), Some("f-5e"));
    assert_eq!(u.loadout.as_deref(), Some("custom1"));
    assert_eq!(u.equipment, ["cannonM39"]);
    assert_eq!(u.spawn_position_m, [1.0, 2.0, 3.0]);
    assert_eq!(r.resolve(50, 10_000).unwrap().vehicle.as_deref(), Some("mig_23ml"));
    assert!(r.resolve(51, 100).is_none());
    let mut late = Registry::default();
    late.push(5000, &create(6, "aircraft", unit_data(9, [0.0; 3], &["f-5e", "a_slot", "default"])));
    assert!(late.resolve(9, 4000).is_none());
    let hero = create(5, "aircraft+player_unit+controlled_hero+watched_hero", unit_data(7, [0.0; 3], &["ariete", "x_default", "default"]));
    r.push(0, &hero);
    assert!(r.resolve(7, 0).unwrap().controlled_by_author);
}

/// A valid LZ4 block made of one literal run.
fn lz4_literals(d: &[u8]) -> Vec<u8> {
    let mut out = vec![0xF0];
    let mut rest = d.len() - 15;
    while rest >= 255 {
        out.push(255);
        rest -= 255;
    }
    out.push(rest as u8);
    out.extend_from_slice(d);
    out
}

struct Scene {
    c: Collector,
    h: AircraftHistory,
}

/// Aircraft 10 fires at aircraft 20; weapon flies 2 s and ends beside it.
fn scene() -> Scene {
    let mut c = Collector::default();
    let mut h = AircraftHistory::default();
    for t in (0..=4000).step_by(250) {
        let shooter = [1000.0 + 0.25 * t as f32, 2000.0, 0.0];
        let target = [4000.0, 2100.0, 0.0];
        c.push_aircraft(t, 10, shooter.map(|v| v as f64), [250.0, 0.0, 0.0]);
        c.push_aircraft(t, 20, target.map(|v| v as f64), [0.0, 0.0, 0.0]);
        h.push(10, t, shooter);
        h.push(20, t, target);
    }
    // Creation happens with the aircraft positions current at t=4000; push a
    // fresh snapshot at launch time instead.
    c.push_aircraft(1000, 10, [1250.0, 2000.0, 0.0], [250.0, 0.0, 0.0]);
    c.push_aircraft(1000, 20, [4000.0, 2100.0, 0.0], [0.0, 0.0, 0.0]);
    c.push_create(1000, &create(EID, "rocket", store_data([1255.0, 2000.0, 0.0])));
    for (i, t) in [1100u32, 1600, 2100, 2600, 3100].iter().enumerate() {
        let x = 1300.0 + 600.0 * i as f32;
        let m = shells::decode(&shells_payload(EID, [x, 2050.0, 0.0], [800.0, 0.0, 0.0], (*t as f64 - 1000.0) / 1000.0)).unwrap().unwrap();
        c.push_shells(*t as u64, *t, &m);
    }
    Scene { c, h }
}

fn kill(t: u32, killer: u16, weapon: &str) -> KillEvent {
    KillEvent { timestamp_ms: t, packet_index: 77, victim_uid: Some(20), killer_uid: Some(killer), killer_slot: None, killer_vehicle: Some("f-5e".into()), weapon: Some(weapon.into()), destroyed_weapon: None }
}

#[test]
fn weapon_launcher_trajectory_and_kill_link() {
    let mut s = scene();
    s.c.push_kill(kill(3200, 10, "us_aim9j_sidewinder"));
    let mut reg = Registry::default();
    reg.push(0, &create(1, "aircraft+player_unit", unit_data(10, [0.0; 3], &["f-5e", "t1_player01_0", "aim9_loadout"])));
    reg.push(0, &create(2, "aircraft+player_unit", unit_data(20, [0.0; 3], &["mig_23ml", "t2_player02_0", "default"])));
    let (ev, samples, stats) = s.c.finish(&reg, &s.h, &[10]);
    assert_eq!(ev.len(), 1);
    let w = &ev[0];
    assert_eq!(w.template, "rocket");
    assert_eq!(w.launcher.uid, Some(10));
    assert_eq!(w.launcher.status, "attributed_by_proximity");
    assert!(w.launcher.is_author);
    assert_eq!(w.launcher.vehicle.as_deref(), Some("f-5e"));
    assert_eq!(w.launcher.loadout.as_deref(), Some("aim9_loadout"));
    assert!(w.launcher.distance_m.unwrap() < 10.0);
    // First timer 0.1 s quantizes to 5/48 s.
    assert_eq!(w.launch_time_ms, Some(996));
    let t = &w.trajectory;
    assert_eq!(t.samples, 5);
    assert_eq!(t.launch_altitude_m, Some(2000.0));
    assert!((t.time_of_flight_s.unwrap() - 2.104).abs() < 0.02);
    assert_eq!(t.max_sample_interval_s, Some(0.5));
    assert!((t.path_length_m - 2400.0).abs() < 1.0);
    assert_eq!(w.outcome.status, "kill_linked");
    assert_eq!(w.outcome.evidence, Evidence::Derived);
    assert_eq!(w.outcome.victim_vehicle.as_deref(), Some("mig_23ml"));
    assert_eq!(w.weapon_name.as_deref(), Some("us_aim9j_sidewinder"));
    assert!((w.outcome.target_distance_at_launch_m.unwrap() - 2747.0).abs() < 2.0);
    assert_eq!(w.outcome.target_altitude_at_launch_m, Some(2100.0));
    assert_eq!(stats.weapons_by_author, 1);
    assert_eq!(stats.kills_linked_to_weapon, 1);
    assert_eq!(samples[0].1.len(), 5);
}

#[test]
fn unrelated_kill_leaves_outcome_unknown_and_name_null() {
    // Another aircraft killed the target with guns while the missile passed.
    let mut s = scene();
    s.h.push(30, 1000, [9000.0, 1000.0, 0.0]);
    s.c.push_kill(kill(3200, 30, "30mm_UBR6"));
    let (ev, _, stats) = s.c.finish(&Registry::default(), &s.h, &[]);
    let w = &ev[0];
    assert_eq!(w.outcome.status, "unknown");
    assert_eq!(w.outcome.evidence, Evidence::Unknown);
    assert_eq!(w.weapon_name, None);
    assert_eq!(w.weapon_name_evidence, Evidence::Unknown);
    assert_eq!(w.outcome.target_distance_at_launch_m, None);
    assert_eq!(stats.kills_linked_to_weapon, 0);
    assert!(w.outcome.nearest_aircraft_m.is_some());
}

#[test]
fn a_kill_picks_the_nearest_weapon() {
    let mut s = scene();
    let other = (3 << 22) | 2900;
    s.c.push_create(1000, &create(other, "rocket", store_data([1255.0, 2000.0, 0.0])));
    let m = shells::decode(&shells_payload(other, [3900.0, 2100.0, 0.0], [700.0, 0.0, 0.0], 2.0)).unwrap().unwrap();
    s.c.push_shells(5, 3050, &m);
    s.c.push_kill(kill(3200, 10, "us_aim9j_sidewinder"));
    let (ev, ..) = s.c.finish(&Registry::default(), &s.h, &[]);
    let linked: Vec<_> = ev.iter().filter(|w| w.outcome.status == "kill_linked").collect();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].weapon_id, format!("w{}.{}", 2900, 3));
}

#[test]
fn entity_id_reuse_starts_a_new_track() {
    let mut s = scene();
    let m = shells::decode(&shells_payload(EID, [0.0, 500.0, 0.0], [300.0, 0.0, 0.0], 0.2)).unwrap().unwrap();
    s.c.push_shells(9, 3200, &m);
    let (ev, ..) = s.c.finish(&Registry::default(), &s.h, &[]);
    assert_eq!(ev.len(), 2);
    assert_eq!(ev[0].trajectory.samples, 5);
    assert_eq!(ev[1].trajectory.samples, 1);
    assert_ne!(ev[0].weapon_id, ev[1].weapon_id);
}

#[test]
fn weapon_without_creation_is_explicitly_unavailable() {
    let mut c = Collector::default();
    let m = shells::decode(&shells_payload(EID, [5.0, 5.0, 5.0], [100.0, 0.0, 0.0], 3.0)).unwrap().unwrap();
    c.push_shells(1, 5000, &m);
    let (ev, ..) = c.finish(&Registry::default(), &AircraftHistory::default(), &[]);
    let w = &ev[0];
    assert_eq!(w.template, "unlinked");
    assert_eq!(w.creation_prefix, None);
    assert_eq!(w.trajectory.launch_position_m, None);
    assert_eq!(w.launcher.status, "unavailable");
    assert_eq!(w.launcher.evidence, Evidence::Unknown);
    assert_eq!(w.launch_time_ms, Some(2000));
    assert_eq!(w.trajectory.max_sample_interval_s, None);
}
