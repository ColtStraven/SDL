//! MPI 0xF0DB (`ShellsData`) in-flight store decoder: rockets, missiles,
//! bombs and torpedoes.
//!
//! ```text
//! body    := bit_length:varint counted_list[3]
//! list    := count:u8 record{count}
//! record  := no_seeker:1 short:1 eid:u32le position [if !short: dynamics]
//! position:= plain:1 (f32[3] | u32 u32 packed 22/20/22 bits over +-15 km)
//! dynamics:= i:zigzag dir:f32[3] vel:i16[3] ang:i16[3] [flag:1 u16]
//!            flags:u8 timer{popcount(flags & 0xF)}:varint
//!            (if !no_seeker: float bit seeker_bits:u16 seeker | if no_seeker: flag:1)
//! ```
//!
//! The layout is REPORTED by third-party sources. SDL accepts a message only
//! when the records consume exactly the declared bit length; what the lists
//! and fields mean physically is established separately in `weapons.rs`.

use crate::bitstream::{BitError, BitReader, BitResult};
use crate::cockpit::mpi_header;
use serde::Serialize;

pub const MSG_SHELLS_DATA: u16 = 0xF0DB;

/// Velocity scale: i16 full scale is +-2000 m/s.
pub const VELOCITY_SCALE: f64 = 2000.0 / 32767.0;
/// Position range of the packed form, metres.
pub const PACKED_RANGE_M: f64 = 15_000.0;
/// Timer tick: 48 per second.
pub const TIMER_HZ: f64 = 48.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dynamics {
    pub index: i32,
    /// Raw unit-vector candidate; meaning validated in `weapons.rs`.
    pub direction: [f32; 3],
    pub velocity_raw: [i16; 3],
    pub rates_raw: [i16; 3],
    pub flag_value: Option<u16>,
    pub flags: u8,
    /// `(value - 1) / 48` seconds for each set low flag bit, in bit order.
    pub timers_s: Vec<f64>,
    pub seeker_float: Option<f32>,
    pub seeker_bit: Option<bool>,
    pub seeker_bits: Option<u16>,
}

impl Dynamics {
    pub fn velocity_mps(&self) -> [f64; 3] {
        self.velocity_raw.map(|v| v as f64 * VELOCITY_SCALE)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShellRecord {
    /// List index 0, 1 or 2 within the message.
    pub list: u8,
    pub no_seeker: bool,
    pub short: bool,
    /// Raw entity id: index in the low 22 bits, generation above.
    pub eid: u32,
    pub position_m: [f32; 3],
    pub position_packed: bool,
    pub dynamics: Option<Dynamics>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShellsMessage {
    pub declared_bits: u64,
    pub consumed_bits: u64,
    /// True when the parse ended exactly at the declared length, up to the
    /// padding of the final byte.
    pub exact: bool,
    pub records: Vec<ShellRecord>,
}

fn unpack(v: u32, bits: u32) -> f32 {
    let unit = v as f64 * 2.0 / ((1u64 << bits) - 1) as f64 - 1.0;
    (unit.clamp(-1.0, 1.0) * PACKED_RANGE_M) as f32
}

/// `plain` is false for the packed form.
fn read_position(r: &mut BitReader) -> BitResult<([f32; 3], bool)> {
    if r.bit("position.plain")? {
        Ok(([r.f32_le("pos.x")?, r.f32_le("pos.y")?, r.f32_le("pos.z")?], false))
    } else {
        let a = r.u32_le("pos.packed0")?;
        let b = r.u32_le("pos.packed1")?;
        let x = a >> 10;
        let y = ((a & 0x3FF) << 10) | (b >> 22);
        let z = b & 0x3F_FFFF;
        Ok(([unpack(x, 22), unpack(y, 20), unpack(z, 22)], true))
    }
}

fn read_dynamics(r: &mut BitReader, no_seeker: bool) -> BitResult<Dynamics> {
    let index = r.zigzag_i32("dyn.index")?;
    let direction = [r.f32_le("dyn.dir.x")?, r.f32_le("dyn.dir.y")?, r.f32_le("dyn.dir.z")?];
    let velocity_raw = [r.i16_le("dyn.vel.x")?, r.i16_le("dyn.vel.y")?, r.i16_le("dyn.vel.z")?];
    let rates_raw = [r.i16_le("dyn.rate.x")?, r.i16_le("dyn.rate.y")?, r.i16_le("dyn.rate.z")?];
    let flag_value = if r.bit("dyn.flag")? { Some(r.u16_le("dyn.flag_value")?) } else { None };
    let flags = r.byte("dyn.flags")?;
    let mut timers_s = Vec::new();
    for bit in 0..4 {
        if flags & (1 << bit) != 0 {
            let v = r.compressed("dyn.timer")? as f64;
            timers_s.push((v - 1.0) / TIMER_HZ);
        }
    }
    let (mut seeker_float, mut seeker_bit, mut seeker_bits) = (None, None, None);
    if !no_seeker {
        seeker_float = Some(r.f32_le("seeker.float")?);
        seeker_bit = Some(r.bit("seeker.bit")?);
        let n = r.u16_le("seeker.bits")?;
        r.skip(n as u64, "seeker.block")?;
        seeker_bits = Some(n);
    } else if r.bit("dyn.saclos")? {
        return Err(BitError { bit: r.position(), wanted_bits: 0, available_bits: r.remaining(), what: "SACLOS block not supported" });
    }
    Ok(Dynamics { index, direction, velocity_raw, rates_raw, flag_value, flags, timers_s, seeker_float, seeker_bit, seeker_bits })
}

fn read_body(r: &mut BitReader, out: &mut Vec<ShellRecord>) -> BitResult<()> {
    for list in 0..3u8 {
        let count = r.byte("list.count")?;
        for _ in 0..count {
            let no_seeker = r.bit("rec.no_seeker")?;
            let short = r.bit("rec.short")?;
            let eid = r.u32_le("eid")?;
            let (position_m, position_packed) = read_position(r)?;
            let dynamics = if short { None } else { Some(read_dynamics(r, no_seeker)?) };
            out.push(ShellRecord { list, no_seeker, short, eid, position_m, position_packed, dynamics });
        }
    }
    Ok(())
}

/// Decode a type-4 payload if it is a `ShellsData` message. `Ok(None)` for
/// other messages. Errors carry the failing bit offset within the body.
pub fn decode(payload: &[u8]) -> BitResult<Option<ShellsMessage>> {
    match mpi_header(payload) {
        Some((_, MSG_SHELLS_DATA)) => {}
        _ => return Ok(None),
    }
    let mut r = BitReader::new(&payload[4..]);
    let declared_bits = r.compressed("shells.len")?;
    let start = r.position();
    if declared_bits > r.remaining() {
        return Err(BitError { bit: start, wanted_bits: declared_bits, available_bits: r.remaining(), what: "declared length exceeds payload" });
    }
    let mut records = Vec::new();
    read_body(&mut r, &mut records)?;
    let consumed_bits = r.position() - start;
    let exact = consumed_bits <= declared_bits && declared_bits - consumed_bits < 8;
    Ok(Some(ShellsMessage { declared_bits, consumed_bits, exact, records }))
}
