//! MSB-first bit writer and type-2 / cockpit payload builders for tests.
#![allow(dead_code)]

#[derive(Default)]
pub struct BitWriter {
    pub bytes: Vec<u8>,
    pub bits: u64,
}

impl BitWriter {
    pub fn bit(&mut self, b: bool) -> &mut Self {
        if self.bits % 8 == 0 {
            self.bytes.push(0);
        }
        if b {
            let i = (self.bits / 8) as usize;
            self.bytes[i] |= 0x80 >> (self.bits % 8);
        }
        self.bits += 1;
        self
    }
    pub fn bits(&mut self, n: u32, v: u64) -> &mut Self {
        for i in (0..n).rev() {
            self.bit((v >> i) & 1 == 1);
        }
        self
    }
    pub fn byte(&mut self, v: u8) -> &mut Self {
        self.bits(8, v as u64)
    }
    pub fn raw(&mut self, b: &[u8]) -> &mut Self {
        for &x in b {
            self.byte(x);
        }
        self
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.raw(&v.to_le_bytes())
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.raw(&v.to_le_bytes())
    }
    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.raw(&v.to_le_bytes())
    }
    pub fn varint(&mut self, mut v: u64) -> &mut Self {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                return self.byte(b);
            }
            self.byte(b | 0x80);
        }
    }
    pub fn zigzag(&mut self, v: i32) -> &mut Self {
        self.varint(((v << 1) ^ (v >> 31)) as u32 as u64)
    }
    pub fn align(&mut self) -> &mut Self {
        while self.bits % 8 != 0 {
            self.bit(false);
        }
        self
    }
}

/// Inverse of `flight::decode_orientation` (nearest quantum).
pub fn pack_euler(heading_deg: f64, pitch_deg: f64, bank_deg: f64) -> u32 {
    let mag = |v: f64, range: f64, bits: u32| ((v.abs() / range * ((1u32 << bits) - 1) as f64).round() as u32) & ((1 << bits) - 1);
    let mut w = (mag(heading_deg, 180.0, 10) << 19) | (mag(pitch_deg, 90.0, 9) << 10) | mag(bank_deg, 180.0, 10);
    if heading_deg < 0.0 {
        w |= 1 << 31;
    }
    if pitch_deg < 0.0 {
        w |= 1 << 30;
    }
    if bank_deg < 0.0 {
        w |= 1 << 29;
    }
    w
}

/// Inverse of `flight::decode_velocity` (nearest quantum, bit 18 clear).
pub fn pack_velocity(vx: f64, vy: f64, vz: f64) -> u32 {
    let s = (vx * vx + vy * vy + vz * vz).sqrt();
    let speed = ((s / (500.0 / 1023.0)).round() as u32).min(1023);
    let (rx, ry) = if s > 0.0 { (vx.abs() / s, vy.abs() / s) } else { (0.0, 0.0) };
    let mut w = (speed << 19) | (((rx * 511.0).round() as u32) << 9) | (ry * 511.0).round() as u32;
    if vx < 0.0 {
        w |= 1 << 31;
    }
    if vy < 0.0 {
        w |= 1 << 30;
    }
    if vz < 0.0 {
        w |= 1 << 29;
    }
    w
}

/// Minimal aircraft state record (no guns, weapons, sensors, ...).
pub struct State {
    pub pos: [f32; 3],
    pub euler: u32,
    pub vel: u32,
}

/// Write the per-entry uid prefix: explicit uid, or implicit previous + 1.
pub fn write_uid(w: &mut BitWriter, uid: Option<u16>) {
    match uid {
        Some(u) => {
            w.bit(true).varint(u as u64);
        }
        None => {
            w.bit(false);
        }
    }
}

/// Write one entry with a minimal state record.
pub fn write_state(w: &mut BitWriter, uid: Option<u16>, s: &State) {
    write_uid(w, uid);
    w.bit(false); // has data
    w.bit(false).bit(false); // hidden flags
    write_state_body(w, s, |_| {});
}

/// State body; `extras` writes the engines-through-seeker tail when given
/// a non-default closure (it must write everything after the controls).
pub fn write_state_body(w: &mut BitWriter, s: &State, extras: impl FnOnce(&mut BitWriter)) {
    w.bit(false).u32(0).bit(false); // ground, unk_u32, unk_bit
    w.bit(false); // no gun block
    w.zigzag(0); // no weapons
    w.f32(s.pos[0]).f32(s.pos[1]).f32(s.pos[2]).u32(s.euler).u32(s.vel);
    w.align();
    w.raw(&[0x88, 0, 0, 0, 0, 0, 0]); // controls
    let before = w.bits;
    extras(w);
    if w.bits == before {
        w.byte(0); // engines
        w.byte(0); // sensors
        w.byte(0); // countermeasures
        w.bits(4, 0); // targets
        w.bit(false).bit(false); // tail pair, seeker
    }
}

/// Terminate an entry list and append the trailer floats.
pub fn write_end(w: &mut BitWriter, trailer: [f32; 3]) {
    w.bit(true).varint(0x7FF);
    w.f32(trailer[0]).f32(trailer[1]).f32(trailer[2]);
}

/// MPI 0xD04A cockpit message payload.
pub fn cockpit_payload(params: &[(u16, f32)]) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.u16(0x5803).u16(0xD04A);
    w.varint(16 + 48 * params.len() as u64);
    w.u16(params.len() as u16);
    for (id, v) in params {
        w.u16(*id).f32(*v);
    }
    w.bytes
}
