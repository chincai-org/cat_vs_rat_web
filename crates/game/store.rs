//! Custom maps, clipboard hand-off, and URL parameters, kept in the browser's
//! localStorage (a local file on desktop).

use crate::keymap::KeyMap;
use cat_vs_rat_core::Map;
use quad_storage::STORAGE;

const MAP_PREFIX: &str = "catvsrat.map.";
const KEY_KEYMAP: &str = "catvsrat.keymap";
/// index.html writes a token from a #m=... link here before the game starts.
const KEY_INCOMING: &str = "catvsrat.incoming";
const KEY_URL_MAP: &str = "catvsrat.url_map";
const KEY_IMPORT_MAP: &str = "catvsrat.import_map";
const KEY_DEBUG: &str = "catvsrat.debug";
const KEY_VOLUME: &str = "catvsrat.volume";
const KEY_MUTED: &str = "catvsrat.muted";

pub fn list_maps() -> Vec<String> {
    let s = STORAGE.lock().unwrap();
    let mut names: Vec<String> = (0..s.len())
        .filter_map(|i| s.key(i))
        .filter_map(|k| k.strip_prefix(MAP_PREFIX).map(str::to_string))
        .filter(|name| {
            s.get(&format!("{MAP_PREFIX}{name}"))
                .is_some_and(|text| Map::from_text(&text).is_ok())
        })
        .collect();
    names.sort();
    names
}

pub fn load_map(name: &str) -> Option<Map> {
    let s = STORAGE.lock().unwrap();
    s.get(&format!("{MAP_PREFIX}{name}"))
        .and_then(|text| Map::from_text(&text).ok())
}

pub fn save_map(name: &str, map: &Map) {
    STORAGE
        .lock()
        .unwrap()
        .set(&format!("{MAP_PREFIX}{name}"), &map.to_text());
}

pub fn rename_map(old_name: &str, new_name: &str) -> Result<(), &'static str> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err("Map name cannot be empty.");
    }
    if new_name.len() > 32 {
        return Err("Map name must be 32 characters or fewer.");
    }
    if old_name == new_name {
        return Ok(());
    }

    let old_key = format!("{MAP_PREFIX}{old_name}");
    let new_key = format!("{MAP_PREFIX}{new_name}");
    let mut s = STORAGE.lock().unwrap();
    let Some(text) = s.get(&old_key) else {
        return Err("Map no longer exists.");
    };
    if s.get(&new_key).is_some() {
        return Err("A map with that name already exists.");
    }
    s.set(&new_key, &text);
    s.remove(&old_key);
    Ok(())
}

pub fn delete_map(name: &str) {
    STORAGE
        .lock()
        .unwrap()
        .remove(&format!("{MAP_PREFIX}{name}"));
}

pub fn next_free_name() -> String {
    let existing = list_maps();
    (1..)
        .map(|n| format!("Custom {n}"))
        .find(|n| !existing.contains(n))
        .unwrap()
}

/// Returns and clears a token that arrived through a share link.
fn take_value(key: &str) -> Option<String> {
    let mut s = STORAGE.lock().unwrap();
    let value = s.get(key);
    if value.is_some() {
        s.remove(key);
    }
    value
}

/// Returns and clears a token that arrived through a share link.
pub fn take_incoming() -> Option<String> {
    take_value(KEY_INCOMING)
}

/// Returns and clears a map reference supplied with `?map=`.
pub fn take_url_map() -> Option<String> {
    take_value(KEY_URL_MAP)
}

/// Returns and clears map text supplied by the asynchronous clipboard read.
pub fn take_import_map() -> Option<String> {
    take_value(KEY_IMPORT_MAP)
}

pub fn debug_enabled() -> bool {
    STORAGE.lock().unwrap().get(KEY_DEBUG).as_deref() == Some("1")
}

/// Music volume in 0..=1 plus the mute toggle.
pub fn load_audio() -> (f32, bool) {
    let s = STORAGE.lock().unwrap();
    let volume = s
        .get(KEY_VOLUME)
        .and_then(|text| text.parse::<f32>().ok())
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);
    let muted = s.get(KEY_MUTED).as_deref() == Some("1");
    (volume, muted)
}

pub fn save_audio(volume: f32, muted: bool) {
    let mut s = STORAGE.lock().unwrap();
    s.set(KEY_VOLUME, &format!("{volume:.2}"));
    s.set(KEY_MUTED, if muted { "1" } else { "0" });
}

pub fn load_keymap() -> KeyMap {
    STORAGE
        .lock()
        .unwrap()
        .get(KEY_KEYMAP)
        .and_then(|text| KeyMap::from_storage(&text))
        .unwrap_or_default()
}

pub fn save_keymap(keymap: &KeyMap) {
    STORAGE
        .lock()
        .unwrap()
        .set(KEY_KEYMAP, &keymap.to_storage());
}
