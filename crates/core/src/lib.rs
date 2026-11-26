//! Platform-independent rules for Cat vs Rat. No rendering, no I/O.

pub mod controls;
pub mod editor;
pub mod game;
pub mod map;
pub mod rng;
pub mod share;

pub use controls::{Dir, SideControl};
pub use editor::{Editor, PlaceError, Tool};
pub use game::{FIXED_DT, Game, Input, Outcome};
pub use map::{COLS, Cell, Map, MapError, ROWS};
pub use rng::Rng;
