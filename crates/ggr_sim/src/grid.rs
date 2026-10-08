//! The logical grid: which cells are walls, footprints, slots, the door, and the locked wing.
//! Derived state: rebuilt from the layout plus the placed instances, never saved.
//!
//! Movement is 8-connected with no corner cutting: a diagonal step needs both orthogonal
//! neighbours walkable. Every step costs one, as in V2.

use std::collections::VecDeque;

use ggr_content::Layout;

use crate::types::Cell;

pub const CELL_WALL: u8 = 1;
pub const CELL_FOOTPRINT: u8 = 2;
pub const CELL_SLOT: u8 = 4;
pub const CELL_LOCKED: u8 = 8;
pub const CELL_DOOR: u8 = 16;

#[derive(Debug, Clone)]
pub struct Grid {
    pub width: i32,
    pub height: i32,
    flags: Vec<u8>,
    /// Instance owning each footprint or slot cell, or -1.
    owner: Vec<i32>,
    pub door: Cell,
}

#[derive(Debug, Default)]
pub struct Scratch {
    dist: Vec<i32>,
    queue: VecDeque<usize>,
}

const DIRS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

impl Grid {
    pub fn from_layout(l: &Layout) -> Grid {
        let n = (l.width * l.height) as usize;
        let mut g = Grid {
            width: l.width,
            height: l.height,
            flags: vec![0; n],
            owner: vec![-1; n],
            door: l.door,
        };
        for r in &l.blocked {
            for (x, y) in r.cells() {
                g.set(x, y, CELL_WALL);
            }
        }
        for (x, y) in l.east_wall.cells() {
            g.set(x, y, CELL_WALL);
        }
        for (x, y) in l.east_region.cells() {
            g.set(x, y, CELL_LOCKED);
        }
        g.set(l.door.0, l.door.1, CELL_DOOR);
        g
    }

    /// Opens the East Wing: its doorways become floor and its cells become buildable.
    pub fn open_east_wing(&mut self, l: &Layout) {
        for r in &l.east_openings {
            for (x, y) in r.cells() {
                self.clear(x, y, CELL_WALL);
            }
        }
        for (x, y) in l.east_region.cells() {
            self.clear(x, y, CELL_LOCKED);
        }
    }

    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.width && y < self.height
    }

    fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.width + x) as usize
    }

    pub fn flags(&self, x: i32, y: i32) -> u8 {
        if self.in_bounds(x, y) {
            self.flags[self.idx(x, y)]
        } else {
            CELL_WALL
        }
    }

    pub fn has(&self, x: i32, y: i32, f: u8) -> bool {
        self.flags(x, y) & f != 0
    }

    pub fn set(&mut self, x: i32, y: i32, f: u8) {
        if self.in_bounds(x, y) {
            let i = self.idx(x, y);
            self.flags[i] |= f;
        }
    }

    pub fn clear(&mut self, x: i32, y: i32, f: u8) {
        if self.in_bounds(x, y) {
            let i = self.idx(x, y);
            self.flags[i] &= !f;
        }
    }

    pub fn owner(&self, x: i32, y: i32) -> Option<u32> {
        if !self.in_bounds(x, y) {
            return None;
        }
        let o = self.owner[self.idx(x, y)];
        (o >= 0).then_some(o as u32)
    }

    pub fn set_owner(&mut self, x: i32, y: i32, inst: Option<u32>) {
        if self.in_bounds(x, y) {
            let i = self.idx(x, y);
            self.owner[i] = inst.map_or(-1, |v| v as i32);
        }
    }

    pub fn walkable(&self, x: i32, y: i32) -> bool {
        self.in_bounds(x, y) && self.flags(x, y) & (CELL_WALL | CELL_FOOTPRINT) == 0
    }

    fn step_ok(&self, x: i32, y: i32, dx: i32, dy: i32) -> bool {
        let (nx, ny) = (x + dx, y + dy);
        if !self.walkable(nx, ny) {
            return false;
        }
        if dx != 0 && dy != 0 {
            // No corner cutting: both orthogonal neighbours must be open.
            return self.walkable(x + dx, y) && self.walkable(x, y + dy);
        }
        true
    }

    fn bfs(&self, from: Cell, stop_at: Option<Cell>, s: &mut Scratch) {
        let n = (self.width * self.height) as usize;
        s.dist.clear();
        s.dist.resize(n, -1);
        s.queue.clear();
        if !self.in_bounds(from.0, from.1) {
            return;
        }
        let start = self.idx(from.0, from.1);
        s.dist[start] = 0;
        s.queue.push_back(start);
        let stop = stop_at.map(|c| self.idx(c.0, c.1));
        while let Some(i) = s.queue.pop_front() {
            if Some(i) == stop {
                return;
            }
            let x = i as i32 % self.width;
            let y = i as i32 / self.width;
            for (dx, dy) in DIRS {
                if self.step_ok(x, y, dx, dy) {
                    let j = self.idx(x + dx, y + dy);
                    if s.dist[j] < 0 {
                        s.dist[j] = s.dist[i] + 1;
                        s.queue.push_back(j);
                    }
                }
            }
        }
    }

    /// Steps from `from` to `to`, or None if unreachable. A walk may start on a non-walkable
    /// cell (somebody standing where a footprint has just gone down walks off it).
    pub fn distance(&self, from: Cell, to: Cell, s: &mut Scratch) -> Option<i32> {
        if from == to {
            return Some(0);
        }
        if !self.in_bounds(to.0, to.1) || !self.walkable(to.0, to.1) {
            return None;
        }
        self.bfs(from, Some(to), s);
        let d = s.dist[self.idx(to.0, to.1)];
        (d >= 0).then_some(d)
    }

    /// True if every cell in `targets` is reachable from `from`.
    pub fn all_reachable(&self, from: Cell, targets: &[Cell], s: &mut Scratch) -> bool {
        self.bfs(from, None, s);
        targets
            .iter()
            .all(|c| self.in_bounds(c.0, c.1) && (s.dist[self.idx(c.0, c.1)] >= 0 || *c == from))
    }

    /// The cell path from `from` to `to`, both inclusive. Deterministic: ties broken by the
    /// fixed direction order. Empty if unreachable.
    pub fn path(&self, from: Cell, to: Cell) -> Vec<Cell> {
        let mut s = Scratch::default();
        if from == to {
            return vec![from];
        }
        // Flood from the goal, then walk downhill from the start.
        if !self.walkable(to.0, to.1) {
            return Vec::new();
        }
        self.bfs(to, None, &mut s);
        let Some(mut d) = (self.in_bounds(from.0, from.1))
            .then(|| s.dist[self.idx(from.0, from.1)])
            .filter(|d| *d >= 0)
            .or_else(|| {
                // Starting off-grid-walkable (a footprint just went down): step to the best
                // walkable neighbour first.
                DIRS.iter()
                    .filter(|(dx, dy)| self.walkable(from.0 + dx, from.1 + dy))
                    .map(|(dx, dy)| s.dist[self.idx(from.0 + dx, from.1 + dy)])
                    .filter(|d| *d >= 0)
                    .min()
                    .map(|d| d + 1)
            })
        else {
            return Vec::new();
        };
        let mut out = vec![from];
        let mut cur = from;
        while cur != to {
            let mut next = None;
            for (dx, dy) in DIRS {
                let (nx, ny) = (cur.0 + dx, cur.1 + dy);
                let ok = if self.walkable(cur.0, cur.1) {
                    self.step_ok(cur.0, cur.1, dx, dy)
                } else {
                    self.walkable(nx, ny)
                };
                if ok && s.dist[self.idx(nx, ny)] == d - 1 {
                    next = Some((nx, ny));
                    break;
                }
            }
            match next {
                Some(n) => {
                    out.push(n);
                    cur = n;
                    d -= 1;
                }
                None => return Vec::new(),
            }
        }
        out
    }
}
