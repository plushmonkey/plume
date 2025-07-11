use anyhow::*;
use bit_set::BitSet;

const METADATA_HEADER_SIZE: usize = 12;
const CHUNK_HEADER_SIZE: usize = 8;

struct MetadataHeader {
    magic: u32,
    total_size: u32,
}

impl MetadataHeader {
    pub fn new(data: &[u8; METADATA_HEADER_SIZE]) -> Self {
        let magic = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let total_size = u32::from_le_bytes(data[4..8].try_into().unwrap());

        Self { magic, total_size }
    }
}

struct ChunkHeader {
    kind: u32,
    size: u32,
}

impl ChunkHeader {
    pub fn new(data: &[u8; CHUNK_HEADER_SIZE]) -> Self {
        let kind = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let size = u32::from_le_bytes(data[4..8].try_into().unwrap());

        Self { kind, size }
    }
}

pub struct Attribute {
    pub key: String,
    pub value: String,
}

#[allow(nonstandard_style)]
pub mod RegionFlags {
    pub const Base: u32 = 1 << 0;
    pub const NoAntiwarp: u32 = 1 << 1;
    pub const NoWeapons: u32 = 1 << 2;
    pub const NoFlags: u32 = 1 << 3;
}

pub struct Region {
    pub name: String,
    pub flags: u32,
    pub tiles: BitSet,
    pub tile_count: u32,
}

impl Region {
    pub fn empty() -> Self {
        Self {
            name: String::new(),
            flags: 0,
            tiles: BitSet::new(),
            tile_count: 0,
        }
    }
    pub fn set_tile(&mut self, x: u16, y: u16) {
        let index = Self::get_index(x, y);

        if self.tiles.insert(index) {
            self.tile_count += 1;
        }
    }

    pub fn in_region(&self, x: u16, y: u16) -> bool {
        let index = Self::get_index(x, y);

        self.tiles.contains(index)
    }

    pub fn get_tiles(&self) -> Vec<(u16, u16)> {
        self.tiles
            .iter()
            .map(|index| ((index % 1024) as u16, (index / 1024) as u16))
            .collect()
    }

    pub fn parse_data(&mut self, data: &[u8], mut coord: (u16, u16)) -> Result<(u16, u16)> {
        let mut data = &data[..];

        while !data.is_empty() {
            let sequence_kind = data[0] >> 5;
            // This sequence type is based on the first 3 bits.
            // The 1-32 and 1-1024 of the same type are used for optimization since it would require more bits
            // to encode 1024 always. By using 3 bits to determine, the 1-32 can fit in the remaining 5 bits
            // instead of having to have an extra byte that would be needed for 1024.
            //
            // Since a single tile would be required for the existence of one of these types, it is encoded as
            // +1 from the remaining bit value. That allows 5 bits to be used for 31(32) since 32 wouldn't
            // normally fit.
            let mut consumed = 1;

            let advance = |mut coord: (u16, u16), run: u16| -> (u16, u16) {
                coord.0 += run;
                if coord.0 >= 1024 {
                    coord.0 = 0;
                    coord.1 += 1;
                }
                coord
            };

            match sequence_kind {
                0 => {
                    // 1-32 Empty tiles in a row
                    let run = ((data[0] & 0x1F) + 1) as u16;

                    coord = advance(coord, run);
                    consumed = 1;
                }
                1 => {
                    // 1-1024 Empty tiles in a row
                    if data.len() < 2 {
                        return Err(anyhow!("unexpected end of data during region tile parsing"));
                    }

                    let run = (((data[0] as u16 & 3) << 8) | (data[1] as u16)) + 1;

                    coord = advance(coord, run);
                    consumed = 2;
                }
                2 => {
                    // 1-32 Present tiles in a row
                    let run = ((data[0] & 0x1F) + 1) as u16;

                    for i in 0..run {
                        self.set_tile(coord.0 + i, coord.1);
                    }

                    coord = advance(coord, run);
                    consumed = 1;
                }
                3 => {
                    // 1-1024 Present tiles in a row
                    if data.len() < 2 {
                        return Err(anyhow!("unexpected end of data during region tile parsing"));
                    }

                    let run = (((data[0] as u16 & 3) << 8) | (data[1] as u16)) + 1;

                    for i in 0..run {
                        self.set_tile(coord.0 + i, coord.1);
                    }

                    coord = advance(coord, run);
                    consumed = 2;
                }
                4 => {
                    // 1-32 Rows of empty
                    let run = ((data[0] & 0x1F) + 1) as u16;

                    coord.0 = 0;
                    coord.1 += run;
                    consumed = 1;
                }
                5 => {
                    // 1-1024 Rows of empty
                    if data.len() < 2 {
                        return Err(anyhow!("unexpected end of data during region tile parsing"));
                    }

                    let run = (((data[0] as u16 & 3) << 8) | (data[1] as u16)) + 1;

                    coord.0 = 0;
                    coord.1 += run;
                    consumed = 2;
                }
                6 => {
                    // Repeat last row 1-32 times
                    let run = ((data[0] & 0x1F) + 1) as u16;

                    for i in 0..run {
                        for x in 0..1024 {
                            if self.in_region(x, coord.1 - 1) {
                                self.set_tile(x, coord.1 + i);
                            }
                        }
                    }

                    coord.0 = 0;
                    coord.1 += run;
                    consumed = 1;
                }
                7 => {
                    // Repeat last row 1-1024 times
                    let run = (((data[0] as u16 & 3) << 8) | (data[1] as u16)) + 1;

                    for i in 0..run {
                        for x in 0..1024 {
                            if self.in_region(x, coord.1 - 1) {
                                self.set_tile(x, coord.1 + i);
                            }
                        }
                    }

                    coord.0 = 0;
                    coord.1 += run;
                    consumed = 2;
                }
                _ => {}
            }

            data = &data[consumed..];
        }

        Ok(coord)
    }

    fn get_index(x: u16, y: u16) -> usize {
        y as usize * 1024 + x as usize
    }
}

pub enum Chunk {
    Attribute(Attribute),
    Region(Region),
    Tileset,
    Tile,

    // DCME hash code
    DcmeId(u32),
    DcmeWallTiles,
    DcmeTextTiles,
    DcmeBookmarks,
    DcmeLvz,

    // Kind, Payload
    Other(u32, Vec<u8>),
}

pub fn elvl_read(data: &[u8]) -> anyhow::Result<Vec<Chunk>> {
    let mut chunks = vec![];

    if data.len() < 10 {
        return Ok(chunks);
    }

    // This doesn't have a bitmap header, so it must not contain elvl data.
    if data[0] != b'B' || data[1] != b'M' {
        return Ok(chunks);
    }

    let metadata_offset = u32::from_le_bytes(data[6..10].try_into().unwrap()) as usize;
    if metadata_offset == 0 {
        return Ok(chunks);
    }

    if data.len() < metadata_offset + METADATA_HEADER_SIZE {
        // This isn't a valid elvl file, so ignore it. No error because map files don't need elvl sections.
        return Ok(chunks);
    }

    let header = MetadataHeader::new(
        data[metadata_offset..metadata_offset + METADATA_HEADER_SIZE]
            .try_into()
            .unwrap(),
    );

    if header.magic != 0x6c766c65 {
        // This isn't a valid elvl file, so ignore it. No error because map files don't need elvl sections.
        return Ok(chunks);
    }

    let mut data = &data[metadata_offset + METADATA_HEADER_SIZE..];
    let mut consumed: usize = METADATA_HEADER_SIZE;

    while data.len() >= CHUNK_HEADER_SIZE && consumed < header.total_size as usize {
        let chunk_header = ChunkHeader::new(data[0..CHUNK_HEADER_SIZE].try_into().unwrap());
        let payload = &data[CHUNK_HEADER_SIZE..CHUNK_HEADER_SIZE + chunk_header.size as usize];

        let chunk = match chunk_header.kind {
            0x52545441 => {
                // ATTR
                let mut parts = payload.splitn(2, |c| *c == b'=');

                let key = parts.next();
                let value = parts.next();

                if let (Some(key), Some(value)) = (key, value) {
                    let key = std::str::from_utf8(key)?;
                    let value = std::str::from_utf8(value)?;

                    let data = Attribute {
                        key: key.to_owned(),
                        value: value.to_owned(),
                    };

                    Chunk::Attribute(data)
                } else {
                    return Err(anyhow!("attribute data did not have key value split"));
                }
            }
            0x4E474552 => {
                // REGN
                let mut region = Region::empty();

                let mut region_data = &payload[..];

                const REGION_CHUNK_HEADER_SIZE: usize = 8;

                let mut coord = (0u16, 0u16);

                while region_data.len() > REGION_CHUNK_HEADER_SIZE {
                    let kind = u32::from_le_bytes(region_data[0..4].try_into().unwrap());
                    let chunk_size =
                        u32::from_le_bytes(region_data[4..8].try_into().unwrap()) as usize;
                    let region_chunk_payload = &region_data[8..8 + chunk_size];

                    match kind {
                        0x4D414E72 => {
                            // rNAM
                            region.name = std::str::from_utf8(region_chunk_payload)
                                .unwrap()
                                .to_owned();
                        }
                        0x4C495472 => {
                            // rTIL
                            coord = region.parse_data(region_chunk_payload, coord)?;
                        }
                        0x45534272 => {
                            // rBSE
                            region.flags |= RegionFlags::Base;
                        }
                        0x57414E72 => {
                            // rNAW
                            region.flags |= RegionFlags::NoAntiwarp;
                        }
                        0x50574E72 => {
                            // rNWP
                            region.flags |= RegionFlags::NoWeapons;
                        }
                        0x4C464E72 => {
                            // rNFL
                            region.flags |= RegionFlags::NoFlags;
                        }
                        // TODO: rAWP, rPYC
                        _ => {}
                    }

                    let total_chunk_size =
                        REGION_CHUNK_HEADER_SIZE + ((chunk_size as usize + 3) & !3);
                    region_data = &region_data[total_chunk_size..];
                }

                Chunk::Region(region)
            }
            _ => Chunk::Other(chunk_header.kind, payload.to_owned()),
        };

        chunks.push(chunk);

        // Align data to 4 bytes
        let total_chunk_size = CHUNK_HEADER_SIZE + ((chunk_header.size as usize + 3) & !3);

        data = &data[total_chunk_size..];
        consumed += total_chunk_size;
    }

    Ok(chunks)
}
