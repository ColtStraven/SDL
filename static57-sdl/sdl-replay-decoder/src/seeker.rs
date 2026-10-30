//! In-flight seeker blocks carried by MPI 0xF0DB records.
//!
//! Layouts are REPORTED by third-party documentation and accepted here only
//! to the extent the corpus confirms them (`docs/FINDINGS_M6_ENGAGEMENTS.md`).

use crate::bitstream::{BitReader, BitResult};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeekerObservation {
    pub bits: u16,
    /// Bits 8..=10 of the block.
    pub mode: u8,
    /// Reported estimated target position, world metres.
    pub target_estimate_m: Option<[f32; 3]>,
    /// Reported line of sight, unit vector.
    pub line_of_sight: Option<[f32; 3]>,
    /// Reported range, metres.
    pub range_m: Option<u16>,
}

fn f32_at(r: &BitReader, bit: u64) -> BitResult<f32> {
    let mut c = r.clone();
    c.seek(bit, "seeker.seek")?;
    c.f32_le("seeker.float")
}

fn i16_at(r: &BitReader, bit: u64) -> BitResult<i16> {
    let mut c = r.clone();
    c.seek(bit, "seeker.seek")?;
    c.i16_le("seeker.int")
}

fn u16_at(r: &BitReader, bit: u64) -> BitResult<u16> {
    let mut c = r.clone();
    c.seek(bit, "seeker.seek")?;
    c.u16_le("seeker.int")
}

fn los(r: &BitReader, bit: u64) -> Option<[f32; 3]> {
    let v = [i16_at(r, bit).ok()?, i16_at(r, bit + 16).ok()?, i16_at(r, bit + 32).ok()?].map(|x| x as f32 / 32767.0);
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    ((0.9..1.1).contains(&n)).then_some(v)
}

/// Interpret a seeker block given as left-aligned bytes.
pub fn parse(block: &[u8], bits: u16) -> SeekerObservation {
    let r = BitReader::new(block);
    let mode = {
        let mut c = r.clone();
        c.seek(8, "seeker.seek").and_then(|_| c.bits(3, "seeker.mode")).unwrap_or(0) as u8
    };
    // Radar blocks: 607 bits, or 639 with the layout moved by 32 bits.
    let shift = match bits {
        607 => Some(0),
        639 => Some(32),
        _ => None,
    };
    let target_estimate_m = shift.and_then(|s| {
        let p = [f32_at(&r, 11 + s).ok()?, f32_at(&r, 43 + s).ok()?, f32_at(&r, 75 + s).ok()?];
        (p.iter().all(|v| v.is_finite() && v.abs() < 1e6) && p != [0.0; 3]).then_some(p)
    });
    let line_of_sight = match bits {
        283 => los(&r, 58),
        _ => shift.and_then(|s| los(&r, 340 + s)),
    };
    let range_m = shift.and_then(|s| u16_at(&r, 454 + s).ok());
    SeekerObservation { bits, mode, target_estimate_m, line_of_sight, range_m }
}
