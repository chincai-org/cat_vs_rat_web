//! Logical 1280x640 canvas scaled to fit the window (letterboxed), plus drawing helpers.

use std::cell::{Cell, RefCell};

use crate::assets::{Art, Assets};
use cat_vs_rat_core::{Game, Map};
use macroquad::prelude::*;

pub const W: f32 = 1280.0;
pub const H: f32 = 640.0;

/// Draw order for sprites at the same depth. Spikes are floor decals, so they
/// come first and actors cover them when they share a cell.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SpriteKind {
    Spike,
    Tomato,
    Box,
    RatDead,
    RatAlive,
    Cat,
}

#[derive(Clone, Copy)]
struct Sprite {
    depth: f32,
    kind: SpriteKind,
    x: f32,
    y: f32,
}

thread_local! {
    /// Reused each frame so drawing does not allocate once it has warmed up.
    static SPRITE_SCRATCH: RefCell<Vec<Sprite>> = const { RefCell::new(Vec::new()) };
}

pub struct View {
    scale: f32,
    ox: f32,
    oy: f32,
    hovered_item: Cell<bool>,
}

impl View {
    pub fn new() -> View {
        // A minimized window reports 0; keep the scale positive so nothing divides by it.
        let scale = (screen_width() / W).min(screen_height() / H).max(1e-4);
        View {
            scale,
            ox: (screen_width() - W * scale) / 2.0,
            oy: (screen_height() - H * scale) / 2.0,
            hovered_item: Cell::new(false),
        }
    }

    /// Mouse position in logical coordinates.
    pub fn mouse(&self) -> Vec2 {
        debug_assert!(self.scale > 0.0);
        let (mx, my) = mouse_position();
        vec2((mx - self.ox) / self.scale, (my - self.oy) / self.scale)
    }

    pub fn hovered_rect(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        enabled: bool,
    ) -> bool {
        let m = self.mouse();
        let hovered =
            enabled && m.x >= x && m.x <= x + w && m.y >= y && m.y <= y + h;
        if hovered {
            self.hovered_item.set(true);
        }
        hovered
    }

    pub fn has_hovered_item(&self) -> bool {
        self.hovered_item.get()
    }

    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32, c: Color) {
        draw_rectangle(
            self.ox + x * self.scale,
            self.oy + y * self.scale,
            w * self.scale,
            h * self.scale,
            c,
        );
    }

    pub fn rect_lines(&self, x: f32, y: f32, w: f32, h: f32, t: f32, c: Color) {
        draw_rectangle_lines(
            self.ox + x * self.scale,
            self.oy + y * self.scale,
            w * self.scale,
            h * self.scale,
            t * self.scale,
            c,
        );
    }

    pub fn line(&self, x1: f32, y1: f32, x2: f32, y2: f32, t: f32, c: Color) {
        draw_line(
            self.ox + x1 * self.scale,
            self.oy + y1 * self.scale,
            self.ox + x2 * self.scale,
            self.oy + y2 * self.scale,
            t * self.scale,
            c,
        );
    }

    pub fn tex(&self, t: &Texture2D, x: f32, y: f32, w: f32, h: f32) {
        self.tex_tinted(t, x, y, w, h, WHITE);
    }

    /// Draws a sprite with its visible art centred in the given box.
    pub fn art(&self, a: &Art, x: f32, y: f32, w: f32, h: f32) {
        let dx = (0.5 - a.center.x) * w;
        let dy = (0.5 - a.center.y) * h;
        self.tex(&a.texture, x + dx, y + dy, w, h);
    }

    pub fn tex_tinted(
        &self,
        t: &Texture2D,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        tint: Color,
    ) {
        draw_texture_ex(
            t,
            self.ox + x * self.scale,
            self.oy + y * self.scale,
            tint,
            DrawTextureParams {
                dest_size: Some(vec2(w * self.scale, h * self.scale)),
                ..Default::default()
            },
        );
    }

    fn px(&self, size: u16) -> u16 {
        (size as f32 * self.scale).round().max(1.0) as u16
    }

    /// `y` is the text baseline.
    pub fn text(
        &self,
        font: &Font,
        s: &str,
        x: f32,
        y: f32,
        size: u16,
        c: Color,
    ) {
        draw_text_ex(
            s,
            self.ox + x * self.scale,
            self.oy + y * self.scale,
            TextParams {
                font: Some(font),
                font_size: self.px(size),
                color: c,
                ..Default::default()
            },
        );
    }

    pub fn text_width(&self, font: &Font, s: &str, size: u16) -> f32 {
        debug_assert!(self.scale > 0.0);
        measure_text(s, Some(font), self.px(size), 1.0).width / self.scale
    }

    pub fn text_centered(
        &self,
        font: &Font,
        s: &str,
        cx: f32,
        y: f32,
        size: u16,
        c: Color,
    ) {
        let w = self.text_width(font, s, size);
        self.text(font, s, cx - w / 2.0, y, size, c);
    }

    /// Draws a button and returns true on the frame it is clicked.
    #[allow(clippy::too_many_arguments)]
    pub fn button(
        &self,
        font: &Font,
        label: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        enabled: bool,
    ) -> bool {
        let hot = self.hovered_rect(x, y, w, h, enabled);
        let fill = if !enabled {
            Color::new(0.35, 0.35, 0.35, 0.9)
        } else if hot {
            Color::new(0.97, 0.78, 0.35, 1.0)
        } else {
            Color::new(0.85, 0.60, 0.20, 1.0)
        };
        self.rect(x, y, w, h, fill);
        self.rect_lines(x, y, w, h, 2.0, BLACK);
        self.text_centered(
            font,
            label,
            x + w / 2.0,
            y + h / 2.0 + 8.0,
            22,
            BLACK,
        );
        hot && is_mouse_button_pressed(MouseButton::Left)
    }
}

/// Background tiles: Bg.png is 256x128, so 5x5 tiles cover the 20x10 board.
pub fn draw_board_bg(v: &View, a: &Assets, cell: f32, ox: f32, oy: f32) {
    for tx in 0..5 {
        for ty in 0..5 {
            v.tex(
                &a.bg,
                ox + tx as f32 * 4.0 * cell,
                oy + ty as f32 * 2.0 * cell,
                4.0 * cell,
                2.0 * cell,
            );
        }
    }
}

/// Draw projected sprites back-to-front by their ground position.
pub fn draw_game(v: &View, a: &Assets, g: &Game, cell: f32, ox: f32, oy: f32) {
    draw_board_bg(v, a, cell, ox, oy);
    for hole in &g.holes {
        v.art(
            &a.hole,
            ox + hole[0] as f32 * cell,
            oy + hole[1] as f32 * cell,
            2.0 * cell,
            cell,
        );
    }

    let cat_facing = g.cat_facing().index();
    let rat_facing = g.rat_facing().index();
    SPRITE_SCRATCH.with(|scratch| {
        let mut sprites = scratch.borrow_mut();
        sprites.clear();
        for cat in &g.cats {
            sprites.push(Sprite {
                depth: cat.y + 1.0,
                kind: SpriteKind::Cat,
                x: cat.x,
                y: cat.y,
            });
        }
        for rat in &g.rats {
            let kind = if rat.alive {
                SpriteKind::RatAlive
            } else {
                SpriteKind::RatDead
            };
            sprites.push(Sprite {
                depth: rat.y + 1.0,
                kind,
                x: rat.x,
                y: rat.y,
            });
        }
        for &[x, y] in &g.boxes {
            sprites.push(Sprite {
                depth: y as f32 + 1.0,
                kind: SpriteKind::Box,
                x: x as f32,
                y: y as f32,
            });
        }
        for &[x, y] in &g.tomatoes {
            sprites.push(Sprite {
                depth: y as f32 + 1.0,
                kind: SpriteKind::Tomato,
                x: x as f32,
                y: y as f32,
            });
        }
        for &[x, y] in &g.spikes {
            sprites.push(Sprite {
                // Half a row up: still in front of the row above, but under an
                // actor standing in or over the spike cell.
                depth: y as f32 + 0.5,
                kind: SpriteKind::Spike,
                x: x as f32,
                y: y as f32,
            });
        }
        sprites.sort_by(|left, right| {
            left.depth
                .total_cmp(&right.depth)
                .then(left.kind.cmp(&right.kind))
        });

        let spike = if g.spikes_active {
            &a.spike_active
        } else {
            &a.spike
        };
        for sprite in sprites.iter() {
            let texture = match sprite.kind {
                SpriteKind::Cat => &a.cat[cat_facing],
                SpriteKind::RatAlive => &a.rat[rat_facing],
                SpriteKind::RatDead => &a.rat_dead,
                SpriteKind::Box => &a.boxx,
                SpriteKind::Tomato => &a.tomato,
                SpriteKind::Spike => spike,
            };
            v.art(
                texture,
                ox + sprite.x * cell,
                oy + sprite.y * cell,
                cell,
                cell,
            );
        }
    });
}

/// Editor view of a map: everything facing down, spikes shown in their active art.
pub fn draw_map(v: &View, a: &Assets, map: &Map, cell: f32, ox: f32, oy: f32) {
    for h in &map.holes {
        v.art(
            &a.hole,
            ox + h[0] as f32 * cell,
            oy + h[1] as f32 * cell,
            2.0 * cell,
            cell,
        );
    }
    for c in &map.cats {
        v.art(
            &a.cat[1],
            ox + c[0] as f32 * cell,
            oy + c[1] as f32 * cell,
            cell,
            cell,
        );
    }
    for r in &map.rats {
        v.art(
            &a.rat[1],
            ox + r[0] as f32 * cell,
            oy + r[1] as f32 * cell,
            cell,
            cell,
        );
    }
    for b in &map.boxes {
        v.art(
            &a.boxx,
            ox + b[0] as f32 * cell,
            oy + b[1] as f32 * cell,
            cell,
            cell,
        );
    }
    for t in &map.tomatoes {
        v.art(
            &a.tomato,
            ox + t[0] as f32 * cell,
            oy + t[1] as f32 * cell,
            cell,
            cell,
        );
    }
    for s in &map.spikes {
        v.art(
            &a.spike_active,
            ox + s[0] as f32 * cell,
            oy + s[1] as f32 * cell,
            cell,
            cell,
        );
    }
}
