//! Minimal MPI 0xD04A (`ReplayCockpitParams`) decoder, used only as an
//! independent reference for validating the flight-state decoder.
//!
//! ```text
//! mpi     := object_id:u16le message_id:u16le body
//! body    := bit_length:varint <align> count:u16le (id:u16le value:f32le){count}
//! ```
//!
//! Present only in client replays, about once per author flight-model sync.

use crate::bitstream::BitReader;

pub const MSG_COCKPIT_PARAMS: u16 = 0xD04A;

/// Parameter ids used by SDL validation (meanings REPORTED, then checked).
pub mod id {
    /// Indicated airspeed, m/s.
    pub const AIRSPEED: u16 = 0;
    /// Vertical speed, m/s.
    pub const VERTICAL_SPEED: u16 = 39;
    /// Altimeter; metres or feet depending on the aircraft's instruments.
    /// On some feet altimeters id 40 is the value modulo 10,000 ft and id 42
    /// carries the full altitude (observed by SDL).
    pub const ALTITUDE: u16 = 40;
    pub const ALTITUDE_CANDIDATES: [u16; 3] = [40, 42, 43];
    /// Feet altimeters use 3.28 ft per metre: decoded altitude fits
    /// `0.304877 * ft` with a 0.03 m intercept, not the exact 0.3048.
    pub const METRES_PER_GAME_FOOT: f64 = 1.0 / 3.28;
    /// Roll angle, degrees.
    pub const ROLL: u16 = 52;
    /// Pitch angle, degrees, opposite sign to the flight-model pitch.
    pub const PITCH: u16 = 53;
}

#[derive(Debug, Clone, PartialEq)]
pub struct CockpitParams {
    pub timestamp_ms: u32,
    pub object_id: u16,
    pub params: Vec<(u16, f32)>,
}

impl CockpitParams {
    pub fn get(&self, id: u16) -> Option<f64> {
        self.params.iter().find(|p| p.0 == id).map(|p| p.1 as f64)
    }
}

/// MPI header: `(object_id, message_id)`.
pub fn mpi_header(payload: &[u8]) -> Option<(u16, u16)> {
    (payload.len() >= 4).then(|| (u16::from_le_bytes([payload[0], payload[1]]), u16::from_le_bytes([payload[2], payload[3]])))
}

/// Decode a type-4 payload if it is a cockpit-parameter message. Returns
/// `None` for other messages and for bodies whose declared length or count
/// does not fit.
pub fn decode(timestamp_ms: u32, payload: &[u8]) -> Option<CockpitParams> {
    let (object_id, mid) = mpi_header(payload)?;
    if mid != MSG_COCKPIT_PARAMS {
        return None;
    }
    let mut r = BitReader::new(&payload[4..]);
    let bits = r.compressed("cockpit.len").ok()?;
    r.align();
    if bits > r.remaining() {
        return None;
    }
    let count = r.u16_le("cockpit.count").ok()?;
    if 16 + count as u64 * 48 > bits {
        return None;
    }
    let mut params = Vec::with_capacity(count as usize);
    for _ in 0..count {
        params.push((r.u16_le("cockpit.id").ok()?, r.f32_le("cockpit.value").ok()?));
    }
    Some(CockpitParams { timestamp_ms, object_id, params })
}
