//! The simulation: movement, eating, teleporting, spikes, cat attrition, win checks.

use crate::controls::{Dir, SideControl};
use crate::map::{COLS, Cell, Map, ROWS};
use crate::rng::Rng;

/// The simulation advances in fixed steps of this many seconds.
pub const FIXED_DT: f32 = 1.0 / 60.0;
/// Cells per second. The original moved 0.2 / 0.4 cells per 50-67 ms tick (about 3-4 and 6-8 cells/s).
pub const CAT_SPEED: f32 = 3.5;
pub const RAT_SPEED: f32 = 7.0;

/// Original ground footprint, at the bottom of the cell. Boxes and spikes keep
/// it as a rounded ground footprint.
const FOOT_LEFT: f32 = 0.05;
const FOOT_RIGHT: f32 = 0.95;
const FOOT_TOP: f32 = 0.60;
const FOOT_BOTTOM: f32 = 1.0;
/// Cat and mouse ground footprint. The art is centred on the body, so the tail
/// never adds a collision lobe of its own.
const ACTOR_LEFT: f32 = 0.05;
const ACTOR_RIGHT: f32 = 0.95;
/// Horizontal capsule with the ground footprint.
const ACTOR_RADIUS: f32 = (FOOT_BOTTOM - FOOT_TOP) * 0.5;
const ACTOR_HALF: f32 = (ACTOR_RIGHT - ACTOR_LEFT) * 0.5 - ACTOR_RADIUS;
const ACTOR_CX: f32 = (ACTOR_LEFT + ACTOR_RIGHT) * 0.5;
const ACTOR_CY: f32 = (FOOT_TOP + FOOT_BOTTOM) * 0.5;
/// Boxes block with a slightly narrower footprint than actors so their side
/// walls feel less grabby.
const BOX_LEFT: f32 = 0.10;
const BOX_RIGHT: f32 = 0.90;
/// Tomato hitbox uses the drawn fruit's width and only its lower part, ending
/// at the drawn base instead of the cell bottom.
const TOMATO_LEFT: f32 = 0.296875;
const TOMATO_RIGHT: f32 = 0.703125;
const TOMATO_TOP: f32 = FOOT_TOP;
const TOMATO_BOTTOM: f32 = 0.6875;
/// Round every obstacle corner so a sprite slides around it instead of
/// catching on a square corner.
const CORNER: f32 = 0.08;
const TOMATO_CORNER: f32 = 0.04;
/// Spikes sit slightly higher than the shared footprint: their base stops a
/// little above the cell bottom, and their top reaches a little further up the
/// art so the hitbox matches the drawn spikes.
const SPIKE_TOP: f32 = 0.40;
const SPIKE_BOTTOM: f32 = 0.80;
const COLLISION_EPSILON: f32 = 1e-4;
/// A fully blocked move may try a small sideways nudge so it can cut around a
/// rounded corner. Straight moves never nudge past a flat wall, and the nudge
/// must recover a useful part of the blocked step.
const CORNER_NUDGE: f32 = 0.06;
const CORNER_STEP: f32 = 0.01;
const CORNER_MIN_GAIN: f32 = 1e-3;
const CORNER_MIN_RATIO: f32 = 0.1;
const TELEPORT_COOLDOWN: f32 = 2.0;
/// The original code toggles every 1 s (its README says 2 s).
const SPIKE_INTERVAL: f32 = 1.0;
const ATTRITION_INTERVAL: f32 = 30.0;

/// Ground footprint of one static cell type.
#[derive(Debug, Clone, Copy)]
struct Footprint {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    corner: f32,
}

const BOX_FOOT: Footprint = Footprint {
    left: BOX_LEFT,
    right: BOX_RIGHT,
    top: FOOT_TOP,
    bottom: FOOT_BOTTOM,
    corner: CORNER,
};

const SPIKE_FOOT: Footprint = Footprint {
    left: FOOT_LEFT,
    right: FOOT_RIGHT,
    top: SPIKE_TOP,
    bottom: SPIKE_BOTTOM,
    corner: CORNER,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    CatWin,
    RatWin,
    /// Attrition removed every cat before the rats ate every tomato.
    Draw,
}

/// Held keys for both players, each as [Up, Down, Left, Right].
#[derive(Debug, Clone, Copy, Default)]
pub struct Input {
    pub cat: [bool; 4],
    pub rat: [bool; 4],
}

#[derive(Debug, Clone)]
pub struct Cat {
    pub x: f32,
    pub y: f32,
    /// Position before the latest fixed step, for render interpolation.
    pub prev_x: f32,
    pub prev_y: f32,
    last_teleport: f32,
    /// Cleared by a teleport and set again after the sprite leaves a hole.
    teleport_armed: bool,
}

#[derive(Debug, Clone)]
pub struct Rat {
    pub x: f32,
    pub y: f32,
    /// Position before the latest fixed step, for render interpolation.
    pub prev_x: f32,
    pub prev_y: f32,
    pub alive: bool,
    died_at: f32,
    vanish_after: f32,
    last_teleport: f32,
    /// Cleared by a teleport and set again after the sprite leaves a hole.
    teleport_armed: bool,
}

/// Collision shapes. Cats and rats are horizontal capsules; tomatoes, boxes,
/// spikes, and holes are rounded rectangles.
#[derive(Debug, Clone, Copy)]
enum Collider {
    /// Axis-aligned rectangle with rounded corners. `corner` is the corner
    /// radius; the flat sides still slide. `solid_y` blocks vertical movement.
    RoundedRect {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        corner: f32,
        solid_y: bool,
    },
    /// Horizontal capsule: a segment from `cx - half` to `cx + half` swept by
    /// radius `r`.
    Capsule { cx: f32, cy: f32, r: f32, half: f32 },
}

impl Collider {
    fn actor(x: f32, y: f32) -> Self {
        Self::Capsule {
            cx: x + ACTOR_CX,
            cy: y + ACTOR_CY,
            r: ACTOR_RADIUS,
            half: ACTOR_HALF,
        }
    }

    fn object(x: f32, y: f32) -> Self {
        Self::RoundedRect {
            left: x + FOOT_LEFT,
            top: y + FOOT_TOP,
            right: x + FOOT_RIGHT,
            bottom: y + FOOT_BOTTOM,
            corner: CORNER,
            solid_y: true,
        }
    }

    /// Tomatoes use the drawn fruit's width and lower part. They block both
    /// axes, but a corner nudge lets a sprite slide off them instead of
    /// catching.
    fn tomato(cell: &Cell) -> Self {
        Self::RoundedRect {
            left: cell[0] as f32 + TOMATO_LEFT,
            top: cell[1] as f32 + TOMATO_TOP,
            right: cell[0] as f32 + TOMATO_RIGHT,
            bottom: cell[1] as f32 + TOMATO_BOTTOM,
            corner: TOMATO_CORNER,
            solid_y: true,
        }
    }

    /// The ground area of a 2x1 hole, used only to decide when a sprite's feet
    /// have reached the hole.
    fn hole(cell: &Cell) -> Self {
        Self::RoundedRect {
            left: cell[0] as f32,
            top: cell[1] as f32 + FOOT_TOP,
            right: cell[0] as f32 + 2.0,
            bottom: cell[1] as f32 + FOOT_BOTTOM,
            corner: CORNER,
            solid_y: false,
        }
    }

    fn overlaps(self, other: Self) -> bool {
        match (self, other) {
            (
                Self::Capsule { cx, cy, r, half },
                Self::Capsule {
                    cx: ox,
                    cy: oy,
                    r: or,
                    half: ohalf,
                },
            ) => {
                let dx = (cx - ox).abs() - (half + ohalf);
                let dy = cy - oy;
                let radius = r + or;
                let dx = dx.max(0.0);
                dx * dx + dy * dy < radius * radius + COLLISION_EPSILON
            }
            (
                Self::Capsule { cx, cy, r, half },
                Self::RoundedRect {
                    left,
                    top,
                    right,
                    bottom,
                    corner,
                    solid_y: _,
                },
            ) => capsule_rounded_rect_overlap(
                cx, cy, r, half, left, top, right, bottom, corner,
            ),
            (
                Self::RoundedRect {
                    left,
                    top,
                    right,
                    bottom,
                    corner,
                    solid_y: _,
                },
                Self::Capsule { cx, cy, r, half },
            ) => capsule_rounded_rect_overlap(
                cx, cy, r, half, left, top, right, bottom, corner,
            ),
            (
                Self::RoundedRect {
                    left: a_left,
                    top: a_top,
                    right: a_right,
                    bottom: a_bottom,
                    corner: a_corner,
                    solid_y: _,
                },
                Self::RoundedRect {
                    left,
                    top,
                    right,
                    bottom,
                    corner,
                    solid_y: _,
                },
            ) => rounded_rect_overlap(
                a_left, a_top, a_right, a_bottom, a_corner, left, top, right,
                bottom, corner,
            ),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn capsule_rect_overlap(
    cx: f32,
    cy: f32,
    r: f32,
    half: f32,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) -> bool {
    let dx = (left - (cx + half)).max(cx - half - right).max(0.0);
    let dy = (top - cy).max(cy - bottom).max(0.0);
    dx * dx + dy * dy < r * r + COLLISION_EPSILON
}

/// A rounded rectangle is its inset rectangle swept by a disk of `corner`
/// radius, so every test reduces to a distance test against that inset.
fn rounded_inner(
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    corner: f32,
) -> (f32, f32, f32, f32, f32) {
    let corner = corner
        .max(0.0)
        .min((right - left) * 0.5)
        .min((bottom - top) * 0.5);
    (
        left + corner,
        top + corner,
        right - corner,
        bottom - corner,
        corner,
    )
}

#[allow(clippy::too_many_arguments)]
fn capsule_rounded_rect_overlap(
    cx: f32,
    cy: f32,
    r: f32,
    half: f32,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    corner: f32,
) -> bool {
    let (inner_left, inner_top, inner_right, inner_bottom, corner) =
        rounded_inner(left, top, right, bottom, corner);
    capsule_rect_overlap(
        cx,
        cy,
        r + corner,
        half,
        inner_left,
        inner_top,
        inner_right,
        inner_bottom,
    )
}

#[allow(clippy::too_many_arguments)]
fn rounded_rect_overlap(
    a_left: f32,
    a_top: f32,
    a_right: f32,
    a_bottom: f32,
    a_corner: f32,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    corner: f32,
) -> bool {
    let (a_inner_left, a_inner_top, a_inner_right, a_inner_bottom, a_corner) =
        rounded_inner(a_left, a_top, a_right, a_bottom, a_corner);
    let (inner_left, inner_top, inner_right, inner_bottom, corner) =
        rounded_inner(left, top, right, bottom, corner);
    rect_rect_distance(
        a_inner_left,
        a_inner_top,
        a_inner_right,
        a_inner_bottom,
        inner_left,
        inner_top,
        inner_right,
        inner_bottom,
    ) < a_corner + corner + COLLISION_EPSILON
}

#[allow(clippy::too_many_arguments)]
fn rect_rect_distance(
    a_left: f32,
    a_top: f32,
    a_right: f32,
    a_bottom: f32,
    b_left: f32,
    b_top: f32,
    b_right: f32,
    b_bottom: f32,
) -> f32 {
    let dx = (b_left - a_right).max(a_left - b_right).max(0.0);
    let dy = (b_top - a_bottom).max(a_top - b_bottom).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

/// Merge every contiguous vertical run of cells into one footprint.
///
/// Two cells stacked in the same column would otherwise leave a seam between
/// their foot footprints, which a sprite could walk through. Boxes and raised
/// spikes share this rule.
fn run_colliders(cells: &[Cell], foot: Footprint) -> Vec<Collider> {
    cells
        .iter()
        .filter(|cell| !cells.contains(&[cell[0], cell[1] - 1]))
        .map(|cell| {
            let [x, mut bottom] = *cell;
            while cells.contains(&[x, bottom + 1]) {
                bottom += 1;
            }
            Collider::RoundedRect {
                left: x as f32 + foot.left,
                top: cell[1] as f32 + foot.top,
                right: x as f32 + foot.right,
                bottom: bottom as f32 + foot.bottom,
                corner: foot.corner,
                solid_y: true,
            }
        })
        .collect()
}

fn box_run_colliders(boxes: &[Cell]) -> Vec<Collider> {
    run_colliders(boxes, BOX_FOOT)
}

/// The gap between two cells that touch only at a corner.
///
/// The gap is too narrow for a sprite to fit through, but its feet can slide
/// along the diagonal. The seam is handled separately from cell colliders so
/// it only stops movement *into* the gap: it uses the edge of the cell being
/// approached, never widening either cell's side hitbox.
#[derive(Debug, Clone, Copy)]
struct Seam {
    /// Right edge of the left cell: the wall for a sprite moving left.
    left: f32,
    /// Left edge of the right cell: the wall for a sprite moving right.
    right: f32,
    top: f32,
    bottom: f32,
}

fn seams(cells: &[Cell], foot: Footprint) -> Vec<Seam> {
    let mut seams = Vec::new();
    for &cell in cells {
        for (dx, dy) in [(1, 1), (1, -1)] {
            let other = [cell[0] + dx, cell[1] + dy];
            if !cells.contains(&other) {
                continue;
            }
            let (upper, lower) = if cell[1] < other[1] {
                (cell, other)
            } else {
                (other, cell)
            };
            seams.push(Seam {
                left: cell[0] as f32 + foot.right,
                right: other[0] as f32 + foot.left,
                top: upper[1] as f32 + foot.bottom,
                bottom: lower[1] as f32 + foot.top,
            });
        }
    }
    seams
}

fn box_seams(boxes: &[Cell]) -> Vec<Seam> {
    seams(boxes, BOX_FOOT)
}

#[derive(Debug, Clone)]
pub struct Game {
    pub cats: Vec<Cat>,
    pub rats: Vec<Rat>,
    pub tomatoes: Vec<Cell>,
    pub boxes: Vec<Cell>,
    /// Precomputed because boxes never move during a round.
    box_colliders: Vec<Collider>,
    box_seams: Vec<Seam>,
    /// Precomputed because spikes never move; only their active state toggles.
    spike_colliders: Vec<Collider>,
    spike_seams: Vec<Seam>,
    pub holes: Vec<Cell>,
    pub spikes: Vec<Cell>,
    /// All spikes toggle together.
    pub spikes_active: bool,
    pub time: f32,
    cat_ctl: SideControl,
    rat_ctl: SideControl,
    spike_clock: f32,
    next_attrition: f32,
    outcome: Option<Outcome>,
    rng: Rng,
    /// Reused each step so movement does not allocate.
    order_buf: Vec<Mover>,
    blocker_buf: Vec<Collider>,
    seam_buf: Vec<Seam>,
}

#[derive(Debug, Clone, Copy)]
enum Mover {
    Cat(usize),
    Rat(usize),
}

impl Game {
    pub fn new(map: &Map, seed: u64) -> Game {
        let mut rng = Rng::new(seed);
        let rats: Vec<Rat> = map
            .rats
            .iter()
            .map(|c| Rat {
                x: c[0] as f32,
                y: c[1] as f32,
                prev_x: c[0] as f32,
                prev_y: c[1] as f32,
                alive: true,
                died_at: 0.0,
                vanish_after: rng.range_inclusive(3, 7) as f32,
                last_teleport: f32::NEG_INFINITY,
                teleport_armed: true,
            })
            .collect();
        let box_colliders = box_run_colliders(&map.boxes);
        let box_seams = box_seams(&map.boxes);
        let spike_colliders = run_colliders(&map.spikes, SPIKE_FOOT);
        let spike_seams = seams(&map.spikes, SPIKE_FOOT);
        Game {
            cats: map
                .cats
                .iter()
                .map(|c| Cat {
                    x: c[0] as f32,
                    y: c[1] as f32,
                    prev_x: c[0] as f32,
                    prev_y: c[1] as f32,
                    last_teleport: f32::NEG_INFINITY,
                    teleport_armed: true,
                })
                .collect(),
            rats,
            tomatoes: map.tomatoes.clone(),
            boxes: map.boxes.clone(),
            box_colliders,
            box_seams,
            spike_colliders,
            spike_seams,
            holes: if map.holes.len() == 2 {
                map.holes.clone()
            } else {
                Vec::new()
            },
            spikes: map.spikes.clone(),
            spikes_active: false, // the original's first frame flips them off
            time: 0.0,
            cat_ctl: SideControl::default(),
            rat_ctl: SideControl::default(),
            spike_clock: 0.0,
            next_attrition: ATTRITION_INTERVAL,
            outcome: None,
            rng,
            order_buf: Vec::new(),
            blocker_buf: Vec::new(),
            seam_buf: Vec::new(),
        }
    }

    pub fn cat_facing(&self) -> Dir {
        self.cat_ctl.facing()
    }

    pub fn rat_facing(&self) -> Dir {
        self.rat_ctl.facing()
    }

    /// Seconds until the next cat is removed by attrition.
    pub fn cat_attrition_remaining(&self) -> f32 {
        (self.next_attrition - self.time).max(0.0)
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    /// Advance one fixed step. Returns the outcome once the round is decided.
    pub fn step(&mut self, input: &Input) -> Option<Outcome> {
        if self.outcome.is_some() {
            return self.outcome;
        }
        self.time += FIXED_DT;
        self.cat_ctl.update(input.cat);
        self.rat_ctl.update(input.rat);

        // Remember the pre-step positions so the renderer can interpolate.
        for cat in &mut self.cats {
            cat.prev_x = cat.x;
            cat.prev_y = cat.y;
        }
        for rat in &mut self.rats {
            rat.prev_x = rat.x;
            rat.prev_y = rat.y;
        }

        self.move_sprites();

        // Cats resolve first, so a rat killed in this step cannot also eat a
        // tomato. A same-step cat/rat tie therefore goes to the cat.
        self.cats_eat_rats();
        self.rats_eat_tomatoes();

        let now = self.time;
        self.rats
            .retain(|r| r.alive || now - r.died_at <= r.vanish_after);

        self.spike_clock += FIXED_DT;
        if self.spike_clock > SPIKE_INTERVAL {
            self.spike_clock -= SPIKE_INTERVAL;
            self.spikes_active = !self.spikes_active;
        }

        if self.time >= self.next_attrition {
            self.next_attrition += ATTRITION_INTERVAL;
            if !self.cats.is_empty() {
                let i = self.rng.below(self.cats.len());
                self.cats.remove(i);
            }
        }

        self.outcome = if !self.rats.iter().any(|r| r.alive) {
            Some(Outcome::CatWin)
        } else if self.tomatoes.is_empty() {
            Some(Outcome::RatWin)
        } else if self.cats.is_empty() {
            // Without cats the rats can never be caught, but they still have
            // tomatoes left, so neither goal was met.
            Some(Outcome::Draw)
        } else {
            None
        };
        self.outcome
    }

    fn fill_static_blockers(&self, out: &mut Vec<Collider>) {
        out.clear();
        out.extend(self.box_colliders.iter().copied());
        if self.spikes_active {
            out.extend(self.spike_colliders.iter().copied());
        }
    }

    fn fill_static_seams(&self, out: &mut Vec<Seam>) {
        out.clear();
        out.extend(self.box_seams.iter().copied());
        if self.spikes_active {
            out.extend(self.spike_seams.iter().copied());
        }
    }

    /// Boxes, other cats, tomatoes, active spikes.
    fn fill_cat_blockers(&self, skip: usize, out: &mut Vec<Collider>) {
        self.fill_static_blockers(out);
        out.extend(
            self.cats
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != skip)
                .map(|(_, cat)| Collider::actor(cat.x, cat.y)),
        );
        out.extend(self.tomatoes.iter().map(Collider::tomato));
    }

    /// Boxes, other rats (dead ones included), active spikes.
    fn fill_rat_blockers(&self, skip: usize, out: &mut Vec<Collider>) {
        self.fill_static_blockers(out);
        out.extend(
            self.rats
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != skip)
                .map(|(_, rat)| Collider::actor(rat.x, rat.y)),
        );
    }

    /// True when a sprite's ground contact is on a raised spike base. Such a
    /// sprite cannot walk off until the spikes retract.
    fn on_raised_spike(&self, x: f32, y: f32) -> bool {
        if !self.spikes_active {
            return false;
        }
        let Collider::Capsule { cx, cy, r, half } = Collider::actor(x, y)
        else {
            return false;
        };
        let feet_overlap = |top: f32, bottom: f32| {
            cy - r < bottom - COLLISION_EPSILON
                && cy + r > top + COLLISION_EPSILON
        };
        // The capsule spans its whole footprint, including the rounded ends.
        let x_overlap = |left: f32, right: f32| {
            cx - half - r <= right && cx + half + r >= left
        };
        self.spike_colliders.iter().any(|c| {
            let Collider::RoundedRect {
                left,
                top,
                right,
                bottom,
                ..
            } = *c
            else {
                return false;
            };
            x_overlap(left, right) && feet_overlap(top, bottom)
        }) || self.spike_seams.iter().any(|s| {
            x_overlap(s.left.min(s.right), s.left.max(s.right))
                && feet_overlap(s.top, s.bottom)
        })
    }

    fn move_sprites(&mut self) {
        let mut order = std::mem::take(&mut self.order_buf);
        order.clear();
        if self.cat_ctl.is_moving() {
            order.extend((0..self.cats.len()).map(Mover::Cat));
        }
        if self.rat_ctl.is_moving() {
            order.extend((0..self.rats.len()).map(Mover::Rat));
        }
        self.rng.shuffle(&mut order);

        let mut blockers = std::mem::take(&mut self.blocker_buf);
        let mut seams = std::mem::take(&mut self.seam_buf);
        self.fill_static_seams(&mut seams);
        for m in order.drain(..) {
            let (x, y, speed, last_teleport, armed) = match m {
                Mover::Cat(i) => {
                    let cat = &self.cats[i];
                    (
                        cat.x,
                        cat.y,
                        CAT_SPEED,
                        cat.last_teleport,
                        cat.teleport_armed,
                    )
                }
                Mover::Rat(i) => {
                    let rat = &self.rats[i];
                    if !rat.alive {
                        continue;
                    }
                    (
                        rat.x,
                        rat.y,
                        RAT_SPEED,
                        rat.last_teleport,
                        rat.teleport_armed,
                    )
                }
            };
            if self.on_raised_spike(x, y) {
                continue;
            }
            let (nx, ny, teleported) = self.move_actor(
                m,
                x,
                y,
                speed,
                last_teleport,
                armed,
                &mut blockers,
                &seams,
            );
            let now = self.time;
            let on_hole_now = on_hole(&self.holes, nx, ny);
            // Cats and rats have the same motion fields; keep one update path.
            macro_rules! settle {
                ($sprite:expr) => {{
                    let sprite = $sprite;
                    sprite.x = nx;
                    sprite.y = ny;
                    sprite.teleport_armed = if teleported {
                        false
                    } else if on_hole_now {
                        armed
                    } else {
                        true
                    };
                    if teleported {
                        sprite.last_teleport = now;
                        // Do not interpolate across the teleport.
                        sprite.prev_x = nx;
                        sprite.prev_y = ny;
                    }
                }};
            }
            match m {
                Mover::Cat(i) => settle!(&mut self.cats[i]),
                Mover::Rat(i) => settle!(&mut self.rats[i]),
            }
        }
        self.order_buf = order;
        self.blocker_buf = blockers;
        self.seam_buf = seams;
    }

    /// Move one actor, then teleport it when its feet are on a hole and it has
    /// not just come out of one.
    #[allow(clippy::too_many_arguments)]
    fn move_actor(
        &self,
        mover: Mover,
        x: f32,
        y: f32,
        speed: f32,
        last_teleport: f32,
        armed: bool,
        blockers: &mut Vec<Collider>,
        seams: &[Seam],
    ) -> (f32, f32, bool) {
        debug_assert!(speed > 0.0);
        debug_assert!(x.is_finite() && y.is_finite());
        match mover {
            Mover::Cat(i) => self.fill_cat_blockers(i, blockers),
            Mover::Rat(i) => self.fill_rat_blockers(i, blockers),
        }
        let (vx, vy) = match mover {
            Mover::Cat(_) => self.cat_ctl.movement(),
            Mover::Rat(_) => self.rat_ctl.movement(),
        };
        let step = speed * FIXED_DT;
        let (mut nx, mut ny) = move_sprite_with_vector(
            x,
            y,
            vx * step,
            vy * step,
            blockers,
            seams,
        );
        let reached_hole = on_hole(&self.holes, nx, ny);
        let may_teleport = armed
            && reached_hole
            && self.time - last_teleport > TELEPORT_COOLDOWN;
        let mut teleported = false;
        if may_teleport {
            if let Some((tx, ty)) =
                teleport_target(&self.holes, nx, ny, blockers)
            {
                nx = tx;
                ny = ty;
                teleported = true;
            }
        }
        debug_assert!(nx.is_finite() && ny.is_finite());
        (nx, ny, teleported)
    }

    fn cats_eat_rats(&mut self) {
        let now = self.time;
        for cat in &self.cats {
            let cat_hitbox = Collider::actor(cat.x, cat.y);
            for rat in self.rats.iter_mut() {
                if rat.alive
                    && cat_hitbox.overlaps(Collider::actor(rat.x, rat.y))
                {
                    rat.alive = false;
                    rat.died_at = now;
                }
            }
        }
    }

    fn rats_eat_tomatoes(&mut self) {
        let rats = &self.rats;
        self.tomatoes.retain(|t| {
            let tomato_hitbox = Collider::tomato(t);
            !rats.iter().any(|r| {
                r.alive && Collider::actor(r.x, r.y).overlaps(tomato_hitbox)
            })
        });
    }
}

/// Move a sprite against full-cell blockers using its smaller ground-contact footprint.
pub fn move_sprite(
    x: f32,
    y: f32,
    dir: Dir,
    step: f32,
    blockers: &[(f32, f32)],
) -> (f32, f32) {
    let colliders: Vec<Collider> = blockers
        .iter()
        .map(|&(x, y)| Collider::object(x, y))
        .collect();
    move_sprite_with_colliders(x, y, dir, step, &colliders)
}

fn move_sprite_with_colliders(
    x: f32,
    y: f32,
    dir: Dir,
    step: f32,
    blockers: &[Collider],
) -> (f32, f32) {
    move_sprite_with_seams(x, y, dir, step, blockers, &[])
}

fn move_sprite_with_seams(
    x: f32,
    y: f32,
    dir: Dir,
    step: f32,
    blockers: &[Collider],
    seams: &[Seam],
) -> (f32, f32) {
    debug_assert!(step >= 0.0, "step must not be negative");
    debug_assert!(x >= -1e-3 && x <= (COLS - 1) as f32 + 1e-3);
    debug_assert!(y >= -1e-3 && y <= (ROWS - 1) as f32 + 1e-3);
    let free = free_distance(x, y, dir, blockers, seams);
    let distance = step.min(free).max(0.0);
    let (dx, dy) = dir.delta();
    debug_assert!(distance >= 0.0 && distance <= step);
    (x + dx * distance, y + dy * distance)
}

/// Move one frame as a 2D vector. Each axis is resolved separately, so a
/// blocked component still lets the other slide along the obstacle. If an axis
/// is blocked at a rounded corner, a small sideways nudge lets it slide past
/// instead of sticking.
fn move_sprite_with_vector(
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    blockers: &[Collider],
    seams: &[Seam],
) -> (f32, f32) {
    debug_assert!(x >= -1e-3 && x <= (COLS - 1) as f32 + 1e-3);
    debug_assert!(y >= -1e-3 && y <= (ROWS - 1) as f32 + 1e-3);
    debug_assert!(dx.is_finite() && dy.is_finite());
    let moved = axis_move(x, y, dx, dy, blockers, seams);
    let blocked_x =
        dx != 0.0 && (moved.0 - x).abs() + COLLISION_EPSILON < dx.abs();
    let blocked_y =
        dy != 0.0 && (moved.1 - y).abs() + COLLISION_EPSILON < dy.abs();

    match (blocked_x, blocked_y) {
        (false, false) => moved,
        // One axis is free: cut around the corner of the blocked axis, keeping
        // the progress the free axis already made.
        (true, false) => {
            let remaining = dx.abs() - (moved.0 - x).abs();
            let dir = if dx < 0.0 { Dir::Left } else { Dir::Right };
            cut_blocked_axis(
                moved.0,
                moved.1,
                dir,
                remaining,
                blockers,
                seams,
                [Some(Dir::Up), Some(Dir::Down)],
            )
            .unwrap_or(moved)
        }
        (false, true) => {
            let remaining = dy.abs() - (moved.1 - y).abs();
            let dir = if dy < 0.0 { Dir::Up } else { Dir::Down };
            cut_blocked_axis(
                moved.0,
                moved.1,
                dir,
                remaining,
                blockers,
                seams,
                [Some(Dir::Left), Some(Dir::Right)],
            )
            .unwrap_or(moved)
        }
        // Both axes are blocked: try each one and keep the bigger gain.
        (true, true) => {
            let mut best = moved;
            let mut best_score = (moved.0 - x) * dx + (moved.1 - y) * dy;
            for (dir, step, slides) in [
                (
                    if dy < 0.0 { Dir::Up } else { Dir::Down },
                    dy.abs(),
                    [Some(Dir::Left), Some(Dir::Right)],
                ),
                (
                    if dx < 0.0 { Dir::Left } else { Dir::Right },
                    dx.abs(),
                    [Some(Dir::Up), Some(Dir::Down)],
                ),
            ] {
                let free = free_distance(x, y, dir, blockers, seams);
                if let Some((nx, ny)) =
                    corner_cut(x, y, dir, step, blockers, seams, free, slides)
                {
                    let score = (nx - x) * dx + (ny - y) * dy;
                    if score > best_score + CORNER_MIN_GAIN {
                        best = (nx, ny);
                        best_score = score;
                    }
                }
            }
            best
        }
    }
}

/// Try to cut a single blocked axis around a corner.
fn cut_blocked_axis(
    x: f32,
    y: f32,
    dir: Dir,
    step: f32,
    blockers: &[Collider],
    seams: &[Seam],
    slides: [Option<Dir>; 2],
) -> Option<(f32, f32)> {
    if step <= COLLISION_EPSILON {
        return None;
    }
    let free = free_distance(x, y, dir, blockers, seams);
    corner_cut(x, y, dir, step, blockers, seams, free, slides)
}

fn axis_move(
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    blockers: &[Collider],
    seams: &[Seam],
) -> (f32, f32) {
    let mut x = x;
    let mut y = y;
    if dx != 0.0 {
        let dir = if dx < 0.0 { Dir::Left } else { Dir::Right };
        let (moved, _) =
            move_sprite_with_seams(x, y, dir, dx.abs(), blockers, seams);
        x = moved;
    }
    if dy != 0.0 {
        let dir = if dy < 0.0 { Dir::Up } else { Dir::Down };
        let (_, moved) =
            move_sprite_with_seams(x, y, dir, dy.abs(), blockers, seams);
        y = moved;
    }
    (x, y)
}

/// Try a small sideways nudge so a blocked axis can cut around a corner.
#[allow(clippy::too_many_arguments)]
fn corner_cut(
    x: f32,
    y: f32,
    dir: Dir,
    step: f32,
    blockers: &[Collider],
    seams: &[Seam],
    free: f32,
    slides: [Option<Dir>; 2],
) -> Option<(f32, f32)> {
    // Only rounded obstacles have a corner to cut around. Actor capsules and
    // diagonal seams stay solid, so a blocked move cannot slip past them.
    if !blocked_by_rounded_rect(x, y, dir, blockers, seams) {
        return None;
    }
    let (dx, dy) = dir.delta();
    let mut best = None;
    for slide in slides.into_iter().flatten() {
        let (sx, sy) = slide.delta();
        let mut offset = CORNER_STEP;
        while offset <= CORNER_NUDGE + COLLISION_EPSILON {
            let start_x = x + sx * offset;
            let start_y = y + sy * offset;
            if start_x < -1e-3
                || start_x > (COLS - 1) as f32 + 1e-3
                || start_y < -1e-3
                || start_y > (ROWS - 1) as f32 + 1e-3
            {
                offset += CORNER_STEP;
                continue;
            }

            let shifted_free =
                free_distance(start_x, start_y, dir, blockers, seams);
            let progress = step.min(shifted_free).max(0.0);
            if progress > free + CORNER_MIN_GAIN
                && progress >= step * CORNER_MIN_RATIO
            {
                let nx = start_x + dx * progress;
                let ny = start_y + dy * progress;
                if progress >= step - COLLISION_EPSILON {
                    return Some((nx, ny));
                }
                if best.is_none() {
                    best = Some((nx, ny));
                }
            }
            offset += CORNER_STEP;
        }
    }
    best
}

/// Whether the nearest obstacle along `dir` is a rounded rectangle. Seams and
/// actor capsules do not allow a corner cut.
fn blocked_by_rounded_rect(
    x: f32,
    y: f32,
    dir: Dir,
    blockers: &[Collider],
    seams: &[Seam],
) -> bool {
    let actor = Collider::actor(x, y);
    let mut best = f32::INFINITY;
    let mut rounded = false;
    for &blocker in blockers {
        let distance = free_along(actor, dir, blocker);
        if distance < best {
            best = distance;
            rounded = matches!(blocker, Collider::RoundedRect { .. });
        }
    }
    for seam in seams {
        let distance = free_along_seam(actor, dir, seam);
        if distance < best {
            best = distance;
            rounded = false;
        }
    }
    rounded
}

fn free_distance(
    x: f32,
    y: f32,
    dir: Dir,
    blockers: &[Collider],
    seams: &[Seam],
) -> f32 {
    let actor = Collider::actor(x, y);
    let mut free = match dir {
        Dir::Up => y,
        Dir::Down => (ROWS - 1) as f32 - y,
        Dir::Left => x,
        Dir::Right => (COLS - 1) as f32 - x,
    };

    for blocker in blockers {
        free = free.min(free_along(actor, dir, *blocker));
    }

    // A diagonal gap only needs sealing against horizontal movement: the
    // boxes themselves already block a sprite from fitting between them
    // vertically. Block at the edge of the box the sprite is moving toward.
    for seam in seams {
        free = free.min(free_along_seam(actor, dir, seam));
    }

    free
}

/// Distance an actor can travel along `dir` before touching `blocker`.
/// `f32::INFINITY` means the blocker is not in the way.
fn free_along(actor: Collider, dir: Dir, blocker: Collider) -> f32 {
    let Collider::Capsule { cx, cy, r, half } = actor else {
        return f32::INFINITY;
    };
    match blocker {
        Collider::Capsule {
            cx: ox,
            cy: oy,
            r: or,
            half: ohalf,
        } => free_capsule_capsule(cx, cy, r, half, dir, ox, oy, or, ohalf),
        Collider::RoundedRect {
            left,
            top,
            right,
            bottom,
            corner,
            solid_y,
        } => {
            if !solid_y && matches!(dir, Dir::Up | Dir::Down) {
                return f32::INFINITY;
            }
            let (inner_left, inner_top, inner_right, inner_bottom, corner) =
                rounded_inner(left, top, right, bottom, corner);
            free_capsule_rect(
                cx,
                cy,
                r + corner,
                half,
                dir,
                inner_left,
                inner_top,
                inner_right,
                inner_bottom,
            )
        }
    }
}

/// Sweep a horizontal capsule against an axis-aligned rectangle.
#[allow(clippy::too_many_arguments)]
fn free_capsule_rect(
    cx: f32,
    cy: f32,
    r: f32,
    half: f32,
    dir: Dir,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) -> f32 {
    let dx = (left - (cx + half)).max(cx - half - right).max(0.0);
    let dy = (top - cy).max(cy - bottom).max(0.0);
    match dir {
        Dir::Right => {
            let gap = left - (cx + half);
            if gap > 0.0 {
                if dy >= r - COLLISION_EPSILON {
                    return f32::INFINITY;
                }
                (gap - (r * r - dy * dy).sqrt()).max(0.0)
            } else if right > cx + half + COLLISION_EPSILON
                && dy < r - COLLISION_EPSILON
            {
                0.0
            } else {
                f32::INFINITY
            }
        }
        Dir::Left => {
            let gap = (cx - half) - right;
            if gap > 0.0 {
                if dy >= r - COLLISION_EPSILON {
                    return f32::INFINITY;
                }
                (gap - (r * r - dy * dy).sqrt()).max(0.0)
            } else if left < cx - half - COLLISION_EPSILON
                && dy < r - COLLISION_EPSILON
            {
                0.0
            } else {
                f32::INFINITY
            }
        }
        Dir::Down => {
            let gap = top - cy;
            if gap > 0.0 {
                if dx >= r - COLLISION_EPSILON {
                    return f32::INFINITY;
                }
                (gap - (r * r - dx * dx).sqrt()).max(0.0)
            } else if bottom > cy + r + COLLISION_EPSILON
                && dx < r - COLLISION_EPSILON
            {
                0.0
            } else {
                f32::INFINITY
            }
        }
        Dir::Up => {
            let gap = cy - bottom;
            if gap > 0.0 {
                if dx >= r - COLLISION_EPSILON {
                    return f32::INFINITY;
                }
                (gap - (r * r - dx * dx).sqrt()).max(0.0)
            } else if top < cy - r - COLLISION_EPSILON
                && dx < r - COLLISION_EPSILON
            {
                0.0
            } else {
                f32::INFINITY
            }
        }
    }
}

/// Sweep a horizontal capsule against another horizontal capsule.
#[allow(clippy::too_many_arguments)]
fn free_capsule_capsule(
    cx: f32,
    cy: f32,
    r: f32,
    half: f32,
    dir: Dir,
    ox: f32,
    oy: f32,
    or: f32,
    ohalf: f32,
) -> f32 {
    let radius = r + or;
    let dx = ((ox - ohalf) - (cx + half))
        .max((cx - half) - (ox + ohalf))
        .max(0.0);
    let dy = (oy - cy).abs();
    match dir {
        Dir::Right => {
            if ox <= cx {
                return f32::INFINITY;
            }
            let gap = (ox - ohalf) - (cx + half);
            if gap <= 0.0 {
                if dy < radius - COLLISION_EPSILON {
                    0.0
                } else {
                    f32::INFINITY
                }
            } else if dy >= radius - COLLISION_EPSILON {
                f32::INFINITY
            } else {
                (gap - (radius * radius - dy * dy).sqrt()).max(0.0)
            }
        }
        Dir::Left => {
            if ox >= cx {
                return f32::INFINITY;
            }
            let gap = (cx - half) - (ox + ohalf);
            if gap <= 0.0 {
                if dy < radius - COLLISION_EPSILON {
                    0.0
                } else {
                    f32::INFINITY
                }
            } else if dy >= radius - COLLISION_EPSILON {
                f32::INFINITY
            } else {
                (gap - (radius * radius - dy * dy).sqrt()).max(0.0)
            }
        }
        Dir::Down => {
            if oy <= cy {
                return f32::INFINITY;
            }
            let gap = oy - cy;
            if dx >= radius - COLLISION_EPSILON {
                f32::INFINITY
            } else {
                (gap - (radius * radius - dx * dx).sqrt()).max(0.0)
            }
        }
        Dir::Up => {
            if oy >= cy {
                return f32::INFINITY;
            }
            let gap = cy - oy;
            if dx >= radius - COLLISION_EPSILON {
                f32::INFINITY
            } else {
                (gap - (radius * radius - dx * dx).sqrt()).max(0.0)
            }
        }
    }
}

fn free_along_seam(actor: Collider, dir: Dir, seam: &Seam) -> f32 {
    let Collider::Capsule { cx, cy, r, half } = actor else {
        return f32::INFINITY;
    };
    let dy = if cy < seam.top {
        seam.top - cy
    } else if cy > seam.bottom {
        cy - seam.bottom
    } else {
        0.0
    };
    if dy >= r {
        return f32::INFINITY;
    }
    let reach = (r * r - dy * dy).sqrt();
    match dir {
        Dir::Right => {
            let lead = cx + half;
            if seam.right < lead + r - COLLISION_EPSILON {
                return f32::INFINITY;
            }
            (seam.right - reach - lead).max(0.0)
        }
        Dir::Left => {
            let lead = cx - half;
            if seam.left > lead - r + COLLISION_EPSILON {
                return f32::INFINITY;
            }
            (lead - (seam.left + reach)).max(0.0)
        }
        _ => f32::INFINITY,
    }
}

/// True when the actor's feet overlap the ground area of a hole.
fn on_hole(holes: &[Cell], x: f32, y: f32) -> bool {
    debug_assert!(x.is_finite() && y.is_finite());
    let actor = Collider::actor(x, y);
    holes
        .iter()
        .any(|hole| actor.overlaps(Collider::hole(hole)))
}

/// If (x, y) is touching a hole, return the free landing cell at the other hole.
fn teleport_target(
    holes: &[Cell],
    x: f32,
    y: f32,
    blockers: &[Collider],
) -> Option<(f32, f32)> {
    debug_assert!(holes.len() <= 2, "a map has at most two holes");
    if holes.len() != 2 {
        return None;
    }
    let actor = Collider::actor(x, y);
    for (i, h) in holes.iter().enumerate() {
        if !actor.overlaps(Collider::hole(h)) {
            continue;
        }
        let other = holes[1 - i];
        let (ox, oy) = (other[0] as f32, other[1] as f32);
        for (px, py) in [(ox, oy), (ox + 1.0, oy)] {
            let landing = Collider::actor(px, py);
            let blocked =
                blockers.iter().any(|blocker| landing.overlaps(*blocker));
            if !blocked {
                return Some((px, py));
            }
        }
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(
        cats: &[Cell],
        rats: &[Cell],
        tomatoes: &[Cell],
        boxes: &[Cell],
    ) -> Map {
        Map {
            cats: cats.to_vec(),
            rats: rats.to_vec(),
            tomatoes: tomatoes.to_vec(),
            boxes: boxes.to_vec(),
            ..Map::default()
        }
    }

    fn cat_holds(d: Dir) -> Input {
        let mut i = Input::default();
        i.cat[d.index()] = true;
        i
    }

    fn rat_holds(d: Dir) -> Input {
        let mut i = Input::default();
        i.rat[d.index()] = true;
        i
    }

    #[test]
    fn board_edge_stops_movement() {
        let (x, _) = move_sprite(19.0, 3.0, Dir::Right, 0.5, &[]);
        assert_eq!(x, 19.0);
    }

    #[test]
    fn ground_footprint_stops_at_boxes_but_allows_walking_behind() {
        // Same row: the box blocks the path at its base.
        let (x, y) = move_sprite(1.0, 1.0, Dir::Right, 3.0, &[(3.0, 1.0)]);
        assert!((x - 2.1).abs() < 2e-4);
        assert_eq!(y, 1.0);

        // One row above: the sprite passes behind the box instead of being
        // stopped a full cell away.
        let (x, y) =
            move_sprite(1.0, 0.5, Dir::Right, 3.0, &[(3.0, 0.0), (3.0, 1.0)]);
        assert!((x - 4.0).abs() < 2e-4);
        assert_eq!(y, 0.5);

        // From above, it may walk into the top of the box cell and stops when
        // its feet reach the box base.
        let (x, y) = move_sprite(1.0, 1.0, Dir::Down, 3.0, &[(1.0, 2.0)]);
        assert_eq!(x, 1.0);
        assert!((y - 1.6).abs() < 2e-4);

        // At the stop position it cannot push through the base.
        let (x, y) = move_sprite(2.1, 1.0, Dir::Right, 0.1, &[(3.0, 1.0)]);
        assert!((x - 2.1).abs() < 2e-4);
        assert_eq!(y, 1.0);

        // Having stopped behind a box, it can still slide sideways past it.
        let (x, y) = move_sprite(1.0, 1.0, Dir::Down, 3.0, &[(1.0, 2.0)]);
        let (x, _) = move_sprite(x, y, Dir::Right, 3.0, &[(1.0, 2.0)]);
        assert!((x - 4.0).abs() < 2e-4);
    }

    #[test]
    fn stacked_boxes_form_a_solid_wall() {
        let mut g = Game::new(
            &map(&[[0, 0]], &[[1, 1]], &[[19, 9]], &[[3, 1], [3, 2]]),
            1,
        );
        // Stand in the seam between the two box rows.
        g.rats[0].y = 1.5;
        for _ in 0..120 {
            g.step(&rat_holds(Dir::Right));
        }
        assert!(g.rats[0].x < 3.0);
    }

    #[test]
    fn stacked_box_top_stops_a_sprite_like_a_single_box() {
        let single = box_run_colliders(&[[3, 1]]);
        let stacked = box_run_colliders(&[[3, 1], [3, 2]]);
        let (_, single_y) =
            move_sprite_with_colliders(3.0, 0.5, Dir::Down, 3.0, &single);
        let (_, stacked_y) =
            move_sprite_with_colliders(3.0, 0.5, Dir::Down, 3.0, &stacked);
        assert!((single_y - 0.6).abs() < 2e-4);
        assert!((stacked_y - single_y).abs() < 2e-4);
    }

    #[test]
    fn capsule_corner_does_not_slip_through_a_box() {
        // The box corner sits in the middle of the capsule, between its two
        // end circles. A two-circle approximation missed this and let the
        // sprite slide through.
        let boxes = box_run_colliders(&[[3, 1]]);
        let (x, y) =
            move_sprite_with_colliders(3.4, 0.0, Dir::Down, 3.0, &boxes);
        assert_eq!(x, 3.4);
        assert!((y - 0.6).abs() < 2e-4, "down y={y}");

        let (x, y) = move_sprite_with_colliders(3.4, 3.0, Dir::Up, 3.0, &boxes);
        assert_eq!(x, 3.4);
        assert!((y - 1.4).abs() < 2e-4, "up y={y}");
    }

    #[test]
    fn stacked_boxes_can_be_walked_behind_above_their_top() {
        let mut g = Game::new(
            &map(&[[0, 0]], &[[1, 1]], &[[19, 9]], &[[3, 1], [3, 2]]),
            1,
        );
        g.rats[0].y = 0.5;
        for _ in 0..120 {
            g.step(&rat_holds(Dir::Right));
        }
        assert!(g.rats[0].x > 3.0);
    }

    #[test]
    fn diagonal_boxes_have_no_corner_gap() {
        for boxes in [&[[3, 3], [4, 4]][..], &[[4, 3], [3, 4]][..]] {
            let mut g =
                Game::new(&map(&[[0, 0]], &[[1, 1]], &[[19, 9]], boxes), 1);
            // Walk into the seam between the two diagonal boxes.
            g.rats[0].y = 3.5;
            for _ in 0..120 {
                g.step(&rat_holds(Dir::Right));
            }
            // Stops flush with the left side of the lower-right box.
            assert!(
                (g.rats[0].x - 3.15).abs() < 1e-2,
                "boxes {boxes:?}: rat at x={}",
                g.rats[0].x
            );
        }
    }

    #[test]
    fn diagonal_seam_blocks_both_directions() {
        for boxes in [&[[3, 3], [4, 4]][..], &[[4, 3], [3, 4]][..]] {
            let mut g =
                Game::new(&map(&[[0, 0]], &[[1, 1]], &[[19, 9]], boxes), 1);
            g.rats[0].x = 6.0;
            g.rats[0].y = 3.5;
            for _ in 0..120 {
                g.step(&rat_holds(Dir::Left));
            }
            // Stops flush with the right side of the left box.
            assert!(
                (g.rats[0].x - 3.85).abs() < 1e-2,
                "boxes {boxes:?}: rat at x={}",
                g.rats[0].x
            );
        }
    }

    #[test]
    fn diagonal_seam_stops_where_the_box_does() {
        let boxes = [[3, 3], [4, 4]];
        // Through the seam.
        let (seam_x, _) = move_sprite_with_seams(
            1.0,
            3.5,
            Dir::Right,
            10.0,
            &box_run_colliders(&boxes),
            &box_seams(&boxes),
        );
        // Straight into the lower-right box on a row where it blocks normally.
        let (box_x, _) = move_sprite_with_colliders(
            1.0,
            4.2,
            Dir::Right,
            10.0,
            &box_run_colliders(&boxes),
        );
        // Rounded box corners let the sprite a little closer than the seam.
        assert!((seam_x - box_x).abs() < 0.02, "seam={seam_x} box={box_x}");
    }

    #[test]
    fn diagonal_seam_does_not_block_walking_past_it() {
        let mut g = Game::new(
            &map(&[[0, 0]], &[[1, 1]], &[[19, 9]], &[[3, 3], [4, 4]]),
            1,
        );
        // Feet below both boxes: walk in front of them.
        g.rats[0].y = 4.5;
        for _ in 0..120 {
            g.step(&rat_holds(Dir::Right));
        }
        assert!(g.rats[0].x > 5.0, "rat at x={}", g.rats[0].x);
    }

    #[test]
    fn diagonal_seam_does_not_block_vertical_movement() {
        let mut g = Game::new(
            &map(&[[0, 0]], &[[1, 1]], &[[19, 9]], &[[3, 3], [4, 4]]),
            1,
        );
        // Just right of the upper-left box: slide down until the lower-right
        // box stops the feet, not at the seam.
        g.rats[0].x = 3.9;
        g.rats[0].y = 2.0;
        for _ in 0..120 {
            g.step(&rat_holds(Dir::Down));
        }
        assert!((g.rats[0].y - 3.6).abs() < 1e-2, "rat at y={}", g.rats[0].y);
    }

    #[test]
    fn feet_touching_a_box_top_do_not_block_sideways_movement() {
        // Boxes side by side, tomato above the right one. A rat on the left
        // box has its feet on the right box's footprint edge. Touching there
        // used to count as a wall, so the rat could not move right.
        let m = map(&[[0, 0]], &[[9, 2]], &[[18, 9]], &[[9, 4], [10, 4]]);
        let mut g = Game::new(&m, 1);
        for _ in 0..60 {
            g.step(&rat_holds(Dir::Down));
        }
        let y = g.rats[0].y;
        for _ in 0..120 {
            g.step(&rat_holds(Dir::Right));
        }
        assert!(g.rats[0].x > 10.0, "rat stuck at y={y} x={}", g.rats[0].x);
    }

    #[test]
    fn sprite_can_move_up_while_beside_stacked_boxes() {
        let mut g = Game::new(
            &map(&[[0, 0]], &[[1, 1]], &[[19, 9]], &[[3, 1], [3, 2]]),
            1,
        );
        // Beside the bottom box: the column must not widen to the left.
        g.rats[0].x = 2.0;
        g.rats[0].y = 2.0;
        for _ in 0..120 {
            g.step(&rat_holds(Dir::Up));
        }
        assert!(g.rats[0].y < 1.0);
    }

    #[test]
    fn isolated_box_can_be_walked_behind() {
        let mut g =
            Game::new(&map(&[[0, 0]], &[[1, 1]], &[[19, 9]], &[[3, 1]]), 1);
        g.rats[0].y = 0.5;
        for _ in 0..120 {
            g.step(&rat_holds(Dir::Right));
        }
        assert!(g.rats[0].x > 3.0);
    }

    #[test]
    fn horizontal_box_row_can_still_be_walked_behind() {
        let mut g = Game::new(
            &map(&[[0, 0]], &[[1, 1]], &[[19, 9]], &[[3, 1], [4, 1]]),
            1,
        );
        g.rats[0].y = 0.5;
        for _ in 0..120 {
            g.step(&rat_holds(Dir::Right));
        }
        assert!(g.rats[0].x > 4.0);
    }

    #[test]
    fn cat_stops_at_box() {
        let mut g =
            Game::new(&map(&[[1, 1]], &[[18, 8]], &[[18, 9]], &[[4, 1]]), 1);
        for _ in 0..180 {
            g.step(&cat_holds(Dir::Right));
        }
        assert!((g.cats[0].x - 3.15).abs() < 1e-3);
    }

    #[test]
    fn cat_slides_in_front_of_a_tomato() {
        let mut g =
            Game::new(&map(&[[1, 1]], &[[18, 8]], &[[4, 1], [18, 9]], &[]), 1);
        for _ in 0..180 {
            g.step(&cat_holds(Dir::Right));
        }
        // The tomato has a rounded corner: the cat dips in front of it instead
        // of stopping dead, but it cannot walk through the fruit.
        assert!(g.cats[0].x > 4.0, "x={}", g.cats[0].x);
        assert!(g.cats[0].y > 1.05, "y={}", g.cats[0].y);
    }

    #[test]
    fn tomato_horizontal_block_matches_the_drawn_fruit() {
        let tomato = Collider::tomato(&[3, 1]);
        let Collider::RoundedRect {
            left,
            top,
            right,
            bottom,
            corner,
            ..
        } = tomato
        else {
            panic!("tomato should be a rounded rectangle");
        };
        // Full drawn width, lower part of the fruit, ending at the drawn base.
        assert!((right - left - 0.40625).abs() < 1e-6);
        assert!((top - 1.6).abs() < 1e-6);
        assert!((bottom - 1.6875).abs() < 1e-6);
        assert!((corner - TOMATO_CORNER).abs() < 1e-6);
        let (x, _) =
            move_sprite_with_colliders(1.0, 1.0, Dir::Right, 3.0, &[tomato]);
        assert!((x - 2.40156).abs() < 2e-4);
    }

    #[test]
    fn actor_capsule_ignores_the_tail() {
        let Collider::Capsule { cx, cy, r, half } = Collider::actor(1.0, 1.0)
        else {
            panic!("actors should be capsules");
        };
        // The capsule spans the ground footprint. The sprite is centred on the
        // body, so the tail is not a separate lobe that can widen or catch it.
        assert!((2.0 * (half + r) - 0.9).abs() < 1e-6);
        assert!((cx - 1.5).abs() < 1e-6);
        assert!((cy - 1.8).abs() < 1e-6);
        assert!((r - 0.2).abs() < 1e-6);
        assert!((half - 0.25).abs() < 1e-6);
    }

    #[test]
    fn tomato_bottom_does_not_block_the_cell_below() {
        let tomato = Collider::tomato(&[4, 1]);
        // Below the drawn base: walk straight past the tomato.
        let (x, _) =
            move_sprite_with_colliders(1.0, 1.2, Dir::Right, 4.0, &[tomato]);
        assert!((x - 5.0).abs() < 2e-4);
        // Same row: still blocked horizontally.
        let (x, _) =
            move_sprite_with_colliders(1.0, 1.0, Dir::Right, 4.0, &[tomato]);
        assert!((x - 3.40156).abs() < 2e-4);
    }

    #[test]
    fn tomato_blocks_vertical_movement() {
        let tomato = Collider::tomato(&[4, 1]);
        // Approaching from below: stop at the drawn base instead of passing
        // through the fruit.
        let (x, y) =
            move_sprite_with_colliders(4.0, 2.0, Dir::Up, 2.0, &[tomato]);
        assert!((x - 4.0).abs() < 2e-4, "x={x}");
        assert!((y - 1.0875).abs() < 2e-4, "y={y}");
    }

    #[test]
    fn diagonal_movement_slides_along_a_tomato() {
        let tomato = Collider::tomato(&[4, 1]);
        let (x, y) =
            move_sprite_with_vector(3.40156, 1.0, 0.1, 0.1, &[tomato], &[]);
        assert!((x - 3.40156).abs() < 2e-4, "x={x}");
        assert!((y - 1.1).abs() < 2e-4, "y={y}");
    }

    #[test]
    fn diagonal_movement_slides_along_a_box() {
        let boxes = box_run_colliders(&[[3, 1]]);
        let (x, y) = move_sprite_with_vector(2.15, 1.0, 0.1, -0.1, &boxes, &[]);
        assert!((x - 2.15).abs() < 2e-4, "x={x}");
        assert!((y - 0.9).abs() < 2e-4, "y={y}");
    }

    #[test]
    fn diagonal_movement_cuts_around_a_box_corner() {
        let boxes = box_run_colliders(&[[3, 1]]);
        let (mut x, mut y) = (2.15, 1.0);
        for _ in 0..30 {
            (x, y) = move_sprite_with_vector(x, y, 0.05, 0.05, &boxes, &[]);
        }
        assert!(x > 2.5, "x={x} y={y}");
        assert!(y > 1.3, "x={x} y={y}");
    }

    #[test]
    fn perpendicular_move_cuts_around_a_box_corner() {
        let boxes = box_run_colliders(&[[3, 1]]);
        // Touching the box's lower-left rounded corner: straight up would push
        // into the corner, so a small sideways nudge lets it slide past.
        let (x, y) =
            move_sprite_with_vector(2.1617, 1.2, 0.0, -0.05, &boxes, &[]);
        assert!(y < 1.2 - 0.04, "y={y}");
        assert!(x < 2.1617, "x={x}");
    }

    #[test]
    fn diagonal_movement_does_not_slip_through_a_diagonal_seam() {
        let boxes = [[3, 3], [4, 4]];
        let (x, _) = move_sprite_with_vector(
            1.0,
            3.5,
            10.0,
            10.0,
            &box_run_colliders(&boxes),
            &box_seams(&boxes),
        );
        assert!(x < 3.2, "x={x}");
    }

    #[test]
    fn rounded_tomato_corner_is_softer_than_a_square_corner() {
        let tomato = Collider::tomato(&[4, 1]);
        let Collider::RoundedRect {
            left,
            top,
            right,
            bottom,
            ..
        } = tomato
        else {
            panic!("tomato should be a rounded rectangle");
        };
        let square = Collider::RoundedRect {
            left,
            top,
            right,
            bottom,
            corner: 0.0,
            solid_y: true,
        };
        let (rounded_x, _) =
            move_sprite_with_colliders(1.0, 0.75, Dir::Right, 4.0, &[tomato]);
        let (square_x, _) =
            move_sprite_with_colliders(1.0, 0.75, Dir::Right, 4.0, &[square]);
        assert!(rounded_x > square_x + 1e-3);
    }

    #[test]
    fn rat_eats_tomato_and_wins() {
        let mut g = Game::new(&map(&[[18, 8]], &[[1, 1]], &[[3, 1]], &[]), 1);
        let mut out = None;
        for _ in 0..120 {
            out = g.step(&rat_holds(Dir::Right));
            if out.is_some() {
                break;
            }
        }
        assert_eq!(out, Some(Outcome::RatWin));
    }

    #[test]
    fn cat_kills_rat_and_wins() {
        let mut g = Game::new(&map(&[[1, 1]], &[[3, 1]], &[[18, 9]], &[]), 1);
        let mut out = None;
        for _ in 0..120 {
            out = g.step(&cat_holds(Dir::Right));
            if out.is_some() {
                break;
            }
        }
        assert_eq!(out, Some(Outcome::CatWin));
    }

    #[test]
    fn same_step_cat_and_rat_tie_goes_to_the_cat() {
        // The last rat is on the last tomato while a cat is also there.
        let mut g = Game::new(&map(&[[1, 1]], &[[1, 1]], &[[1, 1]], &[]), 1);
        let outcome = g.step(&Input::default());
        assert_eq!(outcome, Some(Outcome::CatWin));
        // The rat was killed before it could eat the last tomato.
        assert_eq!(g.tomatoes.len(), 1);
    }

    #[test]
    fn cat_catches_rat_before_the_sprites_touch() {
        let mut g = Game::new(&map(&[[1, 1]], &[[1, 2]], &[[18, 9]], &[]), 1);
        g.rats[0].x = 1.3;
        g.rats[0].y = 1.0;
        g.step(&Input::default());
        assert!(!g.rats[0].alive, "rat at x=1.3 was not caught");

        // Vertical reach is circular too: 0.7 right + 0.7 down is outside.
        let mut g = Game::new(&map(&[[1, 1]], &[[1, 2]], &[[18, 9]], &[]), 1);
        g.rats[0].x = 1.7;
        g.rats[0].y = 1.7;
        g.step(&Input::default());
        assert!(g.rats[0].alive, "rat outside the circle was caught");
    }

    #[test]
    fn dead_rat_blocks_living_rats() {
        let mut g =
            Game::new(&map(&[[18, 8]], &[[1, 1], [3, 1]], &[[18, 9]], &[]), 1);
        g.rats[1].alive = false;
        for _ in 0..120 {
            g.step(&rat_holds(Dir::Right));
        }
        assert!((g.rats[0].x - 2.1).abs() < 1e-3);
        assert_eq!(g.outcome(), None);
    }

    #[test]
    fn spikes_toggle_every_second() {
        let m = Map {
            spikes: vec![[10, 5]],
            ..map(&[[0, 0]], &[[19, 9]], &[[19, 0]], &[])
        };
        let mut g = Game::new(&m, 1);
        assert!(!g.spikes_active);
        for _ in 0..62 {
            g.step(&Input::default());
        }
        assert!(g.spikes_active);
        for _ in 0..68 {
            g.step(&Input::default());
        }
        assert!(!g.spikes_active);
    }

    #[test]
    fn raised_spikes_block_at_their_base() {
        let m = Map {
            spikes: vec![[3, 1]],
            ..map(&[[0, 0]], &[[1, 1]], &[[19, 9]], &[])
        };
        let mut g = Game::new(&m, 1);
        g.spikes_active = true;
        // Under one second, so the spikes stay raised for the whole walk.
        for _ in 0..30 {
            g.step(&rat_holds(Dir::Right));
        }
        // Stopped at the spike base, same as a box.
        assert!(
            (g.rats[0].x - 2.11167).abs() < 1e-2,
            "rat at x={}",
            g.rats[0].x
        );
    }

    #[test]
    fn stacked_spikes_form_a_solid_wall() {
        let m = Map {
            spikes: vec![[3, 1], [3, 2]],
            ..map(&[[0, 0]], &[[1, 1]], &[[19, 9]], &[])
        };
        let mut g = Game::new(&m, 1);
        g.spikes_active = true;
        // Feet in the gap between the two spike bases.
        g.rats[0].y = 1.3;
        for _ in 0..30 {
            g.step(&rat_holds(Dir::Right));
        }
        assert!(g.rats[0].x < 3.0, "rat at x={}", g.rats[0].x);
    }

    #[test]
    fn diagonal_spikes_have_no_corner_gap() {
        for spikes in [&[[3, 3], [4, 4]][..], &[[4, 3], [3, 4]][..]] {
            let m = Map {
                spikes: spikes.to_vec(),
                ..map(&[[0, 0]], &[[1, 1]], &[[19, 9]], &[])
            };
            let mut g = Game::new(&m, 1);
            g.spikes_active = true;
            // Feet in the gap between the two spike bases.
            g.rats[0].y = 3.3;
            for _ in 0..30 {
                g.step(&rat_holds(Dir::Right));
            }
            // Stops at the gap edge instead of slipping through the corner.
            assert!(
                g.rats[0].x < 3.4,
                "spikes {spikes:?}: rat at x={}",
                g.rats[0].x
            );
        }
    }

    #[test]
    fn sprites_can_walk_behind_raised_spikes() {
        let m = Map {
            spikes: vec![[3, 1]],
            ..map(&[[0, 0]], &[[1, 1]], &[[19, 9]], &[])
        };
        let mut g = Game::new(&m, 1);
        g.spikes_active = true;
        // Feet above the spike base: walk behind the raised spikes.
        g.rats[0].y = 0.3;
        for _ in 0..30 {
            g.step(&rat_holds(Dir::Right));
        }
        assert!(g.rats[0].x > 3.0, "rat at x={}", g.rats[0].x);
    }

    #[test]
    fn a_sprite_on_a_raised_spike_cannot_walk() {
        let m = Map {
            spikes: vec![[1, 1]],
            ..map(&[[0, 0]], &[[1, 1]], &[[19, 9]], &[])
        };
        let mut g = Game::new(&m, 1);
        g.spikes_active = true;
        for _ in 0..30 {
            g.step(&rat_holds(Dir::Right));
        }
        assert!((g.rats[0].x - 1.0).abs() < 1e-6, "rat at x={}", g.rats[0].x);
    }

    #[test]
    fn raised_spike_catches_the_rounded_end_of_a_capsule() {
        let m = Map {
            spikes: vec![[3, 1]],
            ..map(&[[0, 0]], &[[2, 1]], &[[19, 9]], &[])
        };
        let mut g = Game::new(&m, 1);
        g.spikes_active = true;
        // The capsule's right end just touches the spike's left edge.
        g.rats[0].x = 2.1;
        assert!(g.on_raised_spike(g.rats[0].x, g.rats[0].y));
        for _ in 0..30 {
            g.step(&rat_holds(Dir::Left));
        }
        assert!((g.rats[0].x - 2.1).abs() < 1e-6, "rat at x={}", g.rats[0].x);
    }

    #[test]
    fn losing_the_last_cat_ends_the_round_as_a_draw() {
        let mut g = Game::new(&map(&[[0, 0]], &[[19, 9]], &[[19, 0]], &[]), 1);
        let mut out = None;
        for _ in 0..1810 {
            out = g.step(&Input::default());
            if out.is_some() {
                break;
            }
        }
        assert!(g.cats.is_empty());
        assert_eq!(out, Some(Outcome::Draw));
    }

    #[test]
    fn one_cat_leaves_every_30_seconds() {
        let mut g = Game::new(
            &map(&[[0, 0], [1, 0], [2, 0]], &[[19, 9]], &[[19, 0]], &[]),
            1,
        );
        assert!((g.cat_attrition_remaining() - 30.0).abs() < 1e-5);
        for _ in 0..1810 {
            g.step(&Input::default());
        }
        assert_eq!(g.cats.len(), 2);
        assert!((29.0..30.0).contains(&g.cat_attrition_remaining()));
    }

    #[test]
    fn holes_teleport_with_cooldown() {
        let m = Map {
            holes: vec![[2, 2], [10, 5]],
            ..map(&[[19, 9]], &[[0, 2]], &[[19, 0]], &[])
        };
        let mut g = Game::new(&m, 1);
        let mut teleported = false;
        for _ in 0..60 {
            g.step(&rat_holds(Dir::Right));
            if (g.rats[0].y - 5.0).abs() < 1e-3 {
                teleported = true;
                break;
            }
        }
        assert!(teleported);
        assert!(g.rats[0].x >= 10.0);
        // The teleport must not be interpolated across the two holes.
        assert_eq!(g.rats[0].prev_x, g.rats[0].x);
        assert_eq!(g.rats[0].prev_y, g.rats[0].y);
    }

    #[test]
    fn step_records_the_previous_position_for_interpolation() {
        let mut g = Game::new(&map(&[[0, 0]], &[[1, 1]], &[[19, 9]], &[]), 1);
        let (x, y) = (g.rats[0].x, g.rats[0].y);
        g.step(&rat_holds(Dir::Right));
        assert_eq!(g.rats[0].prev_x, x);
        assert_eq!(g.rats[0].prev_y, y);
        assert!(g.rats[0].x > x);
        assert_eq!(g.rats[0].y, y);
    }

    #[test]
    fn teleport_needs_foot_contact() {
        let holes = [[2, 2], [10, 5]];
        // Within one cell of the hole's corner, but the feet are not on it.
        assert_eq!(teleport_target(&holes, 1.1, 1.1, &[]), None);
        // Feet on the hole ground area.
        assert!(teleport_target(&holes, 2.0, 2.0, &[]).is_some());
    }

    #[test]
    fn teleport_does_not_bounce_while_standing_on_the_exit() {
        let m = Map {
            holes: vec![[2, 2], [10, 5]],
            ..map(&[[19, 9]], &[[0, 2]], &[[19, 0]], &[])
        };
        let mut g = Game::new(&m, 1);
        for _ in 0..60 {
            g.step(&rat_holds(Dir::Right));
            if g.rats[0].x >= 10.0 {
                break;
            }
        }
        assert!((g.rats[0].y - 5.0).abs() < 1e-6);
        // Stay on the exit hole for longer than the cooldown, then walk off.
        for _ in 0..140 {
            g.step(&Input::default());
        }
        for _ in 0..30 {
            g.step(&rat_holds(Dir::Right));
        }
        assert!(
            (g.rats[0].y - 5.0).abs() < 1e-6,
            "rat bounced back to y={}",
            g.rats[0].y
        );
        assert!(g.rats[0].x > 10.5);
    }

    #[test]
    fn teleport_landing_must_be_free() {
        let holes = [[2, 2], [10, 5]];
        let left_blocked = vec![Collider::object(10.0, 5.0)];
        assert_eq!(
            teleport_target(&holes, 2.0, 2.0, &left_blocked),
            Some((11.0, 5.0))
        );

        let both_blocked =
            vec![Collider::object(10.0, 5.0), Collider::object(11.0, 5.0)];
        assert_eq!(teleport_target(&holes, 2.0, 2.0, &both_blocked), None);
    }

    #[test]
    fn same_seed_produces_the_same_round() {
        let m = map(&[[1, 1]], &[[3, 1]], &[[18, 9]], &[]);
        let mut left = Game::new(&m, 42);
        let mut right = Game::new(&m, 42);
        let input = cat_holds(Dir::Right);
        for _ in 0..600 {
            assert_eq!(left.step(&input), right.step(&input));
        }
        assert_eq!(left.time, right.time);
        assert_eq!(left.cats.len(), right.cats.len());
        assert_eq!(left.rats.len(), right.rats.len());
        assert_eq!(left.spikes_active, right.spikes_active);
        for (a, b) in left.cats.iter().zip(&right.cats) {
            assert_eq!((a.x, a.y), (b.x, b.y));
        }
    }
}
