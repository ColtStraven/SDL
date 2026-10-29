//! Replay format version registry.
//!
//! Every format magic SDL knows about is listed explicitly with its evidence
//! status and codec policy. Unknown magics are never silently treated as a
//! known layout: they are inspected best-effort and flagged.

use crate::evidence::Evidence;
use serde::Serialize;

/// Container signature at offset 0 (`E5 AC 00 10` on disk).
pub const WRPL_SIGNATURE: u32 = 0x1000_ACE5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    Zlib,
    Zstd,
}

#[derive(Debug, Clone, Serialize)]
pub struct FormatProfile {
    pub magic: u32,
    pub name: &'static str,
    /// Codec the packet stream is expected to use for this format.
    pub expected_codec: Codec,
    /// Whether the 1234-byte header layout is expected to apply.
    pub header_layout: &'static str,
    pub evidence: Evidence,
    pub notes: &'static str,
}

pub const PROFILES: &[FormatProfile] = &[
    FormatProfile {
        magic: 0x0001_8C0A,
        name: "18c0a-dev",
        expected_codec: Codec::Zlib,
        header_layout: "wrpl-1234",
        evidence: Evidence::Reported,
        notes: "Dev-server format reported by third-party parsers. No Static 57 sample.",
    },
    FormatProfile {
        magic: 0x0001_8C0B,
        name: "18c0b",
        expected_codec: Codec::Zlib,
        header_layout: "wrpl-1234",
        evidence: Evidence::Reported,
        notes: "Pre-2.59 format with zlib packet stream, reported by third-party parsers. No Static 57 sample.",
    },
    FormatProfile {
        magic: 0x0001_8C1C,
        name: "18c1c",
        expected_codec: Codec::Zstd,
        header_layout: "wrpl-1234",
        evidence: Evidence::Verified,
        notes: "Game 2.59 client replays with zstd packet stream. Verified on Static 57 replays recorded 2026-10-22..25 (game version field 2.59.0.80-86).",
    },
];

pub fn lookup(magic: u32) -> Option<&'static FormatProfile> {
    PROFILES.iter().find(|p| p.magic == magic)
}
