//! Identity, battle log, weapon decoding and weapon joining (synthetic).
mod common;

use common::*;
use sdl_replay_decoder::aircraft::TargetRecord;
use sdl_replay_decoder::combat::{self, CombatRecord, DamageEvent, HitMarker, KillEvent};
use sdl_replay_decoder::ecs::{self, EntityCreate, Templates};
use sdl_replay_decoder::evidence::Evidence;
use sdl_replay_decoder::idfield;
use sdl_replay_decoder::seeker;
use sdl_replay_decoder::shells;
use sdl_replay_decoder::units::{prefixed_strings, weapon_equipment, Registry};
use sdl_replay_decoder::weapons::{base_name, entity_key, is_gun_round, launch_geometry, shell_key, AircraftHistory, Collector};

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

/// Entity 1 (uid 10, f-5e) fires at entity 2 (uid 20, mig_23ml) at t=1000.
fn registry() -> Registry {
    let mut r = Registry::default();
    r.push(0, &create(1, "aircraft+player_unit", unit_data(10, [0.0; 3], &["f-5e", "t1_player01_0", "aim9_loadout"])));
    r.push(0, &create(2, "aircraft+player_unit", unit_data(20, [0.0; 3], &["mig_23ml", "t2_player02_0", "default"])));
    r
}

fn scene() -> Scene {
    let mut c = Collector::default();
    let mut h = AircraftHistory::default();
    for t in (0..=20_000).step_by(250) {
        h.push(10, t, [1000.0 + 0.25 * t as f32, 2000.0, 0.0], [250.0, 0.0, 0.0]);
        h.push(20, t, [4000.0, 2100.0, 0.0], [0.0, 0.0, 0.0]);
    }
    c.push_create(1000, &create(EID, "rocket", store_data([1255.0, 2000.0, 0.0])));
    for (i, t) in [1100u32, 1600, 2100, 2600, 3100].iter().enumerate() {
        let x = 1300.0 + 600.0 * i as f32;
        let m = shells::decode(&shells_payload(EID, [x, 2050.0, 0.0], [800.0, 0.0, 0.0], (*t as f64 - 1000.0) / 1000.0)).unwrap().unwrap();
        c.push_shells(*t as u64, *t, &m);
    }
    Scene { c, h }
}

fn kill(t: u32, killer: u16, weapon: &str) -> KillEvent {
    KillEvent {
        timestamp_ms: t,
        packet_index: 77,
        victim_uid: Some(20),
        killer_uid: Some(killer),
        victim_surface: false,
        killer_surface: false,
        killer_slot: None,
        killer_vehicle: Some("f-5e".into()),
        weapon: Some(weapon.into()),
        destroyed_weapon: None,
    }
}

/// A second weapon from entity 1 ending at `end`.
fn second_weapon(s: &mut Scene, eid: u32, type_id: u8, end: [f32; 3]) {
    s.c.push_create(1000, &create(eid, "rocket", store_data_owned(type_id, 1, [1255.0, 2000.0, 0.0])));
    fly(s, eid, [2500.0, 2050.0, 0.0], end);
}

/// Two samples, ending at `end` at t=3000, so the weapon has a measured path.
fn fly(s: &mut Scene, eid: u32, mid: [f32; 3], end: [f32; 3]) {
    for (t, p) in [(2000u32, mid), (3000, end)] {
        let m = shells::decode(&shells_payload(eid, p, [700.0, 0.0, 0.0], (t as f64 - 1000.0) / 1000.0)).unwrap().unwrap();
        s.c.push_shells(t as u64, t, &m);
    }
}

#[test]
fn launcher_comes_from_the_creation_record() {
    let mut s = scene();
    s.c.push_kill(kill(3200, 10, "us_aim9j_sidewinder"));
    let (ev, samples, stats) = s.c.finish(&registry(), &s.h, &[10]);
    assert_eq!(ev.len(), 1);
    let w = &ev[0];
    assert_eq!(w.template, "rocket");
    assert_eq!(w.launcher.status, "creation_record");
    assert_eq!(w.launcher.evidence, Evidence::Verified);
    assert_eq!(w.launcher.entity_id, Some(1));
    assert_eq!(w.launcher.kind, Some("aircraft"));
    assert_eq!(w.launcher.uid, Some(10));
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
    assert_eq!(w.outcome.class, "confirmed_kill");
    assert_eq!(w.outcome.status, "kill_linked");
    assert_eq!(w.outcome.evidence, Evidence::Derived);
    assert_eq!(w.outcome.victim_vehicle.as_deref(), Some("mig_23ml"));
    assert_eq!(w.weapon_name.as_deref(), Some("us_aim9j_sidewinder"));
    assert_eq!(w.target.status, "kill_record");
    assert_eq!(w.target.vehicle.as_deref(), Some("mig_23ml"));
    assert_eq!(w.target.kind, Some("aircraft"));
    assert_eq!(stats.weapons_by_author, 1);
    assert_eq!(stats.launchers_exact, 1);
    assert_eq!(stats.weapons_with_launcher, 1);
    assert_eq!(stats.kills_linked_to_weapon, 1);
    assert_eq!(samples[0].1.len(), 5);
}

#[test]
fn launch_geometry_against_a_tracked_target() {
    let mut s = scene();
    s.c.push_kill(kill(3200, 10, "us_aim9j_sidewinder"));
    let (ev, ..) = s.c.finish(&registry(), &s.h, &[]);
    let g = ev[0].geometry.as_ref().unwrap();
    assert!((g.target_distance_m - 2746.8).abs() < 2.0, "{}", g.target_distance_m);
    assert_eq!(g.target_altitude_m, 2100.0);
    assert_eq!(g.altitude_difference_m, 100.0);
    assert!(g.relative_bearing_deg.unwrap() < 3.0);
    assert!((g.closing_speed_mps.unwrap() - 250.0).abs() < 1.0);
    assert_eq!(g.target_speed_mps, 0.0);
    assert_eq!(g.target_aspect_deg, None, "a stationary target has no aspect");
    assert_eq!(g.evidence, Evidence::Derived);
}

#[test]
fn geometry_function_handles_aspect() {
    // Target 1 km ahead on +X flying toward the launcher: head-on.
    let g = launch_geometry([0.0; 3], Some([200.0, 0.0, 0.0]), ([1000.0, 0.0, 0.0], [-200.0, 0.0, 0.0])).unwrap();
    assert_eq!(g.target_aspect_deg, Some(0.0));
    assert_eq!(g.closing_speed_mps, Some(400.0));
    assert_eq!(g.relative_bearing_deg, Some(0.0));
    // Same target flying away: tail-on, opening.
    let g = launch_geometry([0.0; 3], Some([200.0, 0.0, 0.0]), ([1000.0, 0.0, 0.0], [300.0, 0.0, 0.0])).unwrap();
    assert_eq!(g.target_aspect_deg, Some(180.0));
    assert_eq!(g.closing_speed_mps, Some(-100.0));
    assert!(launch_geometry([0.0; 3], None, ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])).is_none());
}

#[test]
fn an_aircraft_at_the_launch_point_is_not_credited() {
    // Entity 99 does not exist. Uid 10 sits at the launch point, and must
    // not become the launcher by proximity.
    let mut c = Collector::default();
    let mut h = AircraftHistory::default();
    for t in (0..=4000).step_by(250) {
        h.push(10, t, [1255.0, 2000.0, 0.0], [0.0, 0.0, 0.0]);
    }
    c.push_create(1000, &create(EID, "rocket", store_data_owned(0x0A, 99, [1255.0, 2000.0, 0.0])));
    let m = shells::decode(&shells_payload(EID, [1300.0, 2000.0, 0.0], [300.0, 0.0, 0.0], 0.1)).unwrap().unwrap();
    c.push_shells(1, 1100, &m);
    let (ev, _, stats) = c.finish(&registry(), &h, &[10]);
    let l = &ev[0].launcher;
    assert_eq!(l.status, "entity_not_found");
    assert_eq!(l.uid, None);
    assert_eq!(l.vehicle, None);
    assert_eq!(l.entity_id, Some(99));
    assert!(!l.is_author);
    assert_eq!(l.evidence, Evidence::Unknown);
    assert_eq!(stats.weapons_with_launcher, 0);
}

#[test]
fn respawn_generation_selects_the_right_entity() {
    // Same index, new generation: a different life of the same uid.
    let (old, new) = (5, (15 << 22) | 5);
    let mut r = Registry::default();
    r.push(0, &create(old, "aircraft+player_unit", unit_data(10, [0.0; 3], &["f-5e", "a_slot", "default"])));
    r.push(500, &create(new, "aircraft+player_unit", unit_data(10, [0.0; 3], &["mig_23ml", "a_slot", "default"])));
    let mut c = Collector::default();
    c.push_create(1000, &create(EID, "rocket", store_data_owned(0x0A, new, [1.0, 2.0, 3.0])));
    let m = shells::decode(&shells_payload(EID, [5.0, 5.0, 5.0], [100.0, 0.0, 0.0], 0.1)).unwrap().unwrap();
    c.push_shells(1, 1100, &m);
    let (ev, ..) = c.finish(&r, &AircraftHistory::default(), &[]);
    assert_eq!(ev[0].launcher.vehicle.as_deref(), Some("mig_23ml"));
    assert_eq!(ev[0].launcher.entity_id, Some(new));
}

#[test]
fn ground_launcher_and_surface_kill_flags() {
    let ground = 7;
    let mut d = unit_data(33, [4000.0, 50.0, 0.0], &["tankModels/fr_crotale_ng", "a_slot"]);
    d[28] = 0x38;
    let mut r = registry();
    r.push(0, &create(ground, "tank+sam_site_unit", d));
    let u = r.surface_unit(33, 100).unwrap();
    assert_eq!(u.kind, "ground");
    assert_eq!(u.vehicle.as_deref(), Some("fr_crotale_ng"));
    assert!(r.resolve(33, 100).is_none(), "ground units stay out of the aircraft registry");

    let mut s = scene();
    s.c.push_create(1000, &create(EID + 1, "rocket", store_data_owned(0x0B, ground, [4000.0, 60.0, 0.0])));
    fly(&mut s, EID + 1, [4000.0, 1000.0, 0.0], [3990.0, 2090.0, 0.0]);
    // An aircraft with the same uid number is a different unit.
    s.c.push_kill(KillEvent { killer_surface: true, ..kill(3100, 33, "165mm_vt_1") });
    let (ev, ..) = s.c.finish(&r, &s.h, &[]);
    let w = ev.iter().find(|w| w.launcher.kind == Some("ground")).unwrap();
    assert_eq!(w.launcher.uid, Some(33));
    assert!(!w.launcher.is_author);
    assert_eq!(w.outcome.class, "confirmed_kill");
    assert_eq!(w.weapon_name.as_deref(), Some("165mm_vt_1"));
}

#[test]
fn unit_uid_marker_can_move() {
    let mut d = vec![0u8; 4];
    d.extend(unit_data(44, [7.0, 8.0, 9.0], &["f-5e", "a_slot", "default"]));
    let mut r = Registry::default();
    r.push(0, &create(1, "aircraft", d));
    let u = r.resolve(44, 0).unwrap();
    assert_eq!(u.vehicle.as_deref(), Some("f-5e"));
    assert_eq!(u.spawn_position_m, [7.0, 8.0, 9.0]);
}

#[test]
fn unrelated_kill_leaves_outcome_unknown_and_name_null() {
    let mut s = scene();
    s.c.push_kill(kill(3200, 30, "us_aim9j_sidewinder"));
    let (ev, _, stats) = s.c.finish(&registry(), &s.h, &[]);
    let w = &ev[0];
    assert_eq!(w.outcome.class, "unknown");
    assert_eq!(w.outcome.evidence, Evidence::Unknown);
    assert_eq!(w.weapon_name, None);
    assert_eq!(w.weapon_name_evidence, Evidence::Unknown);
    assert_eq!(w.target.status, "unavailable");
    assert_eq!(w.geometry, None);
    assert_eq!(stats.kills_linked_to_weapon, 0);
}

#[test]
fn a_weapon_with_no_record_is_unknown_never_a_miss() {
    let s = scene();
    let (ev, _, stats) = s.c.finish(&registry(), &s.h, &[]);
    assert_eq!(ev[0].outcome.class, "unknown");
    assert_eq!(ev[0].outcome.basis, None);
    assert_eq!(stats.outcome_classes.get("unknown"), Some(&1));
    assert_eq!(stats.outcome_classes.len(), 1);
}

#[test]
fn a_kill_picks_the_nearest_weapon() {
    let mut s = scene();
    second_weapon(&mut s, (3 << 22) | 2900, 0x0B, [3900.0, 2100.0, 0.0]);
    s.c.push_kill(kill(3200, 10, "us_aim9j_sidewinder"));
    let (ev, ..) = s.c.finish(&registry(), &s.h, &[]);
    let linked: Vec<_> = ev.iter().filter(|w| w.outcome.class == "confirmed_kill").collect();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].weapon_id, format!("w{}.{}", 2900, 3));
}

#[test]
fn two_weapons_at_the_victim_are_ambiguous() {
    let mut s = scene();
    second_weapon(&mut s, (3 << 22) | 2900, 0x0B, [3800.0, 2100.0, 0.0]);
    s.c.push_kill(kill(3200, 10, "us_aim9j_sidewinder"));
    let (ev, _, stats) = s.c.finish(&registry(), &s.h, &[]);
    assert!(ev.iter().all(|w| w.outcome.class == "ambiguous"), "{:?}", ev.iter().map(|w| w.outcome.class).collect::<Vec<_>>());
    assert!(ev.iter().all(|w| w.weapon_name.is_none()), "an ambiguous link supplies no name");
    assert_eq!(stats.named_by_kill_record, 0);
}

#[test]
fn impossible_associations_are_rejected() {
    // The victim was 60 km away at launch: this weapon cannot have reached it.
    let mut c = Collector::default();
    let mut h = AircraftHistory::default();
    for t in (0..=4000).step_by(250) {
        h.push(10, t, [0.0, 2000.0, 0.0], [0.0, 0.0, 0.0]);
    }
    h.push(20, 1000, [60_000.0, 2000.0, 0.0], [0.0, 0.0, 0.0]);
    h.push(20, 4000, [1000.0, 2000.0, 0.0], [0.0, 0.0, 0.0]);
    c.push_create(1000, &create(EID, "rocket", store_data([0.5, 2000.0, 0.0])));
    for (i, t) in [1500u32, 3500].iter().enumerate() {
        let m = shells::decode(&shells_payload(EID, [500.0 * (i + 1) as f32, 2000.0, 0.0], [500.0, 0.0, 0.0], (*t as f64 - 1000.0) / 1000.0)).unwrap().unwrap();
        c.push_shells(*t as u64, *t, &m);
    }
    c.push_kill(kill(3600, 10, "us_aim9j_sidewinder"));
    let (ev, _, stats) = c.finish(&registry(), &h, &[]);
    assert_eq!(ev[0].outcome.class, "unknown");
    assert_eq!(stats.rejected_impossible_links, 1);
}

#[test]
fn damage_and_hit_records_set_their_classes() {
    let damage = DamageEvent {
        timestamp_ms: 3200,
        packet_index: 8,
        kind: "critical",
        victim_uid: Some(20),
        victim_slot: None,
        victim_vehicle: Some("f-5e".into()),
        attacker_uid: Some(10),
        victim_surface: false,
        attacker_surface: false,
    };
    let mut s = scene();
    s.c.push_damage(damage.clone());
    let (ev, ..) = s.c.finish(&registry(), &s.h, &[]);
    assert_eq!(ev[0].outcome.class, "confirmed_damage");
    assert_eq!(ev[0].outcome.evidence, Evidence::Derived);

    // Another attacker's damage says nothing about this weapon.
    let mut s = scene();
    s.c.push_damage(DamageEvent { attacker_uid: Some(30), ..damage });
    let (ev, ..) = s.c.finish(&registry(), &s.h, &[]);
    assert_eq!(ev[0].outcome.class, "unknown");

    let hit = HitMarker { timestamp_ms: 3200, packet_index: 9, target_uid: Some(20), target_surface: false };
    let mut s = scene();
    s.c.push_hit(hit.clone());
    let (ev, ..) = s.c.finish(&registry(), &s.h, &[10]);
    assert_eq!(ev[0].outcome.class, "confirmed_hit");
    // Hit markers belong to the author only.
    let mut s = scene();
    s.c.push_hit(hit);
    let (ev, ..) = s.c.finish(&registry(), &s.h, &[]);
    assert_eq!(ev[0].outcome.class, "unknown");
}

#[test]
fn seeker_blocks_are_parsed_by_size() {
    let mut w = BitWriter::default();
    w.bits(8, 0).bits(3, 0b111).f32(100.0).f32(200.0).f32(300.0);
    while w.bits < 607 {
        w.bit(false);
    }
    let o = seeker::parse(&w.bytes, 607);
    assert_eq!(o.mode, 7);
    assert_eq!(o.target_estimate_m, Some([100.0, 200.0, 300.0]));

    let mut w = BitWriter::default();
    w.bits(58, 0).u16(32767).u16(0).u16(0);
    while w.bits < 283 {
        w.bit(false);
    }
    let o = seeker::parse(&w.bytes, 283);
    assert_eq!(o.line_of_sight, Some([1.0, 0.0, 0.0]));
    assert_eq!(o.target_estimate_m, None);

    let o = seeker::parse(&[0xFF; 40], 305);
    assert_eq!(o.target_estimate_m, None);
    assert_eq!(o.line_of_sight, None);
}

#[test]
fn seeker_observations_establish_a_target() {
    let mut w = BitWriter::default();
    w.bits(8, 0).bits(3, 0b111).f32(4010.0).f32(2100.0).f32(0.0);
    while w.bits < 607 {
        w.bit(false);
    }
    let block = w.bytes.clone();
    let mut s = scene();
    for t in [3200u32, 3400] {
        let m = shells::decode(&shells_payload_seeker(EID, [3800.0, 2100.0, 0.0], [800.0, 0.0, 0.0], (t as f64 - 1000.0) / 1000.0, 607, &block)).unwrap().unwrap();
        s.c.push_shells(t as u64, t, &m);
    }
    let (ev, _, stats) = s.c.finish(&registry(), &s.h, &[]);
    let t = &ev[0].target;
    assert_eq!(t.status, "seeker");
    assert_eq!(t.uid, Some(20));
    assert_eq!(t.vehicle.as_deref(), Some("mig_23ml"));
    assert_eq!(t.method.as_deref(), Some("seeker_estimate"));
    assert_eq!(t.evidence, Evidence::Derived);
    assert_eq!(stats.targets_seeker, 1);
    assert!(ev[0].geometry.is_some());
    assert_eq!(ev[0].outcome.class, "unknown", "a target does not establish an outcome");
}

#[test]
fn a_designation_record_is_only_a_candidate() {
    let mut s = scene();
    s.c.push_designations(10, 900, &[TargetRecord { kind: 6, second: 0, position: None, tail: Some(0xFFFF_0014) }]);
    let (ev, ..) = s.c.finish(&registry(), &s.h, &[]);
    let t = &ev[0].target;
    assert_eq!(t.status, "unavailable");
    assert_eq!(t.uid, None);
    assert_eq!(t.designated_candidate_uid, Some(20));
    assert_eq!(t.evidence, Evidence::Unknown);
    assert_eq!(ev[0].geometry, None);
}

#[test]
fn a_weapon_first_seen_long_after_launch_has_no_launch_data() {
    let mut c = Collector::default();
    let mut h = AircraftHistory::default();
    for t in (0..=40_000).step_by(250) {
        h.push(10, t, [0.0, 2000.0, 0.0], [100.0, 0.0, 0.0]);
    }
    // Created 30 s into flight: this position is mid-flight, not a launch.
    c.push_create(31_000, &create(EID, "rocket", store_data([9000.0, 2000.0, 0.0])));
    let m = shells::decode(&shells_payload(EID, [9100.0, 2000.0, 0.0], [300.0, 0.0, 0.0], 30.1)).unwrap().unwrap();
    c.push_shells(1, 31_100, &m);
    let (ev, ..) = c.finish(&registry(), &h, &[]);
    let w = &ev[0];
    assert!(w.trajectory.launch_late);
    assert_eq!(w.trajectory.launch_position_m, None);
    assert_eq!(w.trajectory.launch_altitude_m, None);
    assert_eq!(w.launcher.distance_m, None);
    assert_eq!(w.launcher.status, "creation_record", "the launcher is still exact");
    assert_eq!(w.geometry, None);
}

#[test]
fn entity_id_reuse_starts_a_new_track() {
    let mut s = scene();
    let m = shells::decode(&shells_payload(EID, [0.0, 500.0, 0.0], [300.0, 0.0, 0.0], 0.2)).unwrap().unwrap();
    s.c.push_shells(9, 3200, &m);
    let (ev, ..) = s.c.finish(&registry(), &s.h, &[]);
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
    assert_eq!(w.launcher.status, "no_creation_record");
    assert_eq!(w.launcher.evidence, Evidence::Unknown);
    assert_eq!(w.launch_time_ms, Some(2000));
    assert_eq!(w.trajectory.max_sample_interval_s, None);
}

/// One more shot from entity 1 that ends far from the target.
fn extra_shot(s: &mut Scene, eid: u32, type_id: u8) {
    s.c.push_create(1000, &create(eid, "rocket", store_data_typed(type_id, [1255.0, 2000.0, 0.0])));
    let m = shells::decode(&shells_payload(eid, [1500.0, 2000.0, 9000.0], [700.0, 0.0, 0.0], 1.0)).unwrap().unwrap();
    s.c.push_shells(6, 2000, &m);
}

#[test]
fn type_id_carries_a_kill_name_to_other_shots() {
    let mut s = scene(); // the scene's weapon has type id 0x0A
    extra_shot(&mut s, (3 << 22) | 2901, 0x0A);
    s.c.push_kill(kill(3200, 10, "us_aim9j_sidewinder_default"));
    let (ev, _, stats) = s.c.finish(&registry(), &s.h, &[]);
    let hit = ev.iter().find(|w| w.outcome.class == "confirmed_kill").unwrap();
    assert_eq!(hit.weapon_name_source, "kill_record");
    assert_eq!(hit.weapon_name.as_deref(), Some("us_aim9j_sidewinder_default"));
    let other = ev.iter().find(|w| w.outcome.class == "unknown").unwrap();
    assert_eq!(other.type_id, Some(0x0A));
    assert_eq!(other.weapon_name.as_deref(), Some("us_aim9j_sidewinder"));
    assert_eq!(other.weapon_name_source, "replay_type_id");
    assert_eq!(other.weapon_name_evidence, Evidence::Derived);
    assert_eq!(other.outcome.evidence, Evidence::Unknown, "a name does not establish an outcome");
    assert_eq!(stats.named_by_kill_record, 1);
    assert_eq!(stats.named_by_type_id, 1);
    assert_eq!(stats.weapons_named, 2);
}

#[test]
fn different_type_id_is_not_named() {
    let mut s = scene();
    extra_shot(&mut s, (3 << 22) | 2901, 0x0B);
    s.c.push_kill(kill(3200, 10, "us_aim9j_sidewinder"));
    let (ev, ..) = s.c.finish(&registry(), &s.h, &[]);
    let other = ev.iter().find(|w| w.type_id == Some(0x0B)).unwrap();
    assert_eq!(other.weapon_name, None);
    assert_eq!(other.weapon_name_source, "unavailable");
    assert_eq!(other.weapon_name_evidence, Evidence::Unknown);
}

#[test]
fn equipment_gives_candidates_but_never_a_name() {
    let s = scene();
    let mut reg = Registry::default();
    reg.push(0, &create(1, "aircraft+player_unit", unit_data(10, [0.0; 3], &["f-5e", "t1_player01_0", "custom1", "us_aim9j_sidewinder", "countermeasure_split_launcher_jet"])));
    let (ev, ..) = s.c.finish(&reg, &s.h, &[]);
    let w = &ev[0];
    assert_eq!(w.weapon_name, None, "equipment failed hold-out and must not name a weapon");
    assert_eq!(w.weapon_candidates, ["us_aim9j_sidewinder"]);
    assert_eq!(w.weapon_name_source, "unavailable");
}

#[test]
fn disputed_type_id_stays_unnamed_and_lists_the_names() {
    // Two weapons of one id, each killing with a different recorded name.
    let mut s = scene();
    second_weapon(&mut s, (3 << 22) | 2901, 0x0A, [3990.0, 2100.0, 0.0]);
    extra_shot(&mut s, (3 << 22) | 2902, 0x0A);
    for t in (0..=20_000).step_by(250) {
        s.h.push(21, t, [3700.0, 2050.0, 0.0], [0.0, 0.0, 0.0]);
    }
    s.c.push_kill(kill(3100, 10, "us_aim9j_sidewinder"));
    s.c.push_kill(KillEvent { victim_uid: Some(21), weapon: Some("us_aim7e_sparrow".into()), ..kill(3200, 10, "") });
    let (ev, ..) = s.c.finish(&registry(), &s.h, &[]);
    assert_eq!(ev.iter().filter(|w| w.outcome.status == "kill_linked").count(), 2);
    let third = ev.iter().find(|w| w.outcome.status == "unknown").unwrap();
    assert_eq!(third.weapon_name, None);
    assert_eq!(third.weapon_candidates, ["us_aim7e_sparrow", "us_aim9j_sidewinder"]);
}

#[test]
fn hold_out_hides_each_kill_name() {
    let mut s = scene();
    second_weapon(&mut s, (3 << 22) | 2901, 0x0A, [3990.0, 2100.0, 0.0]);
    for t in (0..=20_000).step_by(250) {
        s.h.push(21, t, [3700.0, 2050.0, 0.0], [0.0, 0.0, 0.0]);
    }
    s.c.push_kill(kill(3100, 10, "us_aim9j_sidewinder"));
    s.c.push_kill(KillEvent { victim_uid: Some(21), ..kill(3200, 10, "us_aim9j_sidewinder_default") });
    let (.., stats) = s.c.finish(&registry(), &s.h, &[]);
    assert_eq!(stats.holdout_type_id_attempted, 2);
    assert_eq!(stats.holdout_type_id_correct, 2);
}

#[test]
fn gun_kill_does_not_link_to_a_passing_weapon() {
    let mut s = scene();
    s.c.push_kill(kill(3200, 10, "30mm_UBR6"));
    let (ev, _, stats) = s.c.finish(&registry(), &s.h, &[]);
    assert_eq!(ev[0].outcome.class, "unknown");
    assert_eq!(ev[0].weapon_name, None);
    assert_eq!(stats.kills_linked_to_weapon, 0);
}

#[test]
fn gun_rounds_and_stores_are_told_apart() {
    for n in ["ap_i", "ap_i_t", "he_frag_i", "frag_i_t", "30mm_UBR6", "23mm_apit", "35_228mm_aphe_i", "apcr", "apds_fs"] {
        assert!(is_gun_round(n), "{n}");
    }
    for n in ["us_aim9l_sidewinder", "72mm_9m39", "163mm_roland_3", "us_agm_65b", "120mm_9m37m"] {
        assert!(!is_gun_round(n), "{n}");
    }
    assert_eq!(base_name("us_aim7p_sparrow_fa_18_default"), "us_aim7p_sparrow_fa_18");
}

#[test]
fn equipment_filter_keeps_stores_only() {
    let e = ["default", "countermeasure_split_launcher_jet", "cannonAkan_m75", "akan_m75_belt_pack", "swd_ltc_bol", "f_4c_g_suit", "swd_rb74", "swd_rb99", "swe_135mm_m70b"].map(String::from);
    assert_eq!(weapon_equipment(&e), ["swd_rb74", "swd_rb99", "swe_135mm_m70b"]);
}
