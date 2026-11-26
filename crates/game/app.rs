//! Screen state machine: Menu, MapList, Editor, Countdown, Playing, Over.

use crate::assets::Assets;
use crate::editor_ui::{EditorAction, EditorScreen};
use crate::keymap::{KeyMap, Side};
use crate::store;
use crate::view::{H, View, W, draw_board_bg, draw_game};
use cat_vs_rat_core::{Dir, FIXED_DT, Game, Input, Map, Outcome, share};
use macroquad::audio::{
    PlaySoundParams, Sound, load_sound, play_sound, play_sound_once,
};
use macroquad::experimental::coroutines::{Coroutine, start_coroutine};
use macroquad::prelude::*;

const BUILTIN_MAP: &str = include_str!("../../maps/map_1.txt");
const PER_PAGE: usize = 6;
/// Map-list row actions share one width and pitch so every button lines up.
const ROW_BUTTON_X: f32 = 500.0;
const ROW_BUTTON_W: f32 = 120.0;
const ROW_BUTTON_GAP: f32 = 4.0;

fn row_button_x(slot: usize) -> f32 {
    ROW_BUTTON_X + slot as f32 * (ROW_BUTTON_W + ROW_BUTTON_GAP)
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
unsafe extern "C" {
    fn cat_vs_rat_copy_share_link(token: *const u8, len: usize);
    fn cat_vs_rat_copy_map(text: *const u8, len: usize);
    fn cat_vs_rat_paste_map();
}

fn copy_share_link(token: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: The JavaScript import copies the token before returning.
        unsafe { cat_vs_rat_copy_share_link(token.as_ptr(), token.len()) };
    }
    #[cfg(not(target_arch = "wasm32"))]
    macroquad::miniquad::window::clipboard_set(token);
}

fn copy_map(text: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: The JavaScript import copies the text before returning.
        unsafe { cat_vs_rat_copy_map(text.as_ptr(), text.len()) };
    }
    #[cfg(not(target_arch = "wasm32"))]
    macroquad::miniquad::window::clipboard_set(text);
}

fn request_paste_map() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: The JavaScript import has no arguments and stores its result for Rust to read.
        unsafe { cat_vs_rat_paste_map() };
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        macroquad::miniquad::window::clipboard_get()
    }
}

fn toolbar_button(v: &View, a: &Assets, label: &str, x: f32, y: f32) -> bool {
    let hovered = v.hovered_rect(x, y, 320.0, 102.0, true);
    let texture = if hovered {
        &a.toolbar_hover
    } else {
        &a.toolbar
    };
    v.tex(texture, x, y, 320.0, 102.0);
    v.text_centered(&a.font, label, x + 160.0, y + 60.0, 30, RED);
    hovered && is_mouse_button_pressed(MouseButton::Left)
}

/// Where Q (or leaving a test play) goes after a round.
enum Back {
    List,
    Editor(Box<EditorScreen>),
}

struct Round {
    game: Game,
    map: Map,
    back: Back,
    acc: f32,
    started_at: f64,
}

struct ListState {
    offset: usize,
    message: Option<String>,
    confirm: Option<String>,
    rename: Option<(String, String)>,
    names: Vec<String>,
}

impl ListState {
    fn new() -> Self {
        ListState {
            offset: 0,
            message: None,
            confirm: None,
            rename: None,
            names: store::list_maps(),
        }
    }
}

/// Which action is waiting for a new key, plus feedback for the last change.
struct ControlsState {
    listening: Option<(Side, Dir)>,
    message: Option<String>,
}

enum Screen {
    Menu,
    Controls(ControlsState),
    MapList(ListState),
    Editor(EditorScreen),
    Countdown(Round),
    Playing(Round),
    Paused(Round),
    Over(Round, Outcome),
}

pub struct App {
    assets: Assets,
    screen: Screen,
    cursor_pointer: bool,
    /// Loaded only after a user gesture on web so the menu does not wait for
    /// or compete with 850 KB of audio.
    music: Option<Sound>,
    music_pending: Option<Coroutine<Option<Sound>>>,
    music_requested: bool,
    music_started: bool,
    music_failed: bool,
    /// Small countdown beep, loaded after the first frame.
    countdown_pending: Option<Coroutine<Option<Sound>>>,
    debug: bool,
    keymap: KeyMap,
    /// A map that arrived through a URL or share link.
    shared: Option<Map>,
    /// Mixed into the clock so two rounds started in the same millisecond differ.
    seed_counter: u64,
}

fn new_round(a: &Assets, map: Map, back: Back, seed: u64) -> Round {
    if let Some(s) = &a.countdown {
        play_sound_once(s);
    }
    Round {
        game: Game::new(&map, seed),
        map,
        back,
        acc: 0.0,
        started_at: get_time(),
    }
}

async fn load_music() -> Option<Sound> {
    load_sound("Assets/Music.ogg").await.ok()
}

async fn load_countdown() -> Option<Sound> {
    load_sound("Assets/countdown.ogg").await.ok()
}

/// Order inside each array matches Dir::index(): Up, Down, Left, Right.
fn read_input(keymap: &KeyMap) -> Input {
    let keys = |k: &[KeyCode; 4]| {
        [
            is_key_down(k[0]),
            is_key_down(k[1]),
            is_key_down(k[2]),
            is_key_down(k[3]),
        ]
    };
    Input {
        cat: keys(keymap.keys(Side::Cat)),
        rat: keys(keymap.keys(Side::Rat)),
    }
}

fn map_from_reference(reference: &str) -> Option<Map> {
    // Share tokens are already capped; direct map text needs a limit too.
    const MAX_REFERENCE_LEN: usize = 16 * 1024;
    if reference.len() > MAX_REFERENCE_LEN {
        return None;
    }
    if ["1", "builtin", "map_1", "Map 1 (built-in)"]
        .iter()
        .any(|name| reference.eq_ignore_ascii_case(name))
    {
        return Map::from_text(BUILTIN_MAP).ok();
    }

    Map::from_text(reference)
        .ok()
        .or_else(|| store::load_map(reference))
        .or_else(|| share::map_from_token(reference).ok())
}

fn draw_round_hud(v: &View, a: &Assets, round: &Round, debug: bool) {
    let attrition = if round.game.cats.is_empty() {
        "NO CATS LEFT".to_string()
    } else {
        format!(
            "CAT LOST IN: {}s",
            round.game.cat_attrition_remaining().ceil() as u32
        )
    };
    v.text(&a.font, &attrition, 18.0, 32.0, 20, WHITE);

    if debug {
        let alive_rats = round.game.rats.iter().filter(|rat| rat.alive).count();
        let details = format!(
            "DEBUG t={:.1}s cats={} rats={} tomatoes={}",
            round.game.time,
            round.game.cats.len(),
            alive_rats,
            round.game.tomatoes.len()
        );
        v.rect(8.0, 46.0, 420.0, 30.0, Color::new(0.0, 0.0, 0.0, 0.55));
        v.text(&a.font, &details, 18.0, 68.0, 16, WHITE);
    }
}

fn dir_label(dir: Dir) -> &'static str {
    match dir {
        Dir::Up => "Move up",
        Dir::Down => "Move down",
        Dir::Left => "Move left",
        Dir::Right => "Move right",
    }
}

/// One player's binding column. Returns the action whose Change button was
/// clicked.
fn draw_binding_panel(
    v: &View,
    a: &Assets,
    keymap: &KeyMap,
    side: Side,
    x: f32,
    listening: Option<(Side, Dir)>,
    enabled: bool,
) -> Option<(Side, Dir)> {
    const PANEL_W: f32 = 390.0;
    v.rect(x, 165.0, PANEL_W, 330.0, Color::new(0.0, 0.0, 0.0, 0.35));
    v.rect_lines(x, 165.0, PANEL_W, 330.0, 2.0, WHITE);
    v.text(&a.font, side.label(), x + 18.0, 205.0, 30, YELLOW);

    let mut clicked = None;
    for dir in Dir::ALL {
        let y = 220.0 + dir.index() as f32 * 62.0;
        let is_listening = listening == Some((side, dir));
        v.text(&a.font, dir_label(dir), x + 18.0, y + 30.0, 22, WHITE);

        let badge = if is_listening {
            Color::new(0.97, 0.78, 0.35, 1.0)
        } else {
            WHITE
        };
        v.rect(x + 130.0, y, 130.0, 44.0, badge);
        v.rect_lines(x + 130.0, y, 130.0, 44.0, 2.0, BLACK);
        let key_text = if is_listening {
            "Press a key".to_string()
        } else {
            KeyMap::label(keymap.key(side, dir))
        };
        v.text_centered(&a.font, &key_text, x + 195.0, y + 29.0, 22, BLACK);

        if v.button(
            &a.font,
            "Change",
            x + 272.0,
            y,
            100.0,
            44.0,
            enabled && !is_listening,
        ) {
            clicked = Some((side, dir));
        }
    }
    clicked
}

impl App {
    pub fn new(assets: Assets) -> App {
        let url_map = store::take_url_map();
        let incoming = store::take_incoming();
        App {
            assets,
            screen: Screen::Menu,
            cursor_pointer: false,
            music: None,
            music_pending: None,
            music_requested: cfg!(not(target_arch = "wasm32")),
            music_started: false,
            music_failed: false,
            countdown_pending: Some(start_coroutine(load_countdown())),
            shared: url_map
                .and_then(|reference| map_from_reference(reference.trim()))
                .or_else(|| {
                    incoming.and_then(|token| {
                        share::map_from_token(token.trim()).ok()
                    })
                }),
            debug: store::debug_enabled(),
            keymap: store::load_keymap(),
            seed_counter: 0,
        }
    }

    fn next_seed(&mut self) -> u64 {
        self.seed_counter = self.seed_counter.wrapping_add(1);
        let now_ms = (macroquad::miniquad::date::now() * 1000.0) as u64;
        now_ms ^ self.seed_counter.wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    /// Collect the countdown beep once its background load finishes.
    fn poll_countdown(&mut self) {
        if self.countdown_pending.as_ref().is_some_and(|c| c.is_done()) {
            if let Some(pending) = self.countdown_pending.take() {
                self.assets.countdown = pending.retrieve().flatten();
            }
        }
    }

    /// Start the music download after the browser's first user gesture
    /// (autoplay policy), then play it once decoding finishes.
    fn poll_music(&mut self) {
        if !self.music_requested || self.music_started || self.music_failed {
            return;
        }
        if self.music_pending.is_none() && self.music.is_none() {
            self.music_pending = Some(start_coroutine(load_music()));
        }
        if self.music_pending.as_ref().is_some_and(|c| c.is_done()) {
            if let Some(pending) = self.music_pending.take() {
                self.music = pending.retrieve().flatten();
                if self.music.is_none() {
                    self.music_failed = true;
                    return;
                }
            }
        }
        if let Some(music) = &self.music {
            play_sound(
                music,
                PlaySoundParams {
                    looped: true,
                    volume: 0.5,
                },
            );
            self.music_started = true;
        }
    }

    pub fn frame(&mut self, v: &View) {
        // macroquad queues typed characters until they are read. Only the
        // rename dialog consumes them; drop the rest so the queue stays bounded.
        let renaming = matches!(
            self.screen,
            Screen::MapList(ListState {
                rename: Some(_),
                ..
            })
        );
        if !renaming {
            while get_char_pressed().is_some() {}
        }

        // Web audio stays suspended until input; start music on the first user gesture.
        if !self.music_requested
            && (is_any_key_down() || is_mouse_button_pressed(MouseButton::Left))
        {
            self.music_requested = true;
        }
        self.poll_countdown();
        self.poll_music();
        // A paste request can resolve late; never let it replace a live round.
        let accepts_import = matches!(
            self.screen,
            Screen::Menu | Screen::MapList(_) | Screen::Editor(_)
        );
        let imported = if accepts_import {
            store::take_import_map()
        } else {
            None
        };
        let screen = match imported {
            Some(text) => match Map::from_text(&text) {
                Ok(map) => Screen::Editor(EditorScreen::imported(map)),
                Err(error) => {
                    let current =
                        std::mem::replace(&mut self.screen, Screen::Menu);
                    match current {
                        Screen::MapList(mut state) => {
                            state.message =
                                Some(format!("Map import failed: {error}"));
                            Screen::MapList(state)
                        }
                        other => other,
                    }
                }
            },
            None => std::mem::replace(&mut self.screen, Screen::Menu),
        };
        self.screen = self.run(screen, v);
        let pointer = v.has_hovered_item();
        if pointer != self.cursor_pointer {
            let icon = if pointer {
                macroquad::miniquad::CursorIcon::Pointer
            } else {
                macroquad::miniquad::CursorIcon::Default
            };
            macroquad::miniquad::window::set_mouse_cursor(icon);
            self.cursor_pointer = pointer;
        }
    }

    fn run(&mut self, screen: Screen, v: &View) -> Screen {
        let seed = self.next_seed();
        match screen {
            Screen::Menu => self.menu(v),
            Screen::Controls(st) => self.controls(v, st),
            Screen::MapList(st) => self.map_list(v, st),
            Screen::Editor(mut ed) => match ed.frame(v, &self.assets) {
                EditorAction::Stay => Screen::Editor(ed),
                EditorAction::Back => Screen::MapList(ListState::new()),
                EditorAction::Test(map) => Screen::Countdown(new_round(
                    &self.assets,
                    map,
                    Back::Editor(Box::new(ed)),
                    seed,
                )),
            },
            Screen::Countdown(round) => self.countdown(v, round),
            Screen::Playing(round) => self.playing(v, round),
            Screen::Paused(round) => self.paused(v, round),
            Screen::Over(round, outcome) => self.over(v, round, outcome),
        }
    }

    fn menu(&mut self, v: &View) -> Screen {
        let seed = self.next_seed();
        let a = &self.assets;
        draw_board_bg(v, a, 64.0, 0.0, 0.0);
        v.rect(340.0, 60.0, 600.0, 520.0, Color::new(0.0, 0.0, 0.0, 0.55));
        v.text_centered(&a.font, "Cat vs Rat", W / 2.0, 150.0, 80, WHITE);

        let mut next = Screen::Menu;
        if v.button(&a.font, "Play", 480.0, 190.0, 320.0, 56.0, true) {
            next = Screen::MapList(ListState::new());
        }
        if v.button(&a.font, "Map editor", 480.0, 260.0, 320.0, 56.0, true) {
            next = Screen::Editor(EditorScreen::new(Map::default(), None));
        }
        if v.button(&a.font, "Controls", 480.0, 330.0, 320.0, 56.0, true) {
            next = Screen::Controls(ControlsState {
                listening: None,
                message: None,
            });
        }

        let mut dismiss = false;
        if let Some(shared) = &self.shared {
            v.text_centered(
                &a.font,
                "A map was loaded from the URL",
                W / 2.0,
                420.0,
                26,
                YELLOW,
            );
            if v.button(&a.font, "Play it", 400.0, 440.0, 150.0, 44.0, true) {
                next = Screen::Countdown(new_round(
                    a,
                    shared.clone(),
                    Back::List,
                    seed,
                ));
                dismiss = true;
            }
            if v.button(&a.font, "Edit a copy", 565.0, 440.0, 150.0, 44.0, true)
            {
                next = Screen::Editor(EditorScreen::new(shared.clone(), None));
                dismiss = true;
            }
            if v.button(&a.font, "Dismiss", 730.0, 440.0, 150.0, 44.0, true) {
                dismiss = true;
            }
        }
        if dismiss {
            self.shared = None;
        }

        let hint = format!(
            "Cats: {} {} {} {}     Rats: {} {} {} {}",
            KeyMap::label(self.keymap.key(Side::Cat, Dir::Up)),
            KeyMap::label(self.keymap.key(Side::Cat, Dir::Down)),
            KeyMap::label(self.keymap.key(Side::Cat, Dir::Left)),
            KeyMap::label(self.keymap.key(Side::Cat, Dir::Right)),
            KeyMap::label(self.keymap.key(Side::Rat, Dir::Up)),
            KeyMap::label(self.keymap.key(Side::Rat, Dir::Down)),
            KeyMap::label(self.keymap.key(Side::Rat, Dir::Left)),
            KeyMap::label(self.keymap.key(Side::Rat, Dir::Right)),
        );
        v.text_centered(&a.font, &hint, W / 2.0, 550.0, 22, WHITE);
        next
    }

    fn controls(&mut self, v: &View, mut st: ControlsState) -> Screen {
        let a = &self.assets;
        draw_board_bg(v, a, 64.0, 0.0, 0.0);
        v.rect(200.0, 40.0, 880.0, 560.0, Color::new(0.0, 0.0, 0.0, 0.72));
        v.text_centered(&a.font, "Controls", W / 2.0, 105.0, 54, WHITE);
        v.text_centered(
            &a.font,
            "Click Change, then press a key. Esc cancels.",
            W / 2.0,
            145.0,
            22,
            WHITE,
        );

        let was_listening = st.listening.is_some();
        if let Some((side, dir)) = st.listening {
            if is_key_pressed(KeyCode::Escape) {
                st.listening = None;
                st.message = Some("Rebinding cancelled.".to_string());
            } else if let Some(key) = get_last_key_pressed() {
                if KeyMap::reserved(key) {
                    st.message = Some(format!(
                        "{} is reserved. Press another key.",
                        KeyMap::label(key)
                    ));
                } else {
                    let old = self.keymap.key(side, dir);
                    let owner = self.keymap.owner(key);
                    self.keymap.bind(side, dir, key);
                    store::save_keymap(&self.keymap);
                    st.listening = None;
                    let label = KeyMap::label(key);
                    st.message = Some(match owner {
                        Some(owner) if owner == (side, dir) => format!(
                            "{}: {} already uses {label}.",
                            side.label(),
                            dir_label(dir)
                        ),
                        Some((other_side, other_dir)) => format!(
                            "{}: {} uses {label}; {}: {} takes {}.",
                            side.label(),
                            dir_label(dir),
                            other_side.label(),
                            dir_label(other_dir),
                            KeyMap::label(old)
                        ),
                        None => format!(
                            "{}: {} now uses {label}.",
                            side.label(),
                            dir_label(dir)
                        ),
                    });
                }
            }
        }

        let enabled = st.listening.is_none();
        let mut clicked = None;
        if let Some(hit) = draw_binding_panel(
            v,
            a,
            &self.keymap,
            Side::Cat,
            240.0,
            st.listening,
            enabled,
        ) {
            clicked = Some(hit);
        }
        if let Some(hit) = draw_binding_panel(
            v,
            a,
            &self.keymap,
            Side::Rat,
            650.0,
            st.listening,
            enabled,
        ) {
            clicked = Some(hit);
        }
        if let Some((side, dir)) = clicked {
            st.listening = Some((side, dir));
            st.message = None;
        }

        if let Some(message) = &st.message {
            v.text_centered(&a.font, message, W / 2.0, 520.0, 22, YELLOW);
        }

        if v.button(
            &a.font,
            "Reset defaults",
            330.0,
            535.0,
            200.0,
            48.0,
            enabled,
        ) {
            self.keymap.reset();
            store::save_keymap(&self.keymap);
            st.message = Some("Default keys restored.".to_string());
        }
        if v.button(&a.font, "Back", 750.0, 535.0, 200.0, 48.0, enabled)
            || (is_key_pressed(KeyCode::Escape) && !was_listening)
        {
            return Screen::Menu;
        }

        Screen::Controls(st)
    }

    fn map_list(&mut self, v: &View, mut st: ListState) -> Screen {
        let a = &self.assets;
        draw_board_bg(v, a, 64.0, 0.0, 0.0);
        v.rect(30.0, 30.0, 1220.0, 580.0, Color::new(0.0, 0.0, 0.0, 0.6));
        v.text(&a.font, "Choose a map", 100.0, 90.0, 44, WHITE);
        if let Some(message) = &st.message {
            v.text(&a.font, message, 100.0, 125.0, 22, YELLOW);
        }

        let seed = self.next_seed();
        let (mut next, mut refresh) = self.map_list_rows(v, &mut st, seed);
        if let Some(action) = self.map_list_actions(v, &mut st) {
            next = Some(action);
        }
        refresh |= self.rename_dialog(v, &mut st);
        if refresh {
            st.names = store::list_maps();
        }
        next.unwrap_or(Screen::MapList(st))
    }

    fn map_list_rows(
        &self,
        v: &View,
        st: &mut ListState,
        seed: u64,
    ) -> (Option<Screen>, bool) {
        let a = &self.assets;
        let customs = st.names.clone();
        let total = 1 + customs.len();
        if st.offset >= total {
            st.offset = 0;
        }

        let mut next: Option<Screen> = None;
        let mut refresh = false;
        // While the rename dialog is open, the list behind it must not react.
        let modal = st.rename.is_some();
        for row in 0..PER_PAGE {
            let idx = st.offset + row;
            if idx >= total {
                break;
            }
            let y = 150.0 + row as f32 * 62.0;
            let name = if idx == 0 {
                "Map 1 (built-in)".to_string()
            } else {
                customs[idx - 1].clone()
            };
            v.text(&a.font, &name, 100.0, y + 30.0, 28, WHITE);

            let load = || {
                if idx == 0 {
                    Map::from_text(BUILTIN_MAP).ok()
                } else {
                    store::load_map(&customs[idx - 1])
                }
            };
            if v.button(
                &a.font,
                "Play",
                row_button_x(0),
                y,
                ROW_BUTTON_W,
                44.0,
                !modal,
            ) {
                if let Some(map) = load() {
                    next = Some(Screen::Countdown(new_round(
                        a,
                        map,
                        Back::List,
                        seed,
                    )));
                }
            }
            let edit_label = if idx == 0 { "Clone" } else { "Edit" };
            if v.button(
                &a.font,
                edit_label,
                row_button_x(1),
                y,
                ROW_BUTTON_W,
                44.0,
                !modal,
            ) {
                if let Some(map) = load() {
                    let saved_name = if idx == 0 {
                        None
                    } else {
                        Some(customs[idx - 1].clone())
                    };
                    next = Some(Screen::Editor(EditorScreen::new(
                        map, saved_name,
                    )));
                }
            }
            if v.button(
                &a.font,
                "Share",
                row_button_x(2),
                y,
                ROW_BUTTON_W,
                44.0,
                !modal,
            ) {
                if let Some(map) = load() {
                    let token = share::token_from_map(&map);
                    copy_share_link(&token);
                    #[cfg(target_arch = "wasm32")]
                    {
                        st.message = Some("Copying share link...".to_string());
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        st.message = Some(
                            "Share token copied to clipboard.".to_string(),
                        );
                    }
                }
            }
            if v.button(
                &a.font,
                "Copy map",
                row_button_x(3),
                y,
                ROW_BUTTON_W,
                44.0,
                !modal,
            ) {
                if let Some(map) = load() {
                    copy_map(&map.to_text());
                    #[cfg(target_arch = "wasm32")]
                    {
                        st.message = Some("Copying map text...".to_string());
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        st.message =
                            Some("Map text copied to clipboard.".to_string());
                    }
                }
            }
            if idx > 0 {
                let this = customs[idx - 1].as_str();
                if v.button(
                    &a.font,
                    "Rename",
                    row_button_x(4),
                    y,
                    ROW_BUTTON_W,
                    44.0,
                    !modal,
                ) {
                    st.rename = Some((this.to_string(), this.to_string()));
                    st.confirm = None;
                }
                let sure = st.confirm.as_deref() == Some(this);
                let label = if sure { "Sure?" } else { "Delete" };
                if v.button(
                    &a.font,
                    label,
                    row_button_x(5),
                    y,
                    ROW_BUTTON_W,
                    44.0,
                    !modal,
                ) {
                    if sure {
                        store::delete_map(this);
                        st.confirm = None;
                        st.message = Some("Deleted.".to_string());
                        refresh = true;
                    } else {
                        st.confirm = Some(this.to_string());
                    }
                }
            }
        }
        (next, refresh)
    }

    fn map_list_actions(&self, v: &View, st: &mut ListState) -> Option<Screen> {
        let a = &self.assets;
        let total = 1 + st.names.len();
        let modal = st.rename.is_some();
        let mut next = None;
        if v.button(&a.font, "New map", 100.0, 550.0, 170.0, 44.0, !modal) {
            next =
                Some(Screen::Editor(EditorScreen::new(Map::default(), None)));
        }
        if v.button(&a.font, "Paste map", 290.0, 550.0, 180.0, 44.0, !modal) {
            match request_paste_map() {
                Some(text) => match Map::from_text(&text) {
                    Ok(map) => {
                        next = Some(Screen::Editor(EditorScreen::imported(map)))
                    }
                    Err(error) => {
                        st.message = Some(format!("Map import failed: {error}"))
                    }
                },
                None => {
                    #[cfg(target_arch = "wasm32")]
                    {
                        st.message =
                            Some("Reading map from clipboard...".to_string());
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        st.message = Some(
                            "Clipboard is empty or unavailable.".to_string(),
                        );
                    }
                }
            }
        }
        if v.button(
            &a.font,
            "Prev",
            700.0,
            550.0,
            90.0,
            44.0,
            !modal && st.offset > 0,
        ) {
            st.offset = st.offset.saturating_sub(PER_PAGE);
        }
        if v.button(
            &a.font,
            "Next",
            800.0,
            550.0,
            90.0,
            44.0,
            !modal && st.offset + PER_PAGE < total,
        ) {
            st.offset += PER_PAGE;
        }
        if v.button(&a.font, "Back", 1000.0, 550.0, 100.0, 44.0, !modal) {
            next = Some(Screen::Menu);
        }
        next
    }

    /// Handles and draws the rename dialog. Returns true when the list changed.
    fn rename_dialog(&self, v: &View, st: &mut ListState) -> bool {
        let a = &self.assets;
        let mut refresh = false;
        if let Some((old_name, mut value)) = st.rename.take() {
            while let Some(ch) = get_char_pressed() {
                if (ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_'))
                    && value.chars().count() < 32
                {
                    value.push(ch);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                value.pop();
            }
            if is_key_pressed(KeyCode::Escape) {
                st.message = Some("Rename cancelled.".to_string());
            } else if is_key_pressed(KeyCode::Enter) {
                match store::rename_map(&old_name, &value) {
                    Ok(()) => {
                        st.message = Some("Map renamed.".to_string());
                        st.confirm = None;
                        refresh = true;
                    }
                    Err(error) => {
                        st.message = Some(error.to_string());
                        st.rename = Some((old_name.clone(), value.clone()));
                    }
                }
            } else {
                st.rename = Some((old_name, value));
            }
        }
        if let Some((_, value)) = &st.rename {
            v.rect(350.0, 205.0, 580.0, 220.0, Color::new(0.0, 0.0, 0.0, 0.94));
            v.rect_lines(350.0, 205.0, 580.0, 220.0, 3.0, WHITE);
            v.text_centered(&a.font, "Rename map", W / 2.0, 255.0, 36, WHITE);
            v.rect(410.0, 275.0, 460.0, 54.0, WHITE);
            v.text(&a.font, value, 425.0, 312.0, 26, BLACK);
            v.text_centered(
                &a.font,
                "Type name, Enter to save, Esc to cancel",
                W / 2.0,
                380.0,
                20,
                WHITE,
            );
        }
        refresh
    }

    fn countdown(&mut self, v: &View, round: Round) -> Screen {
        let a = &self.assets;
        let t = (get_time() - round.started_at) as f32;
        draw_game(v, a, &round.game, 64.0, 0.0, 0.0);
        draw_round_hud(v, a, &round, self.debug);
        let label = if t < 3.0 {
            format!("{}", 3 - t as i32)
        } else {
            "GO!".to_string()
        };
        v.text_centered(&a.font, &label, W / 2.0, H / 2.0 + 70.0, 200, BLACK);
        if t >= 4.0 {
            Screen::Playing(round)
        } else {
            Screen::Countdown(round)
        }
    }

    fn playing(&mut self, v: &View, mut round: Round) -> Screen {
        if is_key_pressed(KeyCode::Escape) {
            return Screen::Paused(round);
        }
        let input = read_input(&self.keymap);
        round.acc += get_frame_time().min(0.1);
        let mut outcome = None;
        while round.acc >= FIXED_DT && outcome.is_none() {
            outcome = round.game.step(&input);
            round.acc -= FIXED_DT;
        }
        draw_game(v, &self.assets, &round.game, 64.0, 0.0, 0.0);
        draw_round_hud(v, &self.assets, &round, self.debug);
        match outcome {
            Some(o) => Screen::Over(round, o),
            None => Screen::Playing(round),
        }
    }

    fn paused(&mut self, v: &View, round: Round) -> Screen {
        let a = &self.assets;
        draw_game(v, a, &round.game, 64.0, 0.0, 0.0);
        v.rect(0.0, 0.0, W, H, Color::new(0.0, 0.0, 0.0, 0.68));
        draw_round_hud(v, a, &round, self.debug);
        v.text_centered(&a.font, "PAUSED", W / 2.0, 250.0, 64, WHITE);

        if toolbar_button(v, a, "RESUME (ESC)", 180.0, 270.0)
            || is_key_pressed(KeyCode::Escape)
        {
            return Screen::Playing(round);
        }
        if toolbar_button(v, a, "MENU (Q)", 780.0, 270.0)
            || is_key_pressed(KeyCode::Q)
        {
            return match round.back {
                Back::List => Screen::MapList(ListState::new()),
                Back::Editor(ed) => Screen::Editor(*ed),
            };
        }
        Screen::Paused(round)
    }

    fn over(&mut self, v: &View, round: Round, outcome: Outcome) -> Screen {
        let seed = self.next_seed();
        let a = &self.assets;
        draw_game(v, a, &round.game, 64.0, 0.0, 0.0);
        v.rect(0.0, 0.0, W, H, Color::new(0.0, 0.0, 0.0, 0.68));
        draw_round_hud(v, a, &round, self.debug);
        let msg = match outcome {
            Outcome::CatWin => "CAT WIN!",
            Outcome::RatWin => "RAT WIN!",
            Outcome::Draw => "DRAW!",
        };
        v.text_centered(&a.font, msg, W / 2.0, 160.0, 72, RED);
        if toolbar_button(v, a, "PLAY AGAIN (P)", 180.0, 270.0)
            || is_key_pressed(KeyCode::P)
        {
            return Screen::Countdown(new_round(
                a, round.map, round.back, seed,
            ));
        }
        if toolbar_button(v, a, "MENU (Q)", 780.0, 270.0)
            || is_key_pressed(KeyCode::Q)
        {
            return match round.back {
                Back::List => Screen::MapList(ListState::new()),
                Back::Editor(ed) => Screen::Editor(*ed),
            };
        }
        Screen::Over(round, outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_url_reference_accepts_builtin_text_and_share_tokens() {
        let builtin = Map::from_text(BUILTIN_MAP).unwrap();
        assert_eq!(map_from_reference("1"), Some(builtin.clone()));
        assert_eq!(
            map_from_reference(&builtin.to_text()),
            Some(builtin.clone())
        );
        let token = share::token_from_map(&builtin);
        assert_eq!(map_from_reference(&token), Some(builtin));
    }
}
