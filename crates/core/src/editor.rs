//! Map editor logic (no UI): placement rules, strokes, undo/redo.

use crate::map::{COLS, Cell, Map};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Cat,
    Rat,
    Box,
    Tomato,
    Spike,
    Hole,
    Eraser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceError {
    OutOfBounds,
    Occupied,
    TooManyHoles,
    HoleNeedsTwoCells,
}

impl fmt::Display for PlaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            PlaceError::OutOfBounds => "That cell is outside the board.",
            PlaceError::Occupied => "That cell is already used.",
            PlaceError::TooManyHoles => "A map can have at most 2 holes.",
            PlaceError::HoleNeedsTwoCells => {
                "A hole needs two free cells side by side."
            }
        };
        write!(f, "{s}")
    }
}

/// Oldest history entries are dropped so a long session stays bounded.
const MAX_UNDO: usize = 100;

#[derive(Debug, Clone, Default)]
pub struct Editor {
    pub map: Map,
    /// True when the map differs from the last saved state.
    pub dirty: bool,
    /// Bumped whenever `map` changes, so callers can cache derived data.
    pub revision: u64,
    undo: Vec<Map>,
    redo: Vec<Map>,
    stroke_base: Option<Map>,
}

impl Editor {
    pub fn new(map: Map) -> Self {
        Editor {
            map,
            ..Default::default()
        }
    }

    pub fn can_place(
        &self,
        tool: Tool,
        x: i32,
        y: i32,
    ) -> Result<(), PlaceError> {
        if !Map::in_bounds(x, y) {
            return Err(PlaceError::OutOfBounds);
        }
        match tool {
            Tool::Eraser => Ok(()),
            Tool::Hole => {
                if self.map.holes.len() >= 2 {
                    return Err(PlaceError::TooManyHoles);
                }
                if x > COLS - 2 {
                    return Err(PlaceError::HoleNeedsTwoCells);
                }
                if self.map.is_free(x, y) && self.map.is_free(x + 1, y) {
                    Ok(())
                } else {
                    Err(PlaceError::Occupied)
                }
            }
            _ => {
                if self.map.is_free(x, y) {
                    Ok(())
                } else {
                    Err(PlaceError::Occupied)
                }
            }
        }
    }

    pub fn apply(
        &mut self,
        tool: Tool,
        x: i32,
        y: i32,
    ) -> Result<(), PlaceError> {
        self.can_place(tool, x, y)?;
        let cell: Cell = [x, y];
        match tool {
            Tool::Cat => self.map.cats.push(cell),
            Tool::Rat => self.map.rats.push(cell),
            Tool::Box => self.map.boxes.push(cell),
            Tool::Tomato => self.map.tomatoes.push(cell),
            Tool::Spike => self.map.spikes.push(cell),
            Tool::Hole => self.map.holes.push(cell),
            Tool::Eraser => self.erase(x, y),
        }
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    fn erase(&mut self, x: i32, y: i32) {
        for list in [
            &mut self.map.boxes,
            &mut self.map.cats,
            &mut self.map.rats,
            &mut self.map.tomatoes,
            &mut self.map.spikes,
        ] {
            list.retain(|c| !(c[0] == x && c[1] == y));
        }
        // Erasing either half of a hole removes the whole hole.
        self.map.holes.retain(|h| {
            let right = h[0].checked_add(1);
            !(h[1] == y && (h[0] == x || right == Some(x)))
        });
    }

    /// Start a mouse stroke: everything painted until `end_stroke` is one undo step.
    pub fn begin_stroke(&mut self) {
        self.stroke_base = Some(self.map.clone());
    }

    fn push_undo(&mut self, map: Map) {
        self.undo.push(map);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    pub fn end_stroke(&mut self) {
        if let Some(base) = self.stroke_base.take() {
            if base != self.map {
                self.push_undo(base);
                self.redo.clear();
                self.dirty = true;
            }
        }
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(prev) => {
                self.redo.push(std::mem::replace(&mut self.map, prev));
                self.dirty = true;
                self.revision = self.revision.wrapping_add(1);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(next) => {
                let previous = std::mem::replace(&mut self.map, next);
                self.push_undo(previous);
                self.dirty = true;
                self.revision = self.revision.wrapping_add(1);
                true
            }
            None => false,
        }
    }

    pub fn clear(&mut self) {
        if self.map != Map::default() {
            let previous = std::mem::take(&mut self.map);
            self.push_undo(previous);
            self.redo.clear();
            self.dirty = true;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hole_needs_two_free_cells_and_erases_as_one() {
        let mut e = Editor::new(Map::default());
        assert_eq!(
            e.apply(Tool::Hole, 19, 0),
            Err(PlaceError::HoleNeedsTwoCells)
        );
        e.apply(Tool::Hole, 3, 3).unwrap();
        assert_eq!(e.apply(Tool::Box, 4, 3), Err(PlaceError::Occupied));
        e.apply(Tool::Eraser, 4, 3).unwrap();
        assert!(e.map.holes.is_empty());
    }

    #[test]
    fn at_most_two_holes() {
        let mut e = Editor::new(Map::default());
        e.apply(Tool::Hole, 0, 0).unwrap();
        e.apply(Tool::Hole, 3, 0).unwrap();
        assert_eq!(e.apply(Tool::Hole, 6, 0), Err(PlaceError::TooManyHoles));
    }

    #[test]
    fn one_object_per_cell() {
        let mut e = Editor::new(Map::default());
        e.apply(Tool::Cat, 2, 2).unwrap();
        assert_eq!(e.apply(Tool::Tomato, 2, 2), Err(PlaceError::Occupied));
        assert_eq!(e.apply(Tool::Box, 20, 2), Err(PlaceError::OutOfBounds));
    }

    #[test]
    fn a_stroke_is_one_undo_step() {
        let mut e = Editor::new(Map::default());
        e.begin_stroke();
        e.apply(Tool::Box, 0, 0).unwrap();
        e.apply(Tool::Box, 1, 0).unwrap();
        e.end_stroke();
        assert_eq!(e.map.boxes.len(), 2);
        assert!(e.undo());
        assert!(e.map.boxes.is_empty());
        assert!(e.redo());
        assert_eq!(e.map.boxes.len(), 2);
    }
}
