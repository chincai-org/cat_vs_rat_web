//! Share links use compact versioned payloads.

use crate::map::{COLS, Cell, Map, MapError, ROWS};

/// Longest token accepted (about 4 KB), so a link cannot carry anything huge.
pub const MAX_TOKEN_LEN: usize = 4096;

const ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

const PACKED_MAGIC: &[u8; 3] = b"CVR";
const PACKED_VERSION: u8 = 1;
const CATEGORY_COUNT: usize = 6;
const HEADER_LEN: usize = PACKED_MAGIC.len() + 1 + CATEGORY_COUNT;
const CELL_COUNT: usize = (COLS * ROWS) as usize;

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 4).div_ceil(3));
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((n >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 63) as usize] as char);
        }
    }
    out
}

pub fn decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

fn cell_index(cell: Cell) -> Option<usize> {
    if !Map::in_bounds(cell[0], cell[1]) {
        return None;
    }
    let columns = usize::try_from(COLS).ok()?;
    let x = usize::try_from(cell[0]).ok()?;
    let y = usize::try_from(cell[1]).ok()?;
    y.checked_mul(columns)?.checked_add(x)
}

fn cell_from_index(index: u8) -> Result<Cell, MapError> {
    let index = usize::from(index);
    if index >= CELL_COUNT {
        return Err(MapError::BadLink);
    }
    let columns = usize::try_from(COLS).map_err(|_| MapError::BadLink)?;
    let x = i32::try_from(index % columns).map_err(|_| MapError::BadLink)?;
    let y = i32::try_from(index / columns).map_err(|_| MapError::BadLink)?;
    Ok([x, y])
}

fn encode_packed_map(map: &Map) -> Option<Vec<u8>> {
    if map.cats.is_empty()
        || map.rats.is_empty()
        || map.tomatoes.is_empty()
        || (!map.holes.is_empty() && map.holes.len() != 2)
    {
        return None;
    }

    let mut occupied = [false; CELL_COUNT];
    for cells in [&map.boxes, &map.cats, &map.rats, &map.tomatoes, &map.spikes]
    {
        for &cell in cells {
            let index = cell_index(cell)?;
            if std::mem::replace(&mut occupied[index], true) {
                return None;
            }
        }
    }
    for &[x, y] in &map.holes {
        let left = cell_index([x, y])?;
        let right_x = x.checked_add(1)?;
        let right = cell_index([right_x, y])?;
        if std::mem::replace(&mut occupied[left], true)
            || std::mem::replace(&mut occupied[right], true)
        {
            return None;
        }
    }

    let lists = [
        &map.boxes,
        &map.cats,
        &map.rats,
        &map.tomatoes,
        &map.holes,
        &map.spikes,
    ];
    let entry_count = lists.iter().map(|cells| cells.len()).sum::<usize>();
    let mut bytes = Vec::with_capacity(HEADER_LEN + entry_count);
    bytes.extend_from_slice(PACKED_MAGIC);
    bytes.push(PACKED_VERSION);
    for cells in &lists {
        bytes.push(u8::try_from(cells.len()).ok()?);
    }
    for cells in &lists {
        for &cell in cells.iter() {
            bytes.push(u8::try_from(cell_index(cell)?).ok()?);
        }
    }
    Some(bytes)
}

fn decode_cells(
    bytes: &[u8],
    cursor: &mut usize,
    count: usize,
    occupied: &mut [bool; CELL_COUNT],
) -> Result<Vec<Cell>, MapError> {
    let end = cursor.checked_add(count).ok_or(MapError::BadLink)?;
    let encoded_cells = bytes.get(*cursor..end).ok_or(MapError::BadLink)?;
    let mut cells = Vec::with_capacity(count);
    for &encoded_cell in encoded_cells {
        let cell = cell_from_index(encoded_cell)?;
        let index = usize::from(encoded_cell);
        if std::mem::replace(&mut occupied[index], true) {
            return Err(MapError::BadLink);
        }
        cells.push(cell);
    }
    *cursor = end;
    Ok(cells)
}

fn decode_packed_map(bytes: &[u8]) -> Result<Map, MapError> {
    if bytes.len() < HEADER_LEN
        || !bytes.starts_with(PACKED_MAGIC)
        || bytes[PACKED_MAGIC.len()] != PACKED_VERSION
    {
        return Err(MapError::BadLink);
    }

    let counts = &bytes[PACKED_MAGIC.len() + 1..HEADER_LEN];
    debug_assert_eq!(counts.len(), CATEGORY_COUNT);
    let entry_count = counts
        .iter()
        .map(|&count| usize::from(count))
        .sum::<usize>();
    if bytes.len() != HEADER_LEN + entry_count {
        return Err(MapError::BadLink);
    }

    let mut cursor = HEADER_LEN;
    let mut occupied = [false; CELL_COUNT];
    let boxes = decode_cells(
        bytes,
        &mut cursor,
        usize::from(counts[0]),
        &mut occupied,
    )?;
    let cats = decode_cells(
        bytes,
        &mut cursor,
        usize::from(counts[1]),
        &mut occupied,
    )?;
    let rats = decode_cells(
        bytes,
        &mut cursor,
        usize::from(counts[2]),
        &mut occupied,
    )?;
    let tomatoes = decode_cells(
        bytes,
        &mut cursor,
        usize::from(counts[3]),
        &mut occupied,
    )?;
    let holes = decode_cells(
        bytes,
        &mut cursor,
        usize::from(counts[4]),
        &mut occupied,
    )?;
    for &[x, y] in &holes {
        let right_x = x.checked_add(1).ok_or(MapError::BadLink)?;
        let right = cell_index([right_x, y]).ok_or(MapError::BadLink)?;
        if std::mem::replace(&mut occupied[right], true) {
            return Err(MapError::BadLink);
        }
    }
    let spikes = decode_cells(
        bytes,
        &mut cursor,
        usize::from(counts[5]),
        &mut occupied,
    )?;
    let map = Map {
        boxes,
        cats,
        rats,
        tomatoes,
        holes,
        spikes,
    };
    if !map.validate().is_empty() {
        return Err(MapError::BadLink);
    }
    Ok(map)
}

pub fn token_from_map(map: &Map) -> String {
    // Only validated maps are shared, and validation guarantees the packed
    // encoding can represent them.
    let bytes = encode_packed_map(map).expect("only valid maps are shared");
    encode(&bytes)
}

/// Decode a compact share token.
pub fn map_from_token(token: &str) -> Result<Map, MapError> {
    if token.is_empty() || token.len() > MAX_TOKEN_LEN {
        return Err(MapError::BadLink);
    }
    let bytes = decode(token).ok_or(MapError::BadLink)?;
    decode_packed_map(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP_1: &str = include_str!("../../../maps/map_1.txt");

    #[test]
    fn base64_round_trip() {
        for len in 0..20usize {
            let data: Vec<u8> =
                (0..len as u8).map(|b| b.wrapping_mul(37)).collect();
            assert_eq!(decode(&encode(&data)).unwrap(), data);
        }
    }

    #[test]
    fn map_round_trip() {
        let m = Map::from_text(MAP_1).unwrap();
        assert_eq!(map_from_token(&token_from_map(&m)).unwrap(), m);
    }

    #[test]
    fn packed_share_tokens_are_shorter_and_round_trip() {
        let map = Map::from_text(MAP_1).unwrap();
        let token = token_from_map(&map);
        let text_token = encode(map.to_text().as_bytes());

        assert!(token.len() < text_token.len());
        assert!(decode(&token).unwrap().starts_with(PACKED_MAGIC));
        assert_eq!(map_from_token(&token).unwrap(), map);
    }

    #[test]
    fn packed_shares_preserve_cell_order_and_holes() {
        let map = Map {
            boxes: vec![[9, 8], [1, 1]],
            cats: vec![[15, 8], [0, 9]],
            rats: vec![[12, 2], [3, 7], [18, 1]],
            tomatoes: vec![[6, 4], [12, 7]],
            holes: vec![[18, 9], [3, 6]],
            spikes: vec![[17, 9]],
        };

        assert_eq!(map_from_token(&token_from_map(&map)).unwrap(), map);
    }

    #[test]
    fn malformed_packed_share_tokens_are_rejected() {
        let mut bytes = PACKED_MAGIC.to_vec();
        bytes.push(PACKED_VERSION);
        bytes.extend_from_slice(&[1, 1, 1, 1, 0, 0, 200, 0, 1, 2]);

        assert_eq!(map_from_token(&encode(&bytes)), Err(MapError::BadLink));
    }

    #[test]
    fn rejects_garbage_and_oversized_tokens() {
        assert_eq!(map_from_token("!!!"), Err(MapError::BadLink));
        assert_eq!(map_from_token(""), Err(MapError::BadLink));
        assert_eq!(
            map_from_token(&"A".repeat(MAX_TOKEN_LEN + 1)),
            Err(MapError::BadLink)
        );
        assert!(map_from_token(&encode(b"not a map")).is_err());
    }
}
