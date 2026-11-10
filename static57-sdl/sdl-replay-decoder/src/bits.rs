//! Bit reader for packed replay payloads.
//!
//! Convention (VERIFIED on `aircraft_small` payloads of Static 57 replay
//! `#2026.11.09 18.50.56`): bits are consumed most-significant-bit first
//! within each byte; multi-byte values are 8-bit groups assembled
//! little-endian. On byte-aligned data this equals ordinary little-endian
//! reads. The convention was established by matching an `f32` read at a
//! non-byte-aligned offset to the cockpit altimeter in 99.5% of packets; the
//! LSB-first and big-endian alternatives matched no fixed offset.

#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0 }
    }

    /// Current position in bits.
    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn len_bits(&self) -> usize {
        self.data.len() * 8
    }

    pub fn remaining(&self) -> usize {
        self.len_bits().saturating_sub(self.pos)
    }

    pub fn skip(&mut self, n: usize) -> Option<()> {
        (self.remaining() >= n).then(|| self.pos += n)
    }

    pub fn bit(&mut self) -> Option<bool> {
        if self.pos >= self.len_bits() {
            return None;
        }
        let b = self.data[self.pos / 8] >> (7 - self.pos % 8) & 1;
        self.pos += 1;
        Some(b == 1)
    }

    /// `n` (<= 64) bits as an unsigned value, first bit most significant.
    pub fn bits(&mut self, n: usize) -> Option<u64> {
        assert!(n <= 64);
        if self.remaining() < n {
            return None;
        }
        let mut v = 0u64;
        for _ in 0..n {
            v = v << 1 | self.bit()? as u64;
        }
        Some(v)
    }

    /// `bytes` 8-bit groups assembled little-endian.
    fn le(&mut self, bytes: usize) -> Option<u64> {
        if self.remaining() < bytes * 8 {
            return None;
        }
        let mut v = 0u64;
        for i in 0..bytes {
            v |= self.bits(8)? << (8 * i);
        }
        Some(v)
    }

    pub fn u8(&mut self) -> Option<u8> {
        self.bits(8).map(|v| v as u8)
    }

    pub fn u16(&mut self) -> Option<u16> {
        self.le(2).map(|v| v as u16)
    }

    pub fn u32(&mut self) -> Option<u32> {
        self.le(4).map(|v| v as u32)
    }

    pub fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }
}

/// Bit writer with the same convention, for building test fixtures.
#[derive(Debug, Default, Clone)]
pub struct BitWriter {
    data: Vec<u8>,
    len: usize,
}

impl BitWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bit(&mut self, b: bool) -> &mut Self {
        if self.len % 8 == 0 {
            self.data.push(0);
        }
        if b {
            *self.data.last_mut().unwrap() |= 0x80 >> (self.len % 8);
        }
        self.len += 1;
        self
    }

    pub fn bits(&mut self, v: u64, n: usize) -> &mut Self {
        for i in (0..n).rev() {
            self.bit(v >> i & 1 == 1);
        }
        self
    }

    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.bits(v as u64, 8)
    }

    pub fn u32(&mut self, v: u32) -> &mut Self {
        for b in v.to_le_bytes() {
            self.u8(b);
        }
        self
    }

    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.u32(v.to_bits())
    }

    pub fn finish(self) -> Vec<u8> {
        self.data
    }
}
