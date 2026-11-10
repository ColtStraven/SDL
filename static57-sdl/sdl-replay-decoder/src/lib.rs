//! Static 57 SDL — native War Thunder `.wrpl` replay decoder (S57-003).
//!
//! Pipeline: container header -> section layout -> codec probe -> bounded
//! streaming inflation -> packet framing and clock reconstruction -> manifest
//! and JSON Lines export. Payload decoders (flight, weapons, sensors, ...) are
//! future modules layered on [`packets::PacketRecord`]; see
//! `docs/ARCHITECTURE.md`.

pub mod bits;
pub mod compression;
pub mod container;
pub mod datablock;
pub mod diagnostics;
pub mod evidence;
pub mod export;
pub mod flight;
pub mod manifest;
pub mod mpi;
pub mod packets;
pub mod versions;
