//! Map editor screen: grid, tool bar, hotkeys, test play, save.

use crate::assets::Assets;
use crate::store;
use crate::view::{H, View, W, draw_board_bg, draw_map};
use cat_vs_rat_core::{COLS, Editor, Map, ROWS, Tool};
use macroquad::prelude::*;

const CELL: f32 = 48.0;
const GX: f32 = 16.0;
const GY: f32 = 100.0;
const PANEL_X: f32 = 1000.0;

pub enum EditorAction {
    Stay,
    Back,
    Test(Map),
}

pub struct EditorScreen {
    editor: Editor,
    /// None until the map is first saved.
    name: Option<String>,
    tool: Tool,
    message: Option<(String, f64)>,
    /// Tool of the stroke in progress (mouse button held).
    stroke: Option<Tool>,
    last_cell: Option<(i32, i32)>,
    confirm_back: bool,
    /// Cached reachability warnings; recomputed only when the map changes.
    warnings: Vec<&'static str>,
    warnings_revision: u64,
}

fn cell_at(m: Vec2) -> Option<(i32, i32)> {
    if m.x < GX || m.y < GY {
        return None;
    }
    let cx = ((m.x - GX) / CELL).floor() as i32;
    let cy = ((m.y - GY) / CELL).floor() as i32;
    if Map::in_bounds(cx, cy) {
        Some((cx, cy))
    } else {
        None
    }
}

impl EditorScreen {
    pub fn new(map: Map, name: Option<String>) -> Self {
        EditorScreen {
            editor: Editor::new(map),
            name,
            tool: Tool::Box,
            message: None,
            stroke: None,
            last_cell: None,
            confirm_back: false,
            warnings: Vec::new(),
            warnings_revision: u64::MAX,
        }
    }

    pub fn imported(map: Map) -> Self {
        let mut screen = Self::new(map, None);
        screen.editor.dirty = true;
        screen
    }

    fn say(&mut self, text: String) {
        self.message = Some((text, get_time() + 3.5));
    }

    fn paint(&mut self, tool: Tool, x: i32, y: i32, report: bool) {
        if self.last_cell == Some((x, y)) {
            return;
        }
        self.last_cell = Some((x, y));
        if let Err(e) = self.editor.apply(tool, x, y) {
            if report {
                self.say(e.to_string());
            }
        }
    }

    /// The map if it can be saved or played, otherwise the first problem as text.
    fn checked_map(&self) -> Result<Map, String> {
        match self.editor.map.validate().into_iter().next() {
            Some(e) => Err(e.to_string()),
            None => Ok(self.editor.map.clone()),
        }
    }

    fn save(&mut self) {
        match self.checked_map() {
            Err(e) => self.say(e),
            Ok(map) => {
                let name =
                    self.name.clone().unwrap_or_else(store::next_free_name);
                store::save_map(&name, &map);
                self.name = Some(name.clone());
                self.editor.mark_saved();
                self.say(format!("Saved as {name}."));
            }
        }
    }

    fn undo(&mut self) {
        if self.editor.undo() {
            self.confirm_back = false;
        }
    }

    fn redo(&mut self) {
        if self.editor.redo() {
            self.confirm_back = false;
        }
    }

    pub fn frame(&mut self, v: &View, a: &Assets) -> EditorAction {
        let font = &a.font;
        self.draw_board(v, a, font);
        self.handle_painting(v);
        self.hotkeys();
        self.toolbar(v, font);
        let action = self.action_buttons(v, font);

        self.refresh_warnings();
        if !self.warnings.is_empty() {
            v.text(font, &self.warnings.join(" "), GX, 604.0, 18, ORANGE);
        }
        if let Some((text, until)) = &self.message {
            if get_time() < *until {
                v.text(font, text, GX, 632.0, 24, YELLOW);
            }
        }
        action
    }

    fn draw_board(&self, v: &View, a: &Assets, font: &Font) {
        v.rect(0.0, 0.0, W, H, Color::new(0.10, 0.10, 0.14, 1.0));
        draw_board_bg(v, a, CELL, GX, GY);
        draw_map(v, a, &self.editor.map, CELL, GX, GY);
        let line_col = Color::new(0.0, 0.0, 0.0, 0.25);
        for i in 0..=COLS {
            let x = GX + i as f32 * CELL;
            v.line(x, GY, x, GY + ROWS as f32 * CELL, 1.0, line_col);
        }
        for j in 0..=ROWS {
            let y = GY + j as f32 * CELL;
            v.line(GX, y, GX + COLS as f32 * CELL, y, 1.0, line_col);
        }
        let title = format!(
            "{}{}",
            self.name.as_deref().unwrap_or("New map"),
            if self.editor.dirty { " *" } else { "" }
        );
        v.text(font, &title, GX, 62.0, 36, WHITE);
    }

    fn handle_painting(&mut self, v: &View) {
        let hover = cell_at(v.mouse());
        let left_pressed = is_mouse_button_pressed(MouseButton::Left);
        let right_pressed = is_mouse_button_pressed(MouseButton::Right);
        let any_down = is_mouse_button_down(MouseButton::Left)
            || is_mouse_button_down(MouseButton::Right);

        let stroke = self.stroke;
        match stroke {
            None => {
                if let Some((x, y)) = hover {
                    if left_pressed || right_pressed {
                        let tool = if right_pressed {
                            Tool::Eraser
                        } else {
                            self.tool
                        };
                        self.editor.begin_stroke();
                        self.stroke = Some(tool);
                        self.last_cell = None;
                        self.confirm_back = false;
                        self.paint(tool, x, y, true);
                    }
                }
            }
            Some(tool) => {
                if !any_down {
                    self.editor.end_stroke();
                    self.stroke = None;
                } else if let Some((x, y)) = hover {
                    if matches!(tool, Tool::Box | Tool::Spike | Tool::Eraser) {
                        self.paint(tool, x, y, false);
                    }
                }
            }
        }

        if let Some((x, y)) = hover {
            let ok = self.editor.can_place(self.tool, x, y).is_ok();
            let col = if ok { GREEN } else { RED };
            v.rect_lines(
                GX + x as f32 * CELL,
                GY + y as f32 * CELL,
                CELL,
                CELL,
                3.0,
                col,
            );
            if self.tool == Tool::Hole {
                v.rect_lines(
                    GX + (x + 1) as f32 * CELL,
                    GY + y as f32 * CELL,
                    CELL,
                    CELL,
                    3.0,
                    col,
                );
            }
        }
    }

    fn hotkeys(&mut self) {
        let ctrl = is_key_down(KeyCode::LeftControl)
            || is_key_down(KeyCode::RightControl)
            || is_key_down(KeyCode::LeftSuper)
            || is_key_down(KeyCode::RightSuper);
        let shift =
            is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
        if ctrl {
            if is_key_pressed(KeyCode::Z) {
                if shift {
                    self.redo();
                } else {
                    self.undo();
                }
            }
            if is_key_pressed(KeyCode::Y) {
                self.redo();
            }
            if is_key_pressed(KeyCode::S) {
                self.save();
            }
        } else {
            let keys = [
                (KeyCode::C, Tool::Cat),
                (KeyCode::R, Tool::Rat),
                (KeyCode::B, Tool::Box),
                (KeyCode::T, Tool::Tomato),
                (KeyCode::S, Tool::Spike),
                (KeyCode::H, Tool::Hole),
                (KeyCode::E, Tool::Eraser),
                (KeyCode::Delete, Tool::Eraser),
                (KeyCode::Backspace, Tool::Eraser),
            ];
            for (key, tool) in keys {
                if is_key_pressed(key) {
                    self.tool = tool;
                }
            }
        }
    }

    fn toolbar(&mut self, v: &View, font: &Font) {
        let tools = [
            (Tool::Cat, "Cat (C)"),
            (Tool::Rat, "Rat (R)"),
            (Tool::Box, "Box (B)"),
            (Tool::Tomato, "Tomato (T)"),
            (Tool::Spike, "Spike (S)"),
            (Tool::Hole, "Hole (H)"),
            (Tool::Eraser, "Eraser (E)"),
        ];
        for (i, (tool, label)) in tools.iter().enumerate() {
            let y = 70.0 + i as f32 * 42.0;
            if v.button(font, label, PANEL_X, y, 250.0, 36.0, true) {
                self.tool = *tool;
            }
            if self.tool == *tool {
                v.rect_lines(PANEL_X - 3.0, y - 3.0, 256.0, 42.0, 3.0, WHITE);
            }
        }
    }

    fn action_buttons(&mut self, v: &View, font: &Font) -> EditorAction {
        let mut action = EditorAction::Stay;
        let ay = 70.0 + 7.0 * 42.0 + 8.0;
        if v.button(font, "Test play", PANEL_X, ay, 250.0, 36.0, true) {
            match self.checked_map() {
                Ok(map) => action = EditorAction::Test(map),
                Err(e) => self.say(e),
            }
        }
        if v.button(
            font,
            "Save (Ctrl+S)",
            PANEL_X,
            ay + 42.0,
            250.0,
            36.0,
            true,
        ) {
            self.save();
        }
        if v.button(
            font,
            "Undo (Ctrl+Z)",
            PANEL_X,
            ay + 84.0,
            250.0,
            36.0,
            true,
        ) {
            self.undo();
        }
        if v.button(
            font,
            "Redo (Ctrl+Y)",
            PANEL_X,
            ay + 126.0,
            250.0,
            36.0,
            true,
        ) {
            self.redo();
        }
        if v.button(font, "Clear", PANEL_X, ay + 168.0, 250.0, 36.0, true) {
            self.editor.clear();
            self.confirm_back = false;
        }
        if v.button(font, "Back", PANEL_X, ay + 210.0, 250.0, 36.0, true) {
            if self.editor.dirty && !self.confirm_back {
                self.confirm_back = true;
                self.say(
                    "Unsaved changes. Press Back again to discard them."
                        .to_string(),
                );
            } else {
                action = EditorAction::Back;
            }
        }
        action
    }

    fn refresh_warnings(&mut self) {
        if self.warnings_revision != self.editor.revision {
            self.warnings = self.editor.map.sealed_in_warnings();
            self.warnings_revision = self.editor.revision;
        }
    }
}
