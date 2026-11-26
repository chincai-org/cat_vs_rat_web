use std::collections::{HashSet, VecDeque};
use std::fmt;

pub const COLS: i32 = 20;
pub const ROWS: i32 = 10;

/// A grid cell as [x, y].
pub type Cell = [i32; 2];

/// One symbol per cell, 20 columns by 10 rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Map {
    pub boxes: Vec<Cell>,
    pub cats: Vec<Cell>,
    pub rats: Vec<Cell>,
    pub tomatoes: Vec<Cell>,
    /// Left cell of each 2x1 hole. Must hold 0 or 2 entries.
    pub holes: Vec<Cell>,
    pub spikes: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapError {
    OutOfBounds { x: i32, y: i32 },
    Overlap { x: i32, y: i32 },
    HoleCount(usize),
    NoCat,
    NoRat,
    NoTomato,
    Text(String),
    BadLink,
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::OutOfBounds { x, y } => {
                write!(f, "Cell ({x}, {y}) is outside the {COLS}x{ROWS} board.")
            }
            MapError::Overlap { x, y } => {
                write!(f, "Two objects share cell ({x}, {y}).")
            }
            MapError::HoleCount(n) => {
                write!(f, "A map needs 0 or 2 holes, found {n}.")
            }
            MapError::NoCat => write!(f, "Add at least one cat."),
            MapError::NoRat => write!(f, "Add at least one rat."),
            MapError::NoTomato => write!(f, "Add at least one tomato."),
            MapError::Text(e) => write!(f, "Could not read the map text: {e}"),
            MapError::BadLink => {
                write!(f, "This share link is not a valid map.")
            }
        }
    }
}

fn text_error(line: usize, column: usize, message: &str) -> MapError {
    MapError::Text(format!(
        "line {}, column {}: {message}",
        line + 1,
        column + 1
    ))
}

impl Map {
    /// Read the canonical 20x10 symbol grid.
    pub fn from_text(s: &str) -> Result<Map, MapError> {
        let mut lines: Vec<&str> = s.lines().collect();
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        if lines.len() != ROWS as usize {
            return Err(MapError::Text(format!(
                "expected {ROWS} lines, found {}",
                lines.len()
            )));
        }

        let mut map = Map::default();
        for (y, line) in lines.iter().enumerate() {
            let bytes = line.as_bytes();
            if bytes.len() != COLS as usize {
                return Err(MapError::Text(format!(
                    "line {}: expected {COLS} columns, found {}",
                    y + 1,
                    bytes.len()
                )));
            }
            for (x, &symbol) in bytes.iter().enumerate() {
                let cell = [x as i32, y as i32];
                match symbol {
                    b'.' => {}
                    b'#' => map.boxes.push(cell),
                    b'C' => map.cats.push(cell),
                    b'R' => map.rats.push(cell),
                    b'T' => map.tomatoes.push(cell),
                    b'S' => map.spikes.push(cell),
                    b'[' => {
                        if bytes.get(x + 1) != Some(&b']') {
                            return Err(text_error(
                                y,
                                x,
                                "'[' must be followed by ']'",
                            ));
                        }
                        map.holes.push(cell);
                    }
                    b']' => {
                        if x == 0 || bytes[x - 1] != b'[' {
                            return Err(text_error(
                                y,
                                x,
                                "']' must follow '['",
                            ));
                        }
                    }
                    other => {
                        return Err(text_error(
                            y,
                            x,
                            &format!("unknown symbol '{}'", other as char),
                        ));
                    }
                }
            }
        }

        match map.validate().into_iter().next() {
            Some(e) => Err(e),
            None => Ok(map),
        }
    }

    /// Write the canonical 20x10 symbol grid. Every cell is one character.
    pub fn to_text(&self) -> String {
        debug_assert!(
            self.validate().is_empty(),
            "only valid maps convert to text"
        );
        let mut rows = [[b'.'; COLS as usize]; ROWS as usize];
        for &[x, y] in &self.boxes {
            rows[y as usize][x as usize] = b'#';
        }
        for &[x, y] in &self.cats {
            rows[y as usize][x as usize] = b'C';
        }
        for &[x, y] in &self.rats {
            rows[y as usize][x as usize] = b'R';
        }
        for &[x, y] in &self.tomatoes {
            rows[y as usize][x as usize] = b'T';
        }
        for &[x, y] in &self.spikes {
            rows[y as usize][x as usize] = b'S';
        }
        for &[x, y] in &self.holes {
            rows[y as usize][x as usize] = b'[';
            rows[y as usize][x as usize + 1] = b']';
        }

        let mut out = String::with_capacity(((COLS + 1) * ROWS) as usize);
        for row in rows {
            for symbol in row {
                out.push(symbol as char);
            }
            out.push('\n');
        }
        out
    }

    pub fn in_bounds(x: i32, y: i32) -> bool {
        (0..COLS).contains(&x) && (0..ROWS).contains(&y)
    }

    /// Every cell used by an object. A hole uses two.
    fn occupied_cells(&self) -> Vec<Cell> {
        let mut v: Vec<Cell> = Vec::new();
        for list in [
            &self.boxes,
            &self.cats,
            &self.rats,
            &self.tomatoes,
            &self.spikes,
        ] {
            v.extend(list.iter().copied());
        }
        for h in &self.holes {
            v.push(*h);
            // A hole at i32::MAX cannot have a right cell; validate reports it.
            if let Some(right) = h[0].checked_add(1) {
                v.push([right, h[1]]);
            }
        }
        v
    }

    pub fn is_free(&self, x: i32, y: i32) -> bool {
        !self.occupied_cells().iter().any(|c| c[0] == x && c[1] == y)
    }

    /// Warnings for players that cannot reach their opponents' goals through the map.
    pub fn sealed_in_warnings(&self) -> Vec<&'static str> {
        if !self.validate().is_empty() {
            return Vec::new();
        }

        let cat_reach: Vec<HashSet<Cell>> = self
            .cats
            .iter()
            .map(|&cat| reachable_cells(self, cat, true))
            .collect();
        let rat_reach: Vec<HashSet<Cell>> = self
            .rats
            .iter()
            .map(|&rat| reachable_cells(self, rat, false))
            .collect();

        let cat_sealed =
            self.cats.iter().zip(&cat_reach).any(|(_, reach)| {
                !self.rats.iter().any(|rat| reach.contains(rat))
            });
        let rat_sealed = self.rats.iter().zip(&rat_reach).any(|(_, reach)| {
            !self.tomatoes.iter().any(|tomato| reach.contains(tomato))
        });
        // The rat wins only after every tomato is eaten, so one unreachable
        // tomato is enough to make the round unwinnable for the rats.
        let tomato_unreachable = self.tomatoes.iter().any(|tomato| {
            !rat_reach.iter().any(|reach| reach.contains(tomato))
        });

        let mut warnings = Vec::new();
        if cat_sealed {
            warnings.push("Warning: a cat is sealed away from all rats.");
        }
        if rat_sealed {
            warnings.push("Warning: a rat is sealed away from all tomatoes.");
        }
        if tomato_unreachable {
            warnings.push("Warning: a tomato is sealed away from all rats.");
        }
        warnings
    }

    /// All problems that stop this map from being saved or played.
    pub fn validate(&self) -> Vec<MapError> {
        let mut errs = Vec::new();

        for list in [
            &self.boxes,
            &self.cats,
            &self.rats,
            &self.tomatoes,
            &self.spikes,
        ] {
            for c in list {
                if !Self::in_bounds(c[0], c[1]) {
                    errs.push(MapError::OutOfBounds { x: c[0], y: c[1] });
                }
            }
        }
        for h in &self.holes {
            let right_in_bounds = h[0]
                .checked_add(1)
                .is_some_and(|right| Self::in_bounds(right, h[1]));
            if !Self::in_bounds(h[0], h[1]) || !right_in_bounds {
                errs.push(MapError::OutOfBounds { x: h[0], y: h[1] });
            }
        }
        if !self.holes.is_empty() && self.holes.len() != 2 {
            errs.push(MapError::HoleCount(self.holes.len()));
        }

        let mut seen = HashSet::new();
        for c in self.occupied_cells() {
            if !seen.insert((c[0], c[1])) {
                errs.push(MapError::Overlap { x: c[0], y: c[1] });
            }
        }

        if self.cats.is_empty() {
            errs.push(MapError::NoCat);
        }
        if self.rats.is_empty() {
            errs.push(MapError::NoRat);
        }
        if self.tomatoes.is_empty() {
            errs.push(MapError::NoTomato);
        }
        errs
    }
}

/// Cells a sprite starting at `start` can walk to.
fn reachable_cells(
    map: &Map,
    start: Cell,
    block_tomatoes: bool,
) -> HashSet<Cell> {
    // shortcut: ignores moving actors and treats spikes as traversable, when player-aware pathfinding is needed.
    let mut blocked: HashSet<Cell> = map.boxes.iter().copied().collect();
    if block_tomatoes {
        blocked.extend(map.tomatoes.iter().copied());
    }

    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(start);
    queue.push_back(start);
    while let Some(cell) = queue.pop_front() {
        for next in [
            [cell[0], cell[1] - 1],
            [cell[0], cell[1] + 1],
            [cell[0] - 1, cell[1]],
            [cell[0] + 1, cell[1]],
        ] {
            if Map::in_bounds(next[0], next[1])
                && !blocked.contains(&next)
                && visited.insert(next)
            {
                queue.push_back(next);
            }
        }

        if map.holes.len() == 2 {
            for (index, hole) in map.holes.iter().enumerate() {
                let Some(right) = hole[0].checked_add(1) else {
                    continue;
                };
                if cell == *hole || cell == [right, hole[1]] {
                    let other = map.holes[1 - index];
                    let Some(other_right) = other[0].checked_add(1) else {
                        continue;
                    };
                    for next in [other, [other_right, other[1]]] {
                        if !blocked.contains(&next) && visited.insert(next) {
                            queue.push_back(next);
                        }
                    }
                }
            }
        }
    }
    visited
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP_1: &str = include_str!("../../../maps/map_1.txt");

    #[test]
    fn map_1_loads_unchanged() {
        let m = Map::from_text(MAP_1).unwrap();
        assert_eq!(
            (m.boxes.len(), m.cats.len(), m.rats.len(), m.tomatoes.len()),
            (38, 4, 6, 6)
        );
        assert!(m.holes.is_empty() && m.spikes.is_empty());
    }

    #[test]
    fn text_round_trip() {
        let m = Map::from_text(MAP_1).unwrap();
        assert_eq!(Map::from_text(&m.to_text()).unwrap(), m);
    }

    #[test]
    fn text_round_trip_keeps_holes_and_spikes() {
        let m = Map {
            boxes: vec![[9, 8], [1, 1]],
            cats: vec![[15, 8], [0, 9]],
            rats: vec![[12, 2], [3, 7], [18, 1]],
            tomatoes: vec![[6, 4], [12, 7]],
            holes: vec![[18, 9], [3, 6]],
            spikes: vec![[17, 9]],
        };
        assert_eq!(
            Map::from_text(&m.to_text()).unwrap().to_text(),
            m.to_text()
        );
    }

    #[test]
    fn text_rejects_bad_grids() {
        let short_line = ".".repeat(COLS as usize - 1);
        let ok_line = ".".repeat(COLS as usize);
        let mut lines = vec![ok_line.clone(); ROWS as usize];
        lines[0] = short_line;
        assert!(matches!(
            Map::from_text(&lines.join("\n")),
            Err(MapError::Text(_))
        ));

        lines[0] = ok_line.clone();
        lines[1].replace_range(0..1, "X");
        assert!(matches!(
            Map::from_text(&lines.join("\n")),
            Err(MapError::Text(_))
        ));

        lines[1] = ok_line;
        lines[2].replace_range(0..1, "[");
        assert!(matches!(
            Map::from_text(&lines.join("\n")),
            Err(MapError::Text(_))
        ));
    }

    #[test]
    fn rejects_overlap() {
        let m = Map {
            cats: vec![[0, 0]],
            boxes: vec![[0, 0]],
            rats: vec![[1, 0]],
            tomatoes: vec![[2, 0]],
            ..Map::default()
        };
        assert!(m.validate().contains(&MapError::Overlap { x: 0, y: 0 }));
    }

    #[test]
    fn rejects_out_of_bounds_and_hole_count() {
        let m = Map {
            cats: vec![[20, 0]],
            rats: vec![[1, 0]],
            tomatoes: vec![[2, 0]],
            holes: vec![[3, 3]],
            ..Map::default()
        };
        let errs = m.validate();
        assert!(errs.contains(&MapError::OutOfBounds { x: 20, y: 0 }));
        assert!(errs.contains(&MapError::HoleCount(1)));
    }

    #[test]
    fn requires_cat_rat_and_tomato() {
        let errs = Map::default().validate();
        assert!(
            errs.contains(&MapError::NoCat)
                && errs.contains(&MapError::NoRat)
                && errs.contains(&MapError::NoTomato)
        );
    }

    #[test]
    fn warns_when_a_cat_is_sealed_away_from_rats() {
        let map = Map {
            cats: vec![[5, 5]],
            rats: vec![[9, 5]],
            tomatoes: vec![[10, 5]],
            boxes: vec![[4, 5], [6, 5], [5, 4], [5, 6]],
            ..Map::default()
        };

        assert_eq!(
            map.sealed_in_warnings(),
            vec!["Warning: a cat is sealed away from all rats."]
        );
    }

    #[test]
    fn warns_when_one_tomato_is_unreachable() {
        let map = Map {
            cats: vec![[18, 8]],
            rats: vec![[0, 0]],
            tomatoes: vec![[1, 0], [9, 9]],
            boxes: vec![[8, 9], [10, 9], [9, 8]],
            ..Map::default()
        };

        assert_eq!(
            map.sealed_in_warnings(),
            vec!["Warning: a tomato is sealed away from all rats."]
        );
    }

    #[test]
    fn rejects_hole_that_would_overflow_when_read() {
        let map = Map {
            cats: vec![[0, 0]],
            rats: vec![[1, 0]],
            tomatoes: vec![[2, 0]],
            holes: vec![[i32::MAX, 0]],
            ..Map::default()
        };

        assert!(
            map.validate()
                .contains(&MapError::OutOfBounds { x: i32::MAX, y: 0 })
        );
    }
}
