//! Remappable cat and rat direction keys.

use cat_vs_rat_core::Dir;
use macroquad::prelude::KeyCode;

/// Which player a binding belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Cat,
    Rat,
}

impl Side {
    pub const ALL: [Side; 2] = [Side::Cat, Side::Rat];

    pub fn label(self) -> &'static str {
        match self {
            Side::Cat => "Cats",
            Side::Rat => "Rats",
        }
    }
}

/// Keys that can be bound from the Controls screen. Keeping the list explicit
/// lets storage hold readable names and rejects keys the font cannot label.
const BINDABLE_KEYS: &[KeyCode] = &[
    KeyCode::Space,
    KeyCode::Apostrophe,
    KeyCode::Comma,
    KeyCode::Minus,
    KeyCode::Period,
    KeyCode::Slash,
    KeyCode::Semicolon,
    KeyCode::Equal,
    KeyCode::LeftBracket,
    KeyCode::Backslash,
    KeyCode::RightBracket,
    KeyCode::GraveAccent,
    KeyCode::Key0,
    KeyCode::Key1,
    KeyCode::Key2,
    KeyCode::Key3,
    KeyCode::Key4,
    KeyCode::Key5,
    KeyCode::Key6,
    KeyCode::Key7,
    KeyCode::Key8,
    KeyCode::Key9,
    KeyCode::A,
    KeyCode::B,
    KeyCode::C,
    KeyCode::D,
    KeyCode::E,
    KeyCode::F,
    KeyCode::G,
    KeyCode::H,
    KeyCode::I,
    KeyCode::J,
    KeyCode::K,
    KeyCode::L,
    KeyCode::M,
    KeyCode::N,
    KeyCode::O,
    KeyCode::R,
    KeyCode::S,
    KeyCode::T,
    KeyCode::U,
    KeyCode::V,
    KeyCode::W,
    KeyCode::X,
    KeyCode::Y,
    KeyCode::Z,
    KeyCode::Insert,
    KeyCode::Delete,
    KeyCode::Home,
    KeyCode::End,
    KeyCode::PageUp,
    KeyCode::PageDown,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Left,
    KeyCode::Right,
    KeyCode::Kp0,
    KeyCode::Kp1,
    KeyCode::Kp2,
    KeyCode::Kp3,
    KeyCode::Kp4,
    KeyCode::Kp5,
    KeyCode::Kp6,
    KeyCode::Kp7,
    KeyCode::Kp8,
    KeyCode::Kp9,
    KeyCode::KpDecimal,
    KeyCode::KpDivide,
    KeyCode::KpMultiply,
    KeyCode::KpSubtract,
    KeyCode::KpAdd,
    KeyCode::KpEnter,
    KeyCode::KpEqual,
    KeyCode::F1,
    KeyCode::F2,
    KeyCode::F3,
    KeyCode::F4,
    KeyCode::F5,
    KeyCode::F6,
    KeyCode::F7,
    KeyCode::F8,
    KeyCode::F9,
    KeyCode::F10,
    KeyCode::F11,
    KeyCode::F12,
];

/// One key per direction, in `Dir::index()` order: Up, Down, Left, Right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyMap {
    cat: [KeyCode; 4],
    rat: [KeyCode; 4],
}

impl Default for KeyMap {
    fn default() -> Self {
        KeyMap {
            cat: [KeyCode::Up, KeyCode::Down, KeyCode::Left, KeyCode::Right],
            rat: [KeyCode::W, KeyCode::S, KeyCode::A, KeyCode::D],
        }
    }
}

impl KeyMap {
    pub fn keys(&self, side: Side) -> &[KeyCode; 4] {
        match side {
            Side::Cat => &self.cat,
            Side::Rat => &self.rat,
        }
    }

    pub fn key(&self, side: Side, dir: Dir) -> KeyCode {
        self.keys(side)[dir.index()]
    }

    fn set(&mut self, side: Side, dir: Dir, key: KeyCode) {
        match side {
            Side::Cat => self.cat[dir.index()] = key,
            Side::Rat => self.rat[dir.index()] = key,
        }
    }

    /// Key currently bound to one of the eight actions, if any.
    pub fn owner(&self, key: KeyCode) -> Option<(Side, Dir)> {
        for side in Side::ALL {
            for dir in Dir::ALL {
                if self.key(side, dir) == key {
                    return Some((side, dir));
                }
            }
        }
        None
    }

    /// Bind `key`, swapping with the action that already owns it so every
    /// action keeps a unique key.
    pub fn bind(&mut self, side: Side, dir: Dir, key: KeyCode) {
        let old = self.key(side, dir);
        if let Some((other_side, other_dir)) = self.owner(key) {
            self.set(other_side, other_dir, old);
        }
        self.set(side, dir, key);
    }

    pub fn reset(&mut self) {
        *self = KeyMap::default();
    }

    /// Keys used by pause, dialogs, menu navigation, and the round hotkeys
    /// stay available.
    pub fn reserved(key: KeyCode) -> bool {
        matches!(
            key,
            KeyCode::Escape
                | KeyCode::Enter
                | KeyCode::Tab
                | KeyCode::Backspace
                | KeyCode::P
                | KeyCode::Q
        )
    }

    pub fn label(key: KeyCode) -> String {
        let name = format!("{key:?}");
        if let Some(digit) = name.strip_prefix("Key") {
            debug_assert!(!digit.is_empty());
            if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() {
                return digit.to_string();
            }
        }
        if let Some(rest) = name.strip_prefix("Kp") {
            return format!("Num {rest}");
        }
        name
    }

    /// Readable, stable storage form: `cat;rat` with four comma-separated
    /// key names each.
    pub fn to_storage(self) -> String {
        let side = |keys: &[KeyCode; 4]| {
            keys.iter()
                .map(|key| format!("{key:?}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        format!("{};{}", side(&self.cat), side(&self.rat))
    }

    pub fn from_storage(text: &str) -> Option<KeyMap> {
        let (cat, rat) = text.split_once(';')?;
        let keymap = KeyMap {
            cat: parse_side(cat)?,
            rat: parse_side(rat)?,
        };
        (keymap.has_unique_keys() && keymap.avoids_reserved()).then_some(keymap)
    }

    fn avoids_reserved(&self) -> bool {
        self.cat
            .iter()
            .chain(self.rat.iter())
            .all(|key| !KeyMap::reserved(*key))
    }

    fn has_unique_keys(&self) -> bool {
        let keys = [
            self.cat[0],
            self.cat[1],
            self.cat[2],
            self.cat[3],
            self.rat[0],
            self.rat[1],
            self.rat[2],
            self.rat[3],
        ];
        keys.iter()
            .enumerate()
            .all(|(index, key)| !keys[..index].contains(key))
    }
}

fn parse_side(text: &str) -> Option<[KeyCode; 4]> {
    let mut keys = [KeyCode::Space; 4];
    let mut count = 0;
    for name in text.split(',') {
        if count == keys.len() {
            return None;
        }
        keys[count] = key_from_name(name)?;
        count += 1;
    }
    if count == keys.len() {
        Some(keys)
    } else {
        None
    }
}

fn key_from_name(name: &str) -> Option<KeyCode> {
    BINDABLE_KEYS
        .iter()
        .copied()
        .find(|key| format!("{key:?}") == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_documented_keys() {
        let map = KeyMap::default();
        assert_eq!(
            map.keys(Side::Cat),
            &[KeyCode::Up, KeyCode::Down, KeyCode::Left, KeyCode::Right]
        );
        assert_eq!(
            map.keys(Side::Rat),
            &[KeyCode::W, KeyCode::S, KeyCode::A, KeyCode::D]
        );
    }

    #[test]
    fn storage_round_trips() {
        let map = KeyMap {
            cat: [KeyCode::Key0, KeyCode::F7, KeyCode::Kp3, KeyCode::Space],
            rat: [
                KeyCode::Comma,
                KeyCode::A,
                KeyCode::Left,
                KeyCode::GraveAccent,
            ],
        };
        assert_eq!(KeyMap::from_storage(&map.to_storage()), Some(map));
    }

    #[test]
    fn bad_storage_is_rejected() {
        assert_eq!(KeyMap::from_storage(""), None);
        assert_eq!(KeyMap::from_storage("Up,Down,Left;W,S,A,D"), None);
        assert_eq!(KeyMap::from_storage("Up,Down,Left,Right;W,S,A"), None);
        assert_eq!(KeyMap::from_storage("Up,Down,Left,Right;W,S,A,Nope"), None);
        assert_eq!(
            KeyMap::from_storage("Up,Down,Left,Right,Space;W,S,A,D"),
            None
        );
        assert_eq!(KeyMap::from_storage("Up,Up,Left,Right;W,S,A,D"), None);
        assert_eq!(KeyMap::from_storage("Up,Down,Left,Right;W,S,A,Up"), None);
        // P and Q are round hotkeys, so stored bindings may not use them.
        assert_eq!(KeyMap::from_storage("P,Down,Left,Right;W,S,A,D"), None);
        assert_eq!(KeyMap::from_storage("Up,Down,Left,Right;W,S,A,Q"), None);
    }

    #[test]
    fn binding_a_taken_key_swaps_owners() {
        let mut map = KeyMap::default();
        map.bind(Side::Cat, Dir::Up, KeyCode::W);
        assert_eq!(map.key(Side::Cat, Dir::Up), KeyCode::W);
        assert_eq!(map.key(Side::Rat, Dir::Up), KeyCode::Up);
        assert_eq!(map.owner(KeyCode::W), Some((Side::Cat, Dir::Up)));
    }

    #[test]
    fn binding_own_key_is_a_no_op() {
        let mut map = KeyMap::default();
        map.bind(Side::Cat, Dir::Up, KeyCode::Up);
        assert_eq!(map, KeyMap::default());
    }

    #[test]
    fn labels_are_short_and_readable() {
        assert_eq!(KeyMap::label(KeyCode::Key4), "4");
        assert_eq!(KeyMap::label(KeyCode::Kp7), "Num 7");
        assert_eq!(KeyMap::label(KeyCode::Up), "Up");
        assert_eq!(KeyMap::label(KeyCode::Space), "Space");
    }

    #[test]
    fn reserved_keys_cover_menu_controls() {
        for key in [
            KeyCode::Escape,
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::Backspace,
            KeyCode::P,
            KeyCode::Q,
        ] {
            assert!(KeyMap::reserved(key));
        }
        assert!(!KeyMap::reserved(KeyCode::Space));
    }
}
