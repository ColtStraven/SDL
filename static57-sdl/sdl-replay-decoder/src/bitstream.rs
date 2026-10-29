//! Bit-level reader for Dagor `danet::BitStream` payloads.
//!
//! Conventions (REPORTED by two independent third-party readers, VERIFIED by
//! SDL through exact record-boundary agreement on genuine type-2 payloads; see
//! `docs/FORMAT_FINDINGS.md`):
//!
//! - Bits are consumed most-significant-bit first within each byte.
//! - Byte-sized reads need not be aligned: a byte read at bit offset `k`
//!   takes the low `8-k` bits of byte `i` and the high `k` bits of byte `i+1`.
//! - Multi-byte integers and floats are little-endian sequences of such bytes.
//! - A sub-byte read of `n` bits returns those `n` bits as an unsigned number
//!   (first bit read is the most significant).
//! - "Compressed" integers are little-endian base-128 varints read byte-wise.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BitError {
    /// Bit offset where the failing read started.
    pub bit: u64,
    pub wanted_bits: u64,
    pub available_bits: u64,
    pub what: &'static str,
}

impl std::fmt::Display for BitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: needed {} bits at bit {}, only {} available",
            self.what, self.wanted_bits, self.bit, self.available_bits
        )
    }
}

impl std::error::Error for BitError {}

pub type BitResult<T> = Result<T, BitError>;

#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    pos: u64,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0 }
    }

    pub fn position(&self) -> u64 {
        self.pos
    }

    pub fn len_bits(&self) -> u64 {
        self.data.len() as u64 * 8
    }

    pub fn remaining(&self) -> u64 {
        self.len_bits().saturating_sub(self.pos)
    }

    fn need(&self, bits: u64, what: &'static str) -> BitResult<()> {
        if self.remaining() < bits {
            return Err(BitError { bit: self.pos, wanted_bits: bits, available_bits: self.remaining(), what });
        }
        Ok(())
    }

    pub fn bit(&mut self, what: &'static str) -> BitResult<bool> {
        self.need(1, what)?;
        let b = self.data[(self.pos >> 3) as usize] & (0x80 >> (self.pos & 7)) != 0;
        self.pos += 1;
        Ok(b)
    }

    /// Up to 64 bits, first bit most significant.
    pub fn bits(&mut self, n: u32, what: &'static str) -> BitResult<u64> {
        assert!(n <= 64);
        self.need(n as u64, what)?;
        let mut v = 0u64;
        for _ in 0..n {
            let b = self.data[(self.pos >> 3) as usize] & (0x80 >> (self.pos & 7)) != 0;
            v = (v << 1) | b as u64;
            self.pos += 1;
        }
        Ok(v)
    }

    pub fn byte(&mut self, what: &'static str) -> BitResult<u8> {
        Ok(self.bits(8, what)? as u8)
    }

    pub fn bytes<const N: usize>(&mut self, what: &'static str) -> BitResult<[u8; N]> {
        self.need(N as u64 * 8, what)?;
        let mut out = [0u8; N];
        for b in &mut out {
            *b = self.bits(8, what)? as u8;
        }
        Ok(out)
    }

    pub fn u16_le(&mut self, what: &'static str) -> BitResult<u16> {
        Ok(u16::from_le_bytes(self.bytes::<2>(what)?))
    }

    pub fn i16_le(&mut self, what: &'static str) -> BitResult<i16> {
        Ok(i16::from_le_bytes(self.bytes::<2>(what)?))
    }

    pub fn u32_le(&mut self, what: &'static str) -> BitResult<u32> {
        Ok(u32::from_le_bytes(self.bytes::<4>(what)?))
    }

    pub fn f32_le(&mut self, what: &'static str) -> BitResult<f32> {
        Ok(f32::from_bits(self.u32_le(what)?))
    }

    /// Little-endian base-128 varint, at most 10 bytes.
    pub fn compressed(&mut self, what: &'static str) -> BitResult<u64> {
        let start = self.pos;
        let mut v = 0u64;
        for i in 0..10 {
            let b = self.byte(what)?;
            v |= ((b & 0x7f) as u64) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(BitError { bit: start, wanted_bits: 80, available_bits: self.remaining(), what: "varint longer than 10 bytes" })
    }

    pub fn zigzag_i32(&mut self, what: &'static str) -> BitResult<i32> {
        let u = self.compressed(what)? as u32;
        Ok(((u >> 1) as i32) ^ -((u & 1) as i32))
    }

    /// Advance to the next byte boundary (no-op when aligned).
    pub fn align(&mut self) {
        self.pos = (self.pos + 7) & !7;
    }

    pub fn skip(&mut self, n: u64, what: &'static str) -> BitResult<()> {
        self.need(n, what)?;
        self.pos += n;
        Ok(())
    }

    /// Copy bits `[from, to)` into bytes (MSB-first, final byte left-aligned).
    pub fn slice_bits(data: &[u8], from: u64, to: u64) -> Vec<u8> {
        let mut r = BitReader { data, pos: from };
        let mut out = Vec::with_capacity(((to - from + 7) / 8) as usize);
        while r.pos < to {
            let n = (to - r.pos).min(8) as u32;
            let v = r.bits(n, "slice").unwrap_or(0) as u8;
            out.push(v << (8 - n));
        }
        out
    }
}

/// Dagor `netutils::UNPACKS`-style signed quantisation: `raw / max * scale`.
pub fn unpack_signed(raw: i64, bits: u32, scale: f64) -> f64 {
    let max = ((1i64 << (bits - 1)) - 1) as f64;
    raw as f64 / max * scale
}

/// Sign-and-magnitude field: `mag` over `mag_bits` maps to `[0, scale]`.
pub fn unpack_sign_magnitude(mag: u64, mag_bits: u32, negative: bool, scale: f64) -> f64 {
    let v = mag as f64 * scale / ((1u64 << mag_bits) - 1) as f64;
    if negative { -v } else { v }
}
