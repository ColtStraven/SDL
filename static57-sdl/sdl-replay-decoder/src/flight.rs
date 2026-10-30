//! Physical interpretation of type-2 aircraft state fields.
//!
//! Every interpretation here was validated by SDL on the 27 genuine 2.59
//! client replays; methods and results are in `docs/FORMAT_FINDINGS.md` §5.
//!
//! World frame: Dagor world axes, metres, `+Y` up. `Y` equals the cockpit
//! altimeter (MSL) for the recording author. Horizontal axes `X`/`Z` are
//! metres in the map's world frame; their relation to map north is not
//! established.

use serde::Serialize;

/// Packed orientation word (32 bits).
///
/// ```text
/// bit 31     heading sign      bits 19..28  |heading| 10 bits over [0, pi]
/// bit 30     pitch sign        bits 10..18  |pitch|    9 bits over [0, pi/2]
/// bit 29     bank sign         bits  0..9   |bank|    10 bits over [0, pi]
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Orientation {
    /// Yaw about +Y, right-handed, zero along +X: the nose points along
    /// `(cos h, 0, -sin h)`.
    pub heading_deg: f64,
    /// Nose-up positive.
    pub pitch_deg: f64,
    /// Same sign as cockpit roll indicator (MPI 0xD04A id 52).
    pub bank_deg: f64,
}

pub const HEADING_QUANTUM_DEG: f64 = 180.0 / 1023.0;
pub const PITCH_QUANTUM_DEG: f64 = 90.0 / 511.0;
pub const BANK_QUANTUM_DEG: f64 = 180.0 / 1023.0;

pub fn decode_orientation(raw: u32) -> Orientation {
    let sign = |bit: u32, v: f64| if raw & (1 << bit) != 0 { -v } else { v };
    Orientation {
        heading_deg: sign(31, ((raw >> 19) & 0x3FF) as f64 * HEADING_QUANTUM_DEG),
        pitch_deg: sign(30, ((raw >> 10) & 0x1FF) as f64 * PITCH_QUANTUM_DEG),
        bank_deg: sign(29, (raw & 0x3FF) as f64 * BANK_QUANTUM_DEG),
    }
}

/// Packed velocity word (32 bits): magnitude plus unit direction.
///
/// ```text
/// bit 31 sign(vx)   bit 30 sign(vy)   bit 29 sign(vz)
/// bits 19..28  speed, 10 bits, 500/1023 m/s per step (1023 = saturated)
/// bit 18       unknown flag (no effect on speed or direction)
/// bits  9..17  |vx|/|v|, 9 bits over [0, 1]
/// bits  0..8   |vy|/|v|, 9 bits over [0, 1]
/// |vz|/|v| = sqrt(1 - rx^2 - ry^2)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Velocity {
    pub vx_mps: f64,
    pub vy_mps: f64,
    pub vz_mps: f64,
    pub speed_mps: f64,
    /// Speed field at its maximum: actual speed is >= `speed_mps`.
    pub saturated: bool,
    /// `rx^2 + ry^2` exceeded 1 by quantisation and `vz` was clamped to 0.
    pub clamped: bool,
    pub flag_bit18: bool,
}

pub const SPEED_QUANTUM_MPS: f64 = 500.0 / 1023.0;
pub const SPEED_MAX_MPS: f64 = 500.0;

pub fn decode_velocity(raw: u32) -> Velocity {
    let mag = (raw >> 19) & 0x3FF;
    let speed = mag as f64 * SPEED_QUANTUM_MPS;
    let rx = ((raw >> 9) & 0x1FF) as f64 / 511.0;
    let ry = (raw & 0x1FF) as f64 / 511.0;
    let rest = 1.0 - rx * rx - ry * ry;
    let rz = rest.max(0.0).sqrt();
    let sign = |bit: u32| if raw & (1 << bit) != 0 { -1.0 } else { 1.0 };
    Velocity {
        vx_mps: speed * rx * sign(31),
        vy_mps: speed * ry * sign(30),
        vz_mps: speed * rz * sign(29),
        speed_mps: speed,
        saturated: mag == 0x3FF,
        clamped: rest < -1e-9,
        flag_bit18: raw & (1 << 18) != 0,
    }
}

/// Track angle of a horizontal vector in the heading convention above.
pub fn track_deg(vx: f64, vz: f64) -> f64 {
    (-vz).atan2(vx).to_degrees()
}

/// Smallest signed difference `a - b` in degrees, in `(-180, 180]`.
pub fn angle_diff_deg(a: f64, b: f64) -> f64 {
    let mut d = (a - b) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d <= -180.0 {
        d += 360.0;
    }
    d
}

/// ISA density ratio in the troposphere (DERIVED; used only to compare
/// decoded world speed with the cockpit's indicated airspeed).
pub fn isa_density_ratio(altitude_m: f64) -> f64 {
    (1.0 - 2.255_77e-5 * altitude_m.clamp(-500.0, 11_000.0)).powf(4.2559)
}
