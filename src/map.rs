use crate::elvl;
use anyhow::*;
use image::{self, DynamicImage};
use std::{fs, io::Cursor};

pub type TileId = u8;

pub const TILE_ID_FIRST_DOOR: TileId = 162;
pub const TILE_ID_LAST_DOOR: TileId = 169;
pub const TILE_ID_FLAG: TileId = 170;
pub const TILE_ID_SAFE: TileId = 171;
pub const TILE_ID_GOAL: TileId = 172;
pub const TILE_ID_WORMHOLE: TileId = 220;

struct ReadTile {
    value: u32,
}

impl ReadTile {
    pub fn new(value: u32) -> Self {
        Self { value }
    }

    pub fn x(&self) -> u16 {
        (self.value & 0xFFF) as u16
    }

    pub fn y(&self) -> u16 {
        ((self.value >> 12) & 0xFFF) as u16
    }

    pub fn id(&self) -> u8 {
        ((self.value >> 24) & 0xFF) as u8
    }
}

pub struct Map {
    pub filename: String,
    pub elvl: Vec<elvl::Chunk>,
    tiles: Box<[TileId; 1024 * 1024]>,
    pub tileset: Option<DynamicImage>,
}

impl Map {
    pub fn empty() -> Self {
        Self {
            filename: String::new(),
            elvl: vec![],
            tiles: vec![0; 1024 * 1024].into_boxed_slice().try_into().unwrap(),
            tileset: None,
        }
    }

    pub fn load(filename: &str) -> anyhow::Result<Self> {
        let mut map = Self {
            filename: filename.to_owned(),
            elvl: vec![],
            tiles: vec![0; 1024 * 1024].into_boxed_slice().try_into().unwrap(),
            tileset: None,
        };

        let data = fs::read(filename)?;

        // Fully empty map is fine.
        if data.len() < 2 {
            return Ok(map);
        }

        let mut tiledata_offset: usize = 0;

        if data[0] == b'B' && data[1] == b'M' {
            // TODO: Read tileset bitmap
            if data.len() < 10 {
                return Err(anyhow!("invalid bitmap header"));
            }

            let img = image::ImageReader::new(Cursor::new(data.clone()))
                .with_guessed_format()?
                .decode()?;
            map.tileset = Some(img);

            tiledata_offset = u32::from_le_bytes(data[2..6].try_into().unwrap()) as usize;
        }

        if tiledata_offset >= data.len() {
            return Err(anyhow!("tile data offset larger than file data length"));
        }

        let tiledata = &data[tiledata_offset..];
        let tiledata_size = data.len() - tiledata_offset;
        let tile_count = tiledata_size / size_of::<u32>();

        for i in 0..tile_count {
            let tile_offset = i * size_of::<u32>();
            let tile = ReadTile::new(u32::from_le_bytes(
                tiledata[tile_offset..tile_offset + 4].try_into().unwrap(),
            ));

            let index = tile.y() as usize * 1024 + tile.x() as usize;
            map.tiles[index] = tile.id();
        }

        map.elvl = elvl::elvl_read(&data)?;

        Ok(map)
    }

    pub fn get_attributes(&self) -> Vec<&elvl::Attribute> {
        self.elvl
            .iter()
            .filter_map(|chunk| match chunk {
                elvl::Chunk::Attribute(attr) => Some(attr),
                _ => None,
            })
            .collect()
    }
}
