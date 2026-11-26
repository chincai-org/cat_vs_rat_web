//! The simulation: movement, eating, teleporting, spikes, cat attrition, win checks.

use crate::controls::{Dir, SideControl};
use crate::map::{COLS, Cell, Map, ROWS};
use crate::rng::Rng;

/// The simulation advances in fixed steps of this many seconds.
pub const FIXED_DT: f32 = 1.0 / 60.0;
/// Cells per second. The original moved 0.2 / 0.4 cells per 50-67 ms tick (about 3-4 and 6-8 cells/s).
pub const CAT_SPEED: f32 = 3.5;
pub const RAT_SPEED: f32 = 7.0;

const EAT_RANGE: f32 = 0.5;
/// Ground footprint, at the bottom of the cell. Actors and map objects share
/// it so sprites can walk behind the upper part of a box (2.5D) but still stop
/// at its base.
const FOOT_LEFT: f32 = 0.05;
const FOOT_RIGHT: f32 = 0.95;
const FOOT_TOP: f32 = 0.60;
const FOOT_BOTTOM: f32 = 1.0;
/// Tomato art is narrower than its cell, so its horizontal blocker matches it.
const TOMATO_LEFT: f32 = 0.30;
const TOMATO_RIGHT: f32 = 0.70;
const COLLISION_EPSILON: f32 = 1e-4;
const TELEPORT_COOLDOWN: f32 = 2.0;
/// The original code toggles every 1 s (its README says 2 s).
const SPIKE_INTERVAL: f32 = 1.0;
const ATTRITION_INTERVAL: f32 = 30.0;

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
    last_teleport: f32,
}

#[derive(Debug, Clone)]
pub struct Rat {
    pub x: f32,
    pub y: f32,
    pub alive: bool,
    died_at: f32,
    vanish_after: f32,
    last_teleport: f32,
}

/// Collision tracks floor contact areas, not the full projected sprite artwork.
#[derive(Debug, Clone, Copy)]
struct Collider {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Collider {
    fn actor(x: f32, y: f32) -> Self {
        Self {
            left: x + FOOT_LEFT,
            top: y + FOOT_TOP,
            right: x + FOOT_RIGHT,
            bottom: y + FOOT_BOTTOM,
        }
    }

    fn object(x: f32, y: f32) -> Self {
        Self::actor(x, y)
    }

    /// One column of vertically touching boxes. The footprint covers the
    /// whole run so sprites cannot slip through the seam between stacked
    /// boxes, while the top of the run keeps the usual walk-behind area.
    fn box_run(x: i32, top: i32, bottom: i32) -> Self {
        debug_assert!(top <= bottom, "a box run ends at or below its start");
        Self {
            left: x as f32 + FOOT_LEFT,
            top: top as f32 + FOOT_TOP,
            right: x as f32 + FOOT_RIGHT,
            bottom: bottom as f32 + FOOT_BOTTOM,
        }
    }

    fn overlaps(self, other: Self) -> bool {
        self.left < other.right + COLLISION_EPSILON
            && self.right > other.left - COLLISION_EPSILON
            && self.top < other.bottom + COLLISION_EPSILON
            && self.bottom > other.top - COLLISION_EPSILON
    }
}

fn map_cell_collider(cell: &Cell) -> Collider {
    Collider::object(cell[0] as f32, cell[1] as f32)
}

/// Cats stop at the tomato's drawn width, not the whole cell.
fn tomato_collider(cell: &Cell) -> Collider {
    let [x, y] = *cell;
    Collider {
        left: x as f32 + TOMATO_LEFT,
        top: y as f32 + FOOT_TOP,
        right: x as f32 + TOMATO_RIGHT,
        bottom: y as f32 + FOOT_BOTTOM,
    }
}

/// Merge every contiguous vertical run of boxes into one footprint.
///
/// Two boxes stacked in the same column would otherwise leave a 0.6-cell
/// seam between their foot footprints, which a sprite could walk through.
fn box_run_colliders(boxes: &[Cell]) -> Vec<Collider> {
    boxes
        .iter()
        .filter(|cell| !boxes.contains(&[cell[0], cell[1] - 1]))
        .map(|cell| {
            let [x, mut bottom] = *cell;
            while boxes.contains(&[x, bottom + 1]) {
                bottom += 1;
            }
            Collider::box_run(x, cell[1], bottom)
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct Game {
    pub cats: Vec<Cat>,
    pub rats: Vec<Rat>,
    pub tomatoes: Vec<Cell>,
    pub boxes: Vec<Cell>,
    /// Precomputed because boxes never move during a round.
    box_colliders: Vec<Collider>,
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
                alive: true,
                died_at: 0.0,
                vanish_after: rng.range_inclusive(3, 7) as f32,
                last_teleport: f32::NEG_INFINITY,
            })
            .collect();
        let box_colliders = box_run_colliders(&map.boxes);
        Game {
            cats: map
                .cats
                .iter()
                .map(|c| Cat {
                    x: c[0] as f32,
                    y: c[1] as f32,
                    last_teleport: f32::NEG_INFINITY,
                })
                .collect(),
            rats,
            tomatoes: map.tomatoes.clone(),
            boxes: map.boxes.clone(),
            box_colliders,
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

        self.move_sprites();

        // Random order decides who wins a same-frame tie.
        if self.rng.coin() {
            self.cats_eat_rats();
            self.rats_eat_tomatoes();
        } else {
            self.rats_eat_tomatoes();
            self.cats_eat_rats();
        }

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
            out.extend(self.spikes.iter().map(map_cell_collider));
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
        out.extend(self.tomatoes.iter().map(tomato_collider));
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
        for m in order.drain(..) {
            let now = self.time;
            match m {
                Mover::Cat(i) => {
                    self.fill_cat_blockers(i, &mut blockers);
                    let (x, y) = (self.cats[i].x, self.cats[i].y);
                    let (mut nx, mut ny) = move_sprite_with_colliders(
                        x,
                        y,
                        self.cat_ctl.facing(),
                        CAT_SPEED * FIXED_DT,
                        &blockers,
                    );
                    let mut teleported = false;
                    if now - self.cats[i].last_teleport > TELEPORT_COOLDOWN {
                        if let Some((tx, ty)) =
                            teleport_target(&self.holes, nx, ny, &blockers)
                        {
                            nx = tx;
                            ny = ty;
                            teleported = true;
                        }
                    }
                    let cat = &mut self.cats[i];
                    cat.x = nx;
                    cat.y = ny;
                    if teleported {
                        cat.last_teleport = now;
                    }
                }
                Mover::Rat(i) => {
                    if !self.rats[i].alive {
                        continue;
                    }
                    self.fill_rat_blockers(i, &mut blockers);
                    let (x, y) = (self.rats[i].x, self.rats[i].y);
                    let (mut nx, mut ny) = move_sprite_with_colliders(
                        x,
                        y,
                        self.rat_ctl.facing(),
                        RAT_SPEED * FIXED_DT,
                        &blockers,
                    );
                    let mut teleported = false;
                    if now - self.rats[i].last_teleport > TELEPORT_COOLDOWN {
                        if let Some((tx, ty)) =
                            teleport_target(&self.holes, nx, ny, &blockers)
                        {
                            nx = tx;
                            ny = ty;
                            teleported = true;
                        }
                    }
                    let rat = &mut self.rats[i];
                    rat.x = nx;
                    rat.y = ny;
                    if teleported {
                        rat.last_teleport = now;
                    }
                }
            }
        }
        self.order_buf = order;
        self.blocker_buf = blockers;
    }

    fn cats_eat_rats(&mut self) {
        let now = self.time;
        for cat in &self.cats {
            for rat in self.rats.iter_mut() {
                if rat.alive
                    && (cat.x - rat.x).abs() < EAT_RANGE
                    && (cat.y - rat.y).abs() < EAT_RANGE
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
            let (tx, ty) = (t[0] as f32, t[1] as f32);
            !rats.iter().any(|r| {
                r.alive
                    && (r.x - tx).abs() < EAT_RANGE
                    && (r.y - ty).abs() < EAT_RANGE
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
    debug_assert!(step >= 0.0, "step must not be negative");
    debug_assert!(x >= -1e-3 && x <= (COLS - 1) as f32 + 1e-3);
    debug_assert!(y >= -1e-3 && y <= (ROWS - 1) as f32 + 1e-3);
    let actor = Collider::actor(x, y);
    let mut free_up = y;
    let mut free_down = (ROWS - 1) as f32 - y;
    let mut free_left = x;
    let mut free_right = (COLS - 1) as f32 - x;

    for blocker in blockers {
        let overlaps_y = actor.top < blocker.bottom + COLLISION_EPSILON
            && actor.bottom > blocker.top - COLLISION_EPSILON;
        if overlaps_y {
            if blocker.right <= actor.left + COLLISION_EPSILON {
                free_left = free_left.min(
                    (actor.left - blocker.right - COLLISION_EPSILON).max(0.0),
                );
            } else if blocker.left >= actor.right - COLLISION_EPSILON {
                free_right = free_right.min(
                    (blocker.left - actor.right - COLLISION_EPSILON).max(0.0),
                );
            }
        }

        let overlaps_x = actor.left < blocker.right + COLLISION_EPSILON
            && actor.right > blocker.left - COLLISION_EPSILON;
        if overlaps_x {
            if blocker.bottom <= actor.top + COLLISION_EPSILON {
                free_up = free_up.min(
                    (actor.top - blocker.bottom - COLLISION_EPSILON).max(0.0),
                );
            } else if blocker.top >= actor.bottom - COLLISION_EPSILON {
                free_down = free_down.min(
                    (blocker.top - actor.bottom - COLLISION_EPSILON).max(0.0),
                );
            }
        }
    }

    let free = match dir {
        Dir::Up => free_up,
        Dir::Down => free_down,
        Dir::Left => free_left,
        Dir::Right => free_right,
    };
    let (dx, dy) = dir.delta();
    let distance = step.min(free).max(0.0);
    debug_assert!(distance >= 0.0 && distance <= step);
    (x + dx * distance, y + dy * distance)
}

/// If (x, y) is within 1 cell of a hole, return the free landing cell at the other hole.
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
    for (i, h) in holes.iter().enumerate() {
        let (hx, hy) = (h[0] as f32, h[1] as f32);
        let near = ((x - hx).abs() < 1.0 || (x - (hx + 1.0)).abs() < 1.0)
            && (y - hy).abs() < 1.0;
        if !near {
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
        assert!((g.cats[0].x - 3.1).abs() < 1e-3);
    }

    #[test]
    fn tomato_blocks_cats() {
        let mut g =
            Game::new(&map(&[[1, 1]], &[[18, 8]], &[[4, 1], [18, 9]], &[]), 1);
        for _ in 0..180 {
            g.step(&cat_holds(Dir::Right));
        }
        assert!((g.cats[0].x - 3.35).abs() < 1e-3);
    }

    #[test]
    fn tomato_horizontal_block_matches_the_apple_art() {
        let tomato = tomato_collider(&[3, 1]);
        assert!(((tomato.right - tomato.left) - 0.4).abs() < 1e-6);
        let (x, _) =
            move_sprite_with_colliders(1.0, 1.0, Dir::Right, 3.0, &[tomato]);
        assert!((x - 2.35).abs() < 2e-4);
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
