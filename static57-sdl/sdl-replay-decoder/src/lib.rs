//! Static 57 SDL — native War Thunder `.wrpl` replay decoder (S57-003).
//!
//! Pipeline: container header -> section layout -> codec probe -> bounded
//! streaming inflation -> packet framing and clock reconstruction -> payload
//! decoders (flight, identity, battle log, weapons) -> research datasets and
//! reports. See `docs/ARCHITECTURE.md`.

pub mod aircraft;
pub mod bitstream;
pub mod cockpit;
pub mod combat;
pub mod compression;
pub mod container;
pub mod datablock;
pub mod diagnostics;
pub mod ecs;
pub mod evidence;
pub mod export;
pub mod flight;
pub mod idfield;
pub mod lab;
pub mod manifest;
pub mod measure;
pub mod packets;
pub mod report;
pub mod research;
pub mod seeker;
pub mod shells;
pub mod simcapture;
pub mod strikemaster;
pub mod survey;
pub mod sync;
pub mod systems;
pub mod telemetry;
pub mod tracks;
pub mod units;
pub mod validate;
pub mod weapons;
pub mod versions;
