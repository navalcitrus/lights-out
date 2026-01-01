use crate::topology::{CellId, Gluing, Topology, canonicalize};

/// A 2-D drawing of a topology: one canonical position per cell, plus "ghost" copies of the non-canonical preimages emitted by quotient operations so callers can render seam identifications.
#[derive(Debug, Clone)]
pub struct Layout {
    pub n: usize,
    /// Canonical drawing position per cell.
    pub positions: Vec<(f32, f32)>,
    /// A scalar coordinate per cell, used as the natural axis
    pub primary: Vec<f32>,
    pub ghosts: Vec<Ghost>,
    pub bounds: Rect,
}

impl Layout {
    fn empty() -> Self {
        Layout {
            n: 0,
            positions: vec![],
            primary: vec![],
            ghosts: vec![],
            bounds: Rect::empty(),
        }
    }

    /// Build a ghost-free layout, computing `n` and `bounds` from `positions`.
    fn from_positions(positions: Vec<(f32, f32)>, primary: Vec<f32>) -> Self {
        let bounds = Rect::from_points(&positions);
        Layout {
            n: positions.len(),
            positions,
            primary,
            ghosts: vec![],
            bounds,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Ghost {
    pub cell: CellId,
    pub xy: (f32, f32),
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub min: (f32, f32),
    pub max: (f32, f32),
}

impl Rect {
    pub fn empty() -> Self {
        Rect {
            min: (0.0, 0.0),
            max: (0.0, 0.0),
        }
    }

    pub fn point(p: (f32, f32)) -> Self {
        Rect { min: p, max: p }
    }

    pub fn from_points(points: &[(f32, f32)]) -> Self {
        match points.split_first() {
            None => Self::empty(),
            Some((first, rest)) => rest
                .iter()
                .fold(Self::point(*first), |r, p| r.union_point(*p)),
        }
    }

    pub fn union_point(self, p: (f32, f32)) -> Self {
        Rect {
            min: (self.min.0.min(p.0), self.min.1.min(p.1)),
            max: (self.max.0.max(p.0), self.max.1.max(p.1)),
        }
    }

    pub fn union(self, other: Self) -> Self {
        Rect {
            min: (self.min.0.min(other.min.0), self.min.1.min(other.min.1)),
            max: (self.max.0.max(other.max.0), self.max.1.max(other.max.1)),
        }
    }

    pub fn translate(self, d: (f32, f32)) -> Self {
        Rect {
            min: (self.min.0 + d.0, self.min.1 + d.1),
            max: (self.max.0 + d.0, self.max.1 + d.1),
        }
    }

    pub fn width(&self) -> f32 {
        self.max.0 - self.min.0
    }
}

impl Topology {
    /// Compute a 2-D layout for this topology.
    pub fn layout(&self) -> Layout {
        match self {
            Topology::Empty => Layout::empty(),
            Topology::Single => Layout::from_positions(vec![(0.0, 0.0)], vec![0.0]),
            Topology::Path(n) => path_layout(*n),
            Topology::Cycle(n) => cycle_layout(*n),
            Topology::Product(a, b) => product_layout(&a.layout(), &b.layout()),
            Topology::Sum(a, b) => sum_layout(&a.layout(), &b.layout()),
            Topology::Quotient(t, g) => quotient_layout(&t.layout(), g),
            Topology::Lifted2D(q) => {
                // Lay the rectangle out as a `rows` by `cols` grid, then quotient by the derived gluing to fold the seams.
                quotient_layout(&grid_layout(q.rows, q.cols), &q.derived_gluing())
            }
        }
    }
}

fn path_layout(n: usize) -> Layout {
    let positions = (0..n).map(|i| (i as f32, 0.0)).collect();
    let primary = (0..n).map(|i| i as f32).collect();
    Layout::from_positions(positions, primary)
}

fn cycle_layout(n: usize) -> Layout {
    if n == 0 {
        return Layout::empty();
    }
    let two_pi = 2.0 * std::f32::consts::PI;
    let r = (n as f32) / two_pi;
    let positions = (0..n)
        .map(|i| {
            let theta = two_pi * (i as f32) / (n as f32);
            (r * theta.sin(), -r * theta.cos())
        })
        .collect();
    let primary = (0..n).map(|i| i as f32).collect();
    Layout::from_positions(positions, primary)
}

/// `rows` by `cols` integer grid in row-major order
fn grid_layout(rows: usize, cols: usize) -> Layout {
    let positions = (0..rows)
        .flat_map(|r| (0..cols).map(move |c| (c as f32, r as f32)))
        .collect();
    let primary = (0..rows * cols).map(|i| i as f32).collect();
    Layout::from_positions(positions, primary)
}

fn product_layout(a: &Layout, b: &Layout) -> Layout {
    let positions = (0..a.n)
        .flat_map(|i| (0..b.n).map(move |j| (b.primary[j], a.primary[i])))
        .collect();
    let primary = (0..a.n * b.n).map(|i| i as f32).collect();
    Layout::from_positions(positions, primary)
}

fn sum_layout(a: &Layout, b: &Layout) -> Layout {
    const PAD: f32 = 1.0;
    if a.n == 0 {
        return b.clone();
    }
    if b.n == 0 {
        return a.clone();
    }
    let dx = a.bounds.width() + PAD;
    let primary_offset = a.primary.iter().copied().fold(0.0_f32, f32::max) + PAD;

    let mut positions = a.positions.clone();
    positions.extend(b.positions.iter().map(|&(x, y)| (x + dx, y)));
    let mut primary = a.primary.clone();
    primary.extend(b.primary.iter().map(|&v| v + primary_offset));
    let mut ghosts = a.ghosts.clone();
    ghosts.extend(b.ghosts.iter().map(|g| Ghost {
        cell: CellId(g.cell.0 + a.n),
        xy: (g.xy.0 + dx, g.xy.1),
    }));

    Layout {
        n: a.n + b.n,
        positions,
        primary,
        ghosts,
        bounds: a.bounds.union(b.bounds.translate((dx, 0.0))),
    }
}

fn quotient_layout(inner: &Layout, g: &Gluing) -> Layout {
    let (canon, n_canon) = canonicalize(inner.n, g);
    let mut positions = vec![(0.0, 0.0); n_canon];
    let mut primary = vec![0.0; n_canon];
    let mut seen = vec![false; n_canon];
    let mut ghosts = Vec::new();
    for (i, &c) in canon.iter().enumerate() {
        if !seen[c] {
            positions[c] = inner.positions[i];
            primary[c] = inner.primary[i];
            seen[c] = true;
        } else {
            ghosts.push(Ghost {
                cell: CellId(c),
                xy: inner.positions[i],
            });
        }
    }
    ghosts.extend(inner.ghosts.iter().map(|gh| Ghost {
        cell: CellId(canon[gh.cell.0]),
        xy: gh.xy,
    }));
    Layout {
        n: n_canon,
        positions,
        primary,
        ghosts,
        bounds: inner.bounds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn path_three_is_colinear() {
        let l = Topology::Path(3).layout();
        assert_eq!(l.n, 3);
        assert!(approx(l.positions[0].0, 0.0));
        assert!(approx(l.positions[1].0, 1.0));
        assert!(approx(l.positions[2].0, 2.0));
        assert!(l.positions.iter().all(|p| approx(p.1, 0.0)));
        assert!(l.ghosts.is_empty());
    }

    #[test]
    fn cycle_six_equidistant_from_origin() {
        let l = Topology::Cycle(6).layout();
        assert_eq!(l.n, 6);
        let r = 6.0 / (2.0 * std::f32::consts::PI);
        for p in &l.positions {
            assert!(approx((p.0 * p.0 + p.1 * p.1).sqrt(), r));
        }
    }

    #[test]
    fn product_path_two_path_three_is_grid() {
        let l =
            Topology::Product(Box::new(Topology::Path(2)), Box::new(Topology::Path(3))).layout();
        assert_eq!(l.n, 6);
        // Cell (i, j) at index i * 3 + j -> position (j, i) (second factor runs along x, first along y).
        assert!(approx(l.positions[0].0, 0.0) && approx(l.positions[0].1, 0.0));
        assert!(approx(l.positions[5].0, 2.0) && approx(l.positions[5].1, 1.0));
    }

    #[test]
    fn sum_two_paths_disjoint() {
        let l = Topology::Sum(Box::new(Topology::Path(2)), Box::new(Topology::Path(2))).layout();
        assert_eq!(l.n, 4);
        // First path at x = 0, 1.
        // Second path shifted by width + pad = 2.
        assert!(approx(l.positions[0].0, 0.0));
        assert!(approx(l.positions[1].0, 1.0));
        assert!(approx(l.positions[2].0, 2.0));
        assert!(approx(l.positions[3].0, 3.0));
    }

    #[test]
    fn quotient_path_three_with_endpoint_glue() {
        let g = Gluing(vec![vec![CellId(0), CellId(2)]]);
        let t = Topology::Quotient(Box::new(Topology::Path(3)), g);
        let l = t.layout();
        assert_eq!(l.n, 2);
        // Cell 0 sits at original cell 0's position
        assert!(approx(l.positions[0].0, 0.0));
        assert!(approx(l.positions[1].0, 1.0));
        assert_eq!(l.ghosts.len(), 1);
        assert_eq!(l.ghosts[0].cell, CellId(0));
        assert!(approx(l.ghosts[0].xy.0, 2.0));
    }
}
