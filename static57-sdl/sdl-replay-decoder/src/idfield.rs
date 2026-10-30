//! Dagor `IdFieldSerializer` (32-field variant) used by battle messages.
//!
//! ```text
//! message := offset:u16le field_mask:varint fields... sizes
//! sizes   := 3-bit code per set mask bit, located at byte `offset` from the
//!            message start: 1=1 bit, 2=8, 3=16, 4=32, 5=64, 6=96, 7=128,
//!            0=varint bit count follows
//! ```
//!
//! Fields follow in ascending field-number order, each exactly its declared
//! size. Field numbers are 1-based here to match the reported tables (the
//! mask bit number).

use crate::bitstream::{BitError, BitReader, BitResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub number: u8,
    pub bit_offset: u64,
    pub bits: u64,
}

fn read_size(r: &mut BitReader) -> BitResult<u64> {
    Ok(match r.bits(3, "idfield.size_code")? {
        1 => 1,
        2 => 8,
        3 => 16,
        4 => 32,
        5 => 64,
        6 => 96,
        7 => 128,
        _ => r.compressed("idfield.size")?,
    })
}

/// Parse the field table of a message starting at byte 0 of `msg`.
pub fn parse(msg: &[u8]) -> BitResult<Vec<Field>> {
    let mut r = BitReader::new(msg);
    let offset = r.u16_le("idfield.offset")? as usize;
    let mask = r.compressed("idfield.mask")? as u32;
    let body = r.position();
    if offset > msg.len() {
        return Err(BitError { bit: 0, wanted_bits: offset as u64 * 8, available_bits: msg.len() as u64 * 8, what: "idfield size table past end" });
    }
    let mut sizes = BitReader::new(&msg[offset..]);
    let mut out = Vec::with_capacity(mask.count_ones() as usize);
    let mut at = body;
    for n in 0..32u8 {
        if mask & (1 << n) == 0 {
            continue;
        }
        let bits = read_size(&mut sizes)?;
        out.push(Field { number: n, bit_offset: at, bits });
        at += bits;
    }
    if at > offset as u64 * 8 {
        return Err(BitError { bit: at, wanted_bits: 0, available_bits: offset as u64 * 8, what: "idfield fields overrun size table" });
    }
    Ok(out)
}

/// Reader positioned at a field.
pub fn field_reader<'a>(msg: &'a [u8], f: &Field) -> BitReader<'a> {
    let mut r = BitReader::new(msg);
    r.skip(f.bit_offset, "idfield.seek").expect("field offset inside message");
    r
}

/// A `len8 + bytes` string field.
pub fn read_string(msg: &[u8], f: &Field) -> BitResult<String> {
    let mut r = field_reader(msg, f);
    let n = r.byte("idfield.str_len")?;
    let mut s = Vec::with_capacity(n as usize);
    for _ in 0..n {
        s.push(r.byte("idfield.str")?);
    }
    Ok(String::from_utf8_lossy(&s).into_owned())
}
