//! Packet type 2 (`AircraftSmall`) flight-model sync decoder.
//!
//! The record layout follows two independent third-party descriptions
//! (REPORTED). SDL treats a payload as structurally decoded only when every
//! aircraft record parses and the list terminator (uid `0x7FF`) is reached
//! inside the payload. Field *meanings* are assigned separately in
//! [`crate::flight`], each with its own evidence class.
//!
//! ```text
//! sync    := entry* terminator trailer
//! entry   := uid_explicit:1 [uid:varint & 0x7FF]   (else previous uid + 1)
//!            no_data:1
//!            [if !no_data: hidden_a:1 hidden_b:1 [if !(a && b): state]]
//! state   := ground_flag:1 unk_u32:32 unk_bit:1 guns weapons
//!            pos:f32[3] euler:u32 packed_vel:u32 <align>
//!            controls:u8[7] engines sensors countermeasures targets
//!            [flag:1 u16 u16] [flag:1 seeker_len:u32 seeker:bits]
//! trailer := f32[3] remaining_bits
//! ```

use crate::bitstream::{BitError, BitReader, BitResult};
use serde::Serialize;

pub const UID_TERMINATOR: u16 = 0x7FF;
pub const MAX_ENGINES: u8 = 15;
pub const MAX_SENSORS: u8 = 8;
pub const MAX_TARGETS: u8 = 8;

#[derive(Debug, Clone, Serialize)]
pub struct GunBlock {
    pub triggers: [bool; 3],
    /// One bit per gun-type weapon entry; meaning unknown.
    pub entry_bits: Vec<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeaponSlot {
    pub a: i32,
    pub values: Vec<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Engine {
    pub state: u8,
    /// Raw int16 (reported scale 1.1: throttle with afterburner).
    pub boost_raw: Option<i16>,
    /// Raw int8 (reported scale 1.0: engine health).
    pub health_raw: Option<i8>,
    pub radiators: [u8; 2],
}

#[derive(Debug, Clone, Serialize)]
pub struct AircraftState {
    pub ground_flag: bool,
    pub unk_u32: u32,
    pub unk_bit: bool,
    pub guns: Option<GunBlock>,
    pub weapons: Vec<WeaponSlot>,
    /// Bit offset of the position triple within the payload.
    pub position_bit: u64,
    pub position: [f32; 3],
    pub euler_raw: u32,
    pub packed_velocity_raw: u32,
    pub controls: [u8; 7],
    pub engines: Vec<Engine>,
    pub sensor_count: u8,
    pub countermeasure_count: u8,
    pub target_count: u8,
    pub tail_pair: Option<(u16, u16)>,
    /// Bit length of the optional seeker block (kept opaque).
    pub seeker_bits: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryBody {
    /// `no_data` bit set: the slot carries no record.
    NoData,
    /// Both leading flags set: record withheld (reported for non-author
    /// aircraft in client replays).
    Withheld,
    State(Box<AircraftState>),
}

#[derive(Debug, Clone, Serialize)]
pub struct AircraftEntry {
    pub uid: u16,
    pub uid_explicit: bool,
    pub bit_offset: u64,
    pub bit_length: u64,
    pub hidden_flags: Option<(bool, bool)>,
    pub body: EntryBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncDecode {
    pub entries: Vec<AircraftEntry>,
    pub terminated: bool,
    /// Three floats after the terminator (reported: author's position in
    /// client replays).
    pub trailer: Option<[f32; 3]>,
    pub consumed_bits: u64,
    pub payload_bits: u64,
    pub error: Option<BitError>,
}

fn read_guns(r: &mut BitReader) -> BitResult<Option<GunBlock>> {
    if !r.bit("guns.present")? {
        return Ok(None);
    }
    let triggers = [r.bit("guns.t0")?, r.bit("guns.t1")?, r.bit("guns.t2")?];
    let n = r.bits(4, "guns.count")?;
    let mut entry_bits = Vec::with_capacity(n as usize);
    for _ in 0..n {
        entry_bits.push(r.bit("guns.entry")?);
    }
    Ok(Some(GunBlock { triggers, entry_bits }))
}

fn invalid(r: &BitReader, what: &'static str) -> BitError {
    BitError { bit: r.position(), wanted_bits: 0, available_bits: r.remaining(), what }
}

fn read_weapons(r: &mut BitReader) -> BitResult<Vec<WeaponSlot>> {
    let n = r.zigzag_i32("weapons.count")?;
    if !(0..=256).contains(&n) {
        return Err(invalid(r, "weapons.count out of range"));
    }
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let a = r.zigzag_i32("weapons.a")?;
        let m = r.zigzag_i32("weapons.n")?;
        if m > 4096 {
            return Err(invalid(r, "weapons.n out of range"));
        }
        let mut values = Vec::new();
        for _ in 0..m.max(0) {
            values.push(r.zigzag_i32("weapons.value")?);
        }
        out.push(WeaponSlot { a, values });
    }
    Ok(out)
}

fn read_engines(r: &mut BitReader) -> BitResult<Vec<Engine>> {
    let n = r.byte("engines.count")?;
    if n > MAX_ENGINES {
        return Err(invalid(r, "engines.count > 15"));
    }
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let has_boost = r.bit("engine.boost_flag")?;
        let state = r.byte("engine.state")?;
        let boost_raw = if has_boost { Some(r.i16_le("engine.boost")?) } else { None };
        let health_raw = if r.bit("engine.health_flag")? { Some(r.byte("engine.health")? as i8) } else { None };
        let radiators = [r.byte("engine.radiator0")?, r.byte("engine.radiator1")?];
        out.push(Engine { state, boost_raw, health_raw, radiators });
    }
    Ok(out)
}

fn read_sensor(r: &mut BitReader) -> BitResult<()> {
    let on = r.bit("sensor.on")?;
    let kind_slot = r.byte("sensor.kind_slot")?;
    match kind_slot >> 4 {
        1 => {
            if !r.bit("sensor.k1.flag")? {
                return Ok(());
            }
            let packed = r.i16_le("sensor.k1.packed")?;
            r.skip(32 + 3 * 16, "sensor.k1.values")?;
            if packed < 0 {
                r.skip(8, "sensor.k1.extra")?;
            }
            if r.bit("sensor.k1.flag2")? {
                r.skip(8, "sensor.k1.extra2")?;
            }
        }
        2 => {
            if !on {
                return Ok(());
            }
            r.skip(1 + 16 + 96 + 96 + 32, "sensor.k2.values")?;
        }
        4 => {
            if r.bit("sensor.k4.flag")? {
                r.skip(96, "sensor.k4.values")?;
            }
        }
        _ => return Err(invalid(r, "sensor kind not in {1,2,4}")),
    }
    if r.bit("sensor.contacts.flag")? {
        let n = r.bits(6, "sensor.contacts.count")?;
        r.skip(32 * n, "sensor.contacts")?;
        r.skip(6, "sensor.contacts.tail")?;
    }
    Ok(())
}

fn read_target(r: &mut BitReader) -> BitResult<()> {
    r.skip(16, "target.v1v2")?;
    if r.bit("target.v3")? {
        r.skip(96, "target.v4")?;
    }
    if r.bit("target.compressed")? {
        r.skip(96 + 48, "target.compressed_pos")?;
    } else {
        r.skip(96 + 96, "target.pos")?;
    }
    r.skip(32, "target.v5")?;
    r.skip(2, "target.v8v10")?;
    if r.bit("target.v11_flag")? {
        r.skip(8, "target.v11")?;
    }
    r.skip(1, "target.v12")?;
    let has_v14 = r.bit("target.v14_flag")?;
    r.skip(1, "target.v13")?;
    if has_v14 {
        r.skip(8, "target.v14")?;
    }
    if r.bit("target.v15_flag")? {
        r.skip(32, "target.v15")?;
    }
    Ok(())
}

fn read_state(r: &mut BitReader) -> BitResult<AircraftState> {
    let ground_flag = r.bit("ground_flag")?;
    let unk_u32 = r.u32_le("unk_u32")?;
    let unk_bit = r.bit("unk_bit")?;
    let guns = read_guns(r)?;
    let weapons = read_weapons(r)?;
    let position_bit = r.position();
    let position = [r.f32_le("pos.x")?, r.f32_le("pos.y")?, r.f32_le("pos.z")?];
    let euler_raw = r.u32_le("euler")?;
    let packed_velocity_raw = r.u32_le("packed_velocity")?;
    r.align();
    let controls = r.bytes::<7>("controls")?;
    let engines = read_engines(r)?;
    let sensor_count = r.byte("sensors.count")?;
    if sensor_count > MAX_SENSORS {
        return Err(invalid(r, "sensors.count > 8"));
    }
    for _ in 0..sensor_count {
        read_sensor(r)?;
    }
    if sensor_count > 0 {
        r.skip(8, "sensors.tail")?;
    }
    let countermeasure_count = r.byte("cm.count")?;
    if countermeasure_count > 8 {
        return Err(invalid(r, "cm.count > 8"));
    }
    r.skip(16 * countermeasure_count as u64, "cm.records")?;
    if countermeasure_count > 0 {
        r.skip(8, "cm.tail")?;
    }
    let target_count = r.bits(4, "targets.count")? as u8;
    if target_count > MAX_TARGETS {
        return Err(invalid(r, "targets.count > 8"));
    }
    for _ in 0..target_count {
        read_target(r)?;
    }
    let tail_pair = if r.bit("tail_pair.flag")? { Some((r.u16_le("tail_pair.a")?, r.u16_le("tail_pair.b")?)) } else { None };
    let seeker_bits = if r.bit("seeker.flag")? {
        let n = r.u32_le("seeker.len")?;
        r.skip(n as u64, "seeker.block")?;
        Some(n)
    } else {
        None
    };
    Ok(AircraftState {
        ground_flag,
        unk_u32,
        unk_bit,
        guns,
        weapons,
        position_bit,
        position,
        euler_raw,
        packed_velocity_raw,
        controls,
        engines,
        sensor_count,
        countermeasure_count,
        target_count,
        tail_pair,
        seeker_bits,
    })
}

/// Decode one type-2 payload (bytes after the packet type and timestamp).
pub fn decode_sync(payload: &[u8]) -> SyncDecode {
    let mut r = BitReader::new(payload);
    let mut out = SyncDecode {
        entries: Vec::new(),
        terminated: false,
        trailer: None,
        consumed_bits: 0,
        payload_bits: r.len_bits(),
        error: None,
    };
    let mut uid: u16 = 0;
    let result: BitResult<()> = (|| {
        loop {
            let start = r.position();
            let explicit = r.bit("uid.explicit")?;
            uid = if explicit { (r.compressed("uid")? & 0x7FF) as u16 } else { uid.wrapping_add(1) & 0x7FF };
            if uid == UID_TERMINATOR {
                out.terminated = true;
                break;
            }
            let mut entry =
                AircraftEntry { uid, uid_explicit: explicit, bit_offset: start, bit_length: 0, hidden_flags: None, body: EntryBody::NoData };
            if !r.bit("no_data")? {
                let a = r.bit("hidden_a")?;
                let b = r.bit("hidden_b")?;
                entry.hidden_flags = Some((a, b));
                entry.body = if a && b { EntryBody::Withheld } else { EntryBody::State(Box::new(read_state(&mut r)?)) };
            }
            entry.bit_length = r.position() - start;
            out.entries.push(entry);
            if out.entries.len() > 2048 {
                return Err(invalid(&r, "more than 2048 aircraft entries"));
            }
        }
        if r.remaining() >= 96 {
            out.trailer = Some([r.f32_le("trailer.x")?, r.f32_le("trailer.y")?, r.f32_le("trailer.z")?]);
        }
        Ok(())
    })();
    out.error = result.err();
    out.consumed_bits = r.position();
    out
}
