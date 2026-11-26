//! Direction input with "last key pressed wins".

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::Up, Dir::Down, Dir::Left, Dir::Right];

    /// Index into `[bool; 4]` key arrays and texture lists: Up, Down, Left, Right.
    pub fn index(self) -> usize {
        match self {
            Dir::Up => 0,
            Dir::Down => 1,
            Dir::Left => 2,
            Dir::Right => 3,
        }
    }

    /// Screen coordinates: y grows downward.
    pub fn delta(self) -> (f32, f32) {
        match self {
            Dir::Up => (0.0, -1.0),
            Dir::Down => (0.0, 1.0),
            Dir::Left => (-1.0, 0.0),
            Dir::Right => (1.0, 0.0),
        }
    }
}

/// One player's direction keys. Keeps the held keys in press order.
#[derive(Debug, Clone)]
pub struct SideControl {
    held: Vec<Dir>,
    facing: Dir,
}

impl Default for SideControl {
    fn default() -> Self {
        SideControl {
            held: Vec::new(),
            facing: Dir::Down,
        }
    }
}

impl SideControl {
    /// Call once per simulation step with the current key state (Up, Down, Left, Right).
    pub fn update(&mut self, down: [bool; 4]) {
        for d in Dir::ALL {
            let is_down = down[d.index()];
            let is_held = self.held.contains(&d);
            if is_down && !is_held {
                self.held.push(d);
            } else if !is_down && is_held {
                self.held.retain(|&h| h != d);
            }
        }
        if let Some(&last) = self.held.last() {
            self.facing = last;
        }
    }

    /// Direction of the most recently pressed key still held, or the last facing if none.
    pub fn facing(&self) -> Dir {
        self.facing
    }

    pub fn is_moving(&self) -> bool {
        !self.held.is_empty()
    }

    pub fn clear(&mut self) {
        self.held.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_pressed_wins_and_falls_back_on_release() {
        let mut c = SideControl::default();
        c.update([false, false, true, false]); // Left
        assert_eq!(c.facing(), Dir::Left);
        c.update([true, false, true, false]); // Left + Up
        assert_eq!(c.facing(), Dir::Up);
        c.update([false, false, true, false]); // release Up
        assert_eq!(c.facing(), Dir::Left);
        assert!(c.is_moving());
        c.update([false; 4]);
        assert!(!c.is_moving());
        assert_eq!(c.facing(), Dir::Left);
    }

    #[test]
    fn pressing_the_opposite_key_turns_immediately() {
        let mut c = SideControl::default();
        c.update([false, false, true, false]); // Left
        c.update([false, false, true, true]); // Left + Right
        assert_eq!(c.facing(), Dir::Right);
    }
}
