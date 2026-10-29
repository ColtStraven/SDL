//! Research classification for every value SDL reports.
//!
//! SDL is a scientific instrument: each reported value carries how strongly it
//! is established, so downstream analysis never mistakes an inferred value for
//! a directly decoded one.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Evidence {
    /// Demonstrated by documented structure plus a reproducible cross-check in
    /// the file being decoded (e.g. a section boundary that lands exactly on a
    /// codec magic).
    Verified,
    /// Documented by external researchers, not independently confirmed here.
    Reported,
    /// Calculated from other measurements.
    Derived,
    /// Plausible interpretation awaiting validation.
    Experimental,
    /// Not established.
    Unknown,
}

/// Where a value came from in the source file.
#[derive(Debug, Clone, Serialize)]
pub struct Source {
    /// Section name (`header`, `settings_blk`, `packet_stream`, ...).
    pub section: &'static str,
    /// Absolute byte offset in the replay file (or decompressed stream when
    /// `section` is `decompressed_stream`).
    pub offset: u64,
    pub length: u64,
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0xf) as usize] as char);
    }
    s
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex(&Sha256::digest(bytes))
}
