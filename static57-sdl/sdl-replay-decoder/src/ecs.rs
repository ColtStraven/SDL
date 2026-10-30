//! ECS packet framing (packet type 6): entity creation records.
//!
//! Only framing is decoded: entity id, template name and the raw component
//! block. Component values are not interpreted here.
//!
//! ```text
//! packet  := control:u8 body            (0x21/0x23/0x25: body is an LZ4 block)
//! create  := count_minus_1:u8 message{count}
//! message := eid block_len:varint block[block_len]
//! block   := template_id:varint [name:len8 components, on first use] data
//! ```

use crate::bitstream::{BitReader, BitResult};
use std::collections::{HashMap, HashSet};

pub const ID_CREATION: u8 = 0x24;
pub const ID_CREATION_LZ4: u8 = 0x25;

#[derive(Debug, Clone, PartialEq)]
pub struct EntityCreate {
    /// Raw entity id: index in the low 22 bits, generation above.
    pub eid: u32,
    pub template: String,
    /// Component data after the template header, uninterpreted.
    pub data: Vec<u8>,
}

/// Dagor compact entity id: 2, 3 or 4 bytes.
pub fn read_compact_eid(r: &mut BitReader) -> BitResult<u32> {
    let first = r.u16_le("eid.first")? as u32;
    Ok(if first & 1 != 0 {
        (first >> 2) | (((first & 2) >> 1) << 22)
    } else if first & 2 != 0 {
        (first >> 2) | ((r.byte("eid.generation")? as u32) << 22)
    } else {
        let tail = r.u16_le("eid.tail")? as u32;
        let all = (tail << 16) | first;
        ((all & 0x00FF_FFFF) >> 2) | ((all >> 24) << 22)
    })
}

/// Template table, shared across a replay: names and component definitions
/// are sent only on first use.
#[derive(Debug, Default)]
pub struct Templates {
    names: HashMap<u64, String>,
    components: HashSet<u64>,
}

impl Templates {
    fn parse_block(&mut self, block: &[u8]) -> BitResult<(String, Vec<u8>)> {
        let mut r = BitReader::new(block);
        let id = r.compressed("template.id")?;
        if !self.names.contains_key(&id) {
            let n = r.byte("template.name_len")?;
            let mut name = Vec::with_capacity(n as usize);
            for _ in 0..n {
                name.push(r.byte("template.name")?);
            }
            let comps = r.u16_le("template.components")?;
            for _ in 0..comps {
                let cid = r.compressed("component.id")?;
                if self.components.insert(cid) {
                    r.skip(64, "component.hashes")?;
                }
            }
            self.names.insert(id, String::from_utf8_lossy(&name).into_owned());
        }
        let from = (r.position() / 8) as usize;
        Ok((self.names[&id].clone(), block[from.min(block.len())..].to_vec()))
    }
}

/// Decode an ECS packet if it is an entity-creation packet.
pub fn decode_creation(payload: &[u8], t: &mut Templates) -> Option<BitResult<Vec<EntityCreate>>> {
    let plain;
    let body: &[u8] = match *payload.first()? {
        ID_CREATION => &payload[1..],
        ID_CREATION_LZ4 => {
            let mut d = vec![0u8; payload.len() * 16 + 4096];
            let n = lz4_flex::block::decompress_into(&payload[1..], &mut d).ok()?;
            d.truncate(n);
            plain = d;
            &plain
        }
        _ => return None,
    };
    let mut out = Vec::new();
    let mut r = BitReader::new(body);
    let res: BitResult<()> = (|| {
        let count = r.byte("create.count")? as u32 + 1;
        for _ in 0..count {
            let eid = read_compact_eid(&mut r)?;
            let len = r.compressed("create.block_len")? as usize;
            let mut block = Vec::with_capacity(len);
            for _ in 0..len {
                block.push(r.byte("create.block")?);
            }
            let (template, data) = t.parse_block(&block)?;
            out.push(EntityCreate { eid, template, data });
        }
        Ok(())
    })();
    Some(res.map(|_| out))
}
