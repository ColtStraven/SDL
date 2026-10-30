//! Builders for synthetic ECS, weapon and battle-log payloads.
#![allow(dead_code)]

use super::bitwriter::BitWriter;

/// Compact entity id, 4-byte form.
fn eid4(w: &mut BitWriter, eid: u32) {
    let (index, generation) = (eid & 0x3F_FFFF, eid >> 22);
    let v = (index << 2) | (generation << 24);
    w.u16(v as u16).u16((v >> 16) as u16);
}

/// Uncompressed ECS creation packet with one entity. `template_id` is sent
/// with its name; pass `new_template = false` for later uses.
pub fn ecs_create(eid: u32, template_id: u8, name: &str, new_template: bool, data: &[u8]) -> Vec<u8> {
    let mut block = BitWriter::default();
    block.varint(template_id as u64);
    if new_template {
        block.byte(name.len() as u8).raw(name.as_bytes()).u16(0);
    }
    block.raw(data);
    let mut w = BitWriter::default();
    w.byte(0x24).byte(0);
    eid4(&mut w, eid);
    w.varint(block.bytes.len() as u64).raw(&block.bytes);
    w.bytes
}

/// Aircraft unit creation data.
pub fn unit_data(uid: u16, spawn: [f32; 3], strings: &[&str]) -> Vec<u8> {
    let mut d = vec![0u8; 28];
    d[1] = 0x0D;
    for k in 0..3 {
        d[11 + 4 * k..15 + 4 * k].copy_from_slice(&spawn[k].to_le_bytes());
    }
    d.extend_from_slice(&[0x30, 0x70]);
    d.extend_from_slice(&uid.to_le_bytes());
    d.extend_from_slice(&[0u8; 12]);
    for s in strings {
        d.push(s.len() as u8);
        d.extend_from_slice(s.as_bytes());
    }
    d
}

/// Store creation data with the marker layout seen on real replays.
pub fn store_data(launch: [f32; 3]) -> Vec<u8> {
    store_data_typed(0x0A, launch)
}

/// Store creation data with a chosen type id (one varint byte below 128).
pub fn store_data_typed(type_id: u8, launch: [f32; 3]) -> Vec<u8> {
    store_data_owned(type_id, 1, launch)
}

/// Store creation data naming launcher entity `owner`.
pub fn store_data_owned(type_id: u8, owner: u32, launch: [f32; 3]) -> Vec<u8> {
    let (index, generation) = (owner & 0x3F_FFFF, owner >> 22);
    let mut d = vec![0x00, 0x02, 0x00, type_id];
    if generation <= 1 {
        d.extend_from_slice(&((1 | (generation << 1) | (index << 2)) as u16).to_le_bytes());
    } else {
        d.extend_from_slice(&((2 | (index << 2)) as u16).to_le_bytes());
        d.push(generation as u8);
    }
    d.extend_from_slice(&[0x01, 0x00, 0x00, 1, 2, 3, 4, 0, 0, 0x13, 0x00]);
    for v in launch {
        d.extend_from_slice(&v.to_le_bytes());
    }
    d.extend_from_slice(&[0u8; 20]);
    d
}

/// ShellsData id for an ECS raw id.
pub fn shell_id(eid: u32) -> u32 {
    ((eid & 0x3F_FFFF) << 10) | ((eid >> 22) + 2)
}

/// One `ShellsData` message holding one record in list 0.
pub fn shells_payload(eid: u32, pos: [f32; 3], vel: [f64; 3], tsl_s: f64) -> Vec<u8> {
    shells_message(eid, pos, vel, tsl_s, None)
}

/// As `shells_payload`, with a seeker block of `bits` bits.
pub fn shells_payload_seeker(eid: u32, pos: [f32; 3], vel: [f64; 3], tsl_s: f64, bits: u16, block: &[u8]) -> Vec<u8> {
    shells_message(eid, pos, vel, tsl_s, Some((bits, block)))
}

fn shells_message(eid: u32, pos: [f32; 3], vel: [f64; 3], tsl_s: f64, seeker: Option<(u16, &[u8])>) -> Vec<u8> {
    let mut body = BitWriter::default();
    body.byte(1);
    body.bit(seeker.is_none()).bit(false); // no seeker flag, full record
    body.u32(shell_id(eid));
    body.bit(true).f32(pos[0]).f32(pos[1]).f32(pos[2]);
    body.zigzag(5).f32(0.0).f32(0.0).f32(0.0);
    for v in vel {
        body.u16((v * 32767.0 / 2000.0).round() as i16 as u16);
    }
    body.u16(0).u16(0).u16(0);
    body.bit(false);
    body.byte(0x0F);
    body.varint((tsl_s * 48.0).round() as u64 + 1).varint(1).varint(1).varint(1);
    match seeker {
        Some((bits, block)) => {
            body.f32(0.0).bit(false).u16(bits);
            for i in 0..bits as usize {
                body.bit(block[i / 8] & (0x80 >> (i % 8)) != 0);
            }
        }
        None => {
            body.bit(false); // no SACLOS
        }
    }
    body.byte(0).byte(0); // empty lists
    let bits = body.bits;
    let mut w = BitWriter::default();
    w.u16(0x5802).u16(0xF0DB).varint(bits).raw(&body.bytes);
    w.bytes
}

/// Id-field message: fields in ascending number, each `(number, bits, bytes)`.
fn id_message(mid: u16, fields: &[(u8, u32, Vec<u8>)]) -> Vec<u8> {
    let mut mask = 0u64;
    let mut body = BitWriter::default();
    for (n, _, b) in fields {
        mask |= 1 << n;
        body.raw(b);
    }
    let mut head = BitWriter::default();
    let mut mask_w = BitWriter::default();
    mask_w.varint(mask);
    head.u16((2 + mask_w.bytes.len() + body.bytes.len()) as u16);
    head.raw(&mask_w.bytes).raw(&body.bytes);
    let mut sizes = BitWriter::default();
    for (_, bits, _) in fields {
        match bits {
            8 => sizes.bits(3, 2),
            16 => sizes.bits(3, 3),
            32 => sizes.bits(3, 4),
            n => sizes.bits(3, 0).varint(*n as u64),
        };
    }
    let mut w = BitWriter::default();
    w.u16(0x5802).u16(mid).raw(&head.bytes).raw(&sizes.bytes);
    w.bytes
}

fn lenstr(s: &str) -> (u32, Vec<u8>) {
    let mut v = vec![s.len() as u8];
    v.extend_from_slice(s.as_bytes());
    ((v.len() * 8) as u32, v)
}

pub fn kill_payload(victim: u16, killer: u16, killer_vehicle: &str, weapon: &str) -> Vec<u8> {
    let (vb, vs) = lenstr(killer_vehicle);
    let (wb, ws) = lenstr(weapon);
    id_message(
        0xF058,
        &[(1, 32, 7u32.to_le_bytes().to_vec()), (2, vb, vs), (3, 16, victim.to_le_bytes().to_vec()), (4, 16, killer.to_le_bytes().to_vec()), (10, wb, ws)],
    )
}

pub fn damage_payload(uid1: u16, vehicle: &str, attacker: u16) -> Vec<u8> {
    let (vb, vs) = lenstr(vehicle);
    id_message(0xF056, &[(1, 16, uid1.to_le_bytes().to_vec()), (2, 32, 3u32.to_le_bytes().to_vec()), (3, vb, vs), (4, 16, attacker.to_le_bytes().to_vec())])
}
