//! Dagor DataBlock (BLK) sections.
//!
//! Milestone 1/2 keeps BLK sections opaque: they are hashed and preserved,
//! and only the leading byte is surfaced. A native BLK reader is a separate
//! future module.

use crate::evidence::{hex, Evidence};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct BlkSummary {
    pub length: u64,
    pub leading_byte: Option<u8>,
    /// EXPERIMENTAL: binary BLK files are reported to start with a format
    /// byte; its meaning is not established for replay sections.
    pub leading_byte_evidence: Evidence,
    pub prefix_hex: String,
    /// Distinct printable ASCII runs (>= 4 chars) from the start of the block,
    /// as navigation hints only. Not decoded keys/values.
    pub printable_strings_preview: Vec<String>,
}

pub fn summarize(bytes: &[u8]) -> BlkSummary {
    BlkSummary {
        length: bytes.len() as u64,
        leading_byte: bytes.first().copied(),
        leading_byte_evidence: Evidence::Experimental,
        prefix_hex: hex(&bytes[..bytes.len().min(16)]),
        printable_strings_preview: printable_runs(bytes, 4, 24),
    }
}

fn printable_runs(bytes: &[u8], min: usize, max_count: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for &b in bytes.iter().chain(std::iter::once(&0u8)) {
        if (0x20..0x7f).contains(&b) {
            cur.push(b);
            continue;
        }
        if cur.len() >= min {
            let s = String::from_utf8_lossy(&cur).into_owned();
            if !out.contains(&s) {
                out.push(s);
                if out.len() == max_count {
                    break;
                }
            }
        }
        cur.clear();
    }
    out
}
