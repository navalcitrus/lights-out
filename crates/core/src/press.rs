use crate::modular::add_mod;
use crate::quotient2d::Quotient2D;
use crate::topology::{Adjacency, CellId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PressError {
    /// Modulus was zero and `MoveVec::new` cannot reduce modulo `0`.
    ZeroModulus,
    /// A `CellId` was outside the valid range `0..n` for the topology.
    CellOutOfRange { n: usize, bad: CellId },
}

/// The effect of a press on a topology (puzzle board) as an additive delta
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct MoveVec {
    // Sparse vector over `\mathbb{Z}/k\mathbb{Z}`.
    // Will be sorted by cell id, with deltas reduced modulo `k` and zero entries removed.
    // Duplicate cells in the input will be summed modulo `k` before zero-stripping.
    pub deltas: Vec<(CellId, u8)>,
}

impl MoveVec {
    pub fn new(k: u8, raw: Vec<(CellId, u8)>) -> Result<Self, PressError> {
        if k == 0 {
            return Err(PressError::ZeroModulus);
        }
        let mut raw = raw;
        raw.sort_by_key(|&(c, _)| c);
        let deltas = raw
            .chunk_by(|a, b| a.0 == b.0)
            .map(|run| {
                (
                    run[0].0,
                    run.iter().fold(0, |acc, &(_, d)| add_mod(acc, d % k, k)),
                )
            })
            .filter(|&(_, d)| d != 0)
            .collect();
        Ok(MoveVec { deltas })
    }

    pub fn empty() -> Self {
        MoveVec { deltas: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.deltas.is_empty()
    }
}

/// Closure type for `PressPattern::Custom`: given a pressed cell, the modulus `k` and the (post-quotient) adjacency, produce a normalised move vector or a press error.
pub type CustomPress = dyn Fn(CellId, u8, &Adjacency) -> Result<MoveVec, PressError>;

/// The eight knight's-move offsets.
pub const KNIGHT_JUMPS: [(i32, i32); 8] = [
    (1, 2),
    (1, -2),
    (-1, 2),
    (-1, -2),
    (2, 1),
    (2, -1),
    (-2, 1),
    (-2, -1),
];

/// A press shape on the lattice `\mathbb{Z}^2`, wrapped onto a surface by `PressPattern::lifted`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stencil {
    /// The pressed cell and the eight cells around it.
    Kings,
    /// The pressed cell and its eight knight's-move destinations.
    Knights,
    /// The pressed cell's whole row and whole column.
    RowColumn,
}

impl Stencil {
    /// Lattice points hit by pressing `(r, c)` on a `rows` by `cols` rectangle.
    /// Points may lie off the rectangle, and the pressed cell may be listed more than once since `Quotient2D::pressforward` counts it once regardless.
    pub fn targets(self, r: i32, c: i32, rows: i32, cols: i32) -> Vec<(i32, i32)> {
        match self {
            Stencil::Kings => (-1..=1)
                .flat_map(|dr| (-1..=1).map(move |dc| (r + dr, c + dc)))
                .collect(),
            Stencil::Knights => std::iter::once((0, 0))
                .chain(KNIGHT_JUMPS)
                .map(|(dr, dc)| (r + dr, c + dc))
                .collect(),
            Stencil::RowColumn => (0..cols)
                .map(|j| (r, j))
                .chain((0..rows).map(|i| (i, c)))
                .collect(),
        }
    }
}

/// How pressing a cell affects the state.
/// `Custom` is consumed at puzzle build time as the closure runs once per cell and the resulting move list is stored on the `Puzzle`.
pub enum PressPattern {
    /// Add `delta` to the pressed cell only.
    Singleton { delta: u8 },
    /// Add `delta` to the pressed cell and each of its open neighbours.
    ClosedNeighborhood { delta: u8 },
    /// Add `delta` to each open neighbour of the pressed cell, but not the cell itself.
    OpenNeighborhood { delta: u8 },
    /// Arbitrary press.
    /// The closure should produce a normalised `MoveVec` (e.g. via `MoveVec::new`) and is responsible for handling its own errors.
    /// Cells outside the adjacency are rejected by `materialize`.
    Custom(Box<CustomPress>),
}

impl PressPattern {
    /// `stencil` pressed on the lattice and pushed forward through `q`, adding `delta` per step.
    /// Build the puzzle on `Topology::Lifted2D(q)` so that the cell ids agree.
    pub fn lifted(q: Quotient2D, stencil: Stencil, delta: u8) -> Self {
        let (rows, cols) = (q.rows as i32, q.cols as i32);
        PressPattern::Custom(Box::new(move |cell, k, _| {
            q.pressforward(cell, k, delta, &|r, c| stencil.targets(r, c, rows, cols))
        }))
    }

    /// Produce one `MoveVec` per cell of `adj`, reduced modulo `k`.
    /// Errors if any move touches a cell outside `0..adj.n`.
    pub fn materialize(&self, k: u8, adj: &Adjacency) -> Result<Vec<MoveVec>, PressError> {
        (0..adj.n)
            .map(|i| {
                let mv = self.move_for(CellId(i), k, adj)?;
                // Deltas are sorted by cell, so the last one is the largest.
                match mv.deltas.last() {
                    Some(&(bad, _)) if bad.0 >= adj.n => {
                        Err(PressError::CellOutOfRange { n: adj.n, bad })
                    }
                    _ => Ok(mv),
                }
            })
            .collect()
    }

    pub fn move_for(&self, c: CellId, k: u8, adj: &Adjacency) -> Result<MoveVec, PressError> {
        match self {
            PressPattern::Singleton { delta } => MoveVec::new(k, vec![(c, *delta)]),
            PressPattern::ClosedNeighborhood { delta } => {
                let mut raw = vec![(c, *delta)];
                raw.extend(adj.neighbors[c.0].iter().map(|&nb| (nb, *delta)));
                MoveVec::new(k, raw)
            }
            PressPattern::OpenNeighborhood { delta } => {
                let raw = adj.neighbors[c.0].iter().map(|nb| (*nb, *delta)).collect();
                MoveVec::new(k, raw)
            }
            PressPattern::Custom(f) => f(c, k, adj),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::Topology;

    fn delta_cells(mv: &MoveVec) -> Vec<usize> {
        mv.deltas.iter().map(|(c, _)| c.0).collect()
    }

    #[test]
    fn normalisation_sums_dedups_reduces_strips() {
        // At cell 2: `1 + 2 = 3 \equiv 0 \pmod{3}` and is stripped.
        // Cell 0 keeps delta 2.
        let mv = MoveVec::new(3, vec![(CellId(2), 1), (CellId(0), 2), (CellId(2), 2)]).unwrap();
        assert_eq!(mv.deltas, vec![(CellId(0), 2)]);

        // Pure reduction modulo `k` without summing.
        let mv = MoveVec::new(2, vec![(CellId(1), 3), (CellId(0), 4)]).unwrap();
        assert_eq!(mv.deltas, vec![(CellId(1), 1)]);
    }

    #[test]
    fn closed_neighborhood_classic_three_by_three() {
        let adj =
            Topology::Product(Box::new(Topology::Path(3)), Box::new(Topology::Path(3))).adjacency();
        let moves = PressPattern::ClosedNeighborhood { delta: 1 }
            .materialize(2, &adj)
            .unwrap();
        assert_eq!(moves.len(), 9);
        // Center cell (1,1) at index 4 toggles itself + 4 neighbours = 5 cells.
        assert_eq!(moves[4].deltas.len(), 5);
        assert_eq!(delta_cells(&moves[4]), vec![1, 3, 4, 5, 7]);
        // Corner (0,0) toggles itself + 2 neighbours.
        assert_eq!(moves[0].deltas.len(), 3);
        assert_eq!(delta_cells(&moves[0]), vec![0, 1, 3]);
    }

    #[test]
    fn open_neighborhood_path_three() {
        let adj = Topology::Path(3).adjacency();
        let moves = PressPattern::OpenNeighborhood { delta: 1 }
            .materialize(2, &adj)
            .unwrap();
        assert_eq!(moves[0].deltas, vec![(CellId(1), 1)]);
        assert_eq!(moves[1].deltas, vec![(CellId(0), 1), (CellId(2), 1)]);
        assert_eq!(moves[2].deltas, vec![(CellId(1), 1)]);
    }

    #[test]
    fn singleton_neighborhood_classic_three_by_three() {
        let adj =
            Topology::Product(Box::new(Topology::Path(3)), Box::new(Topology::Path(3))).adjacency();
        let moves = PressPattern::Singleton { delta: 1 }
            .materialize(2, &adj)
            .unwrap();
        for (idx, mv) in moves.into_iter().enumerate() {
            assert_eq!(mv.deltas, vec![(CellId(idx), 1)]);
        }
    }

    #[test]
    fn three_state_press_doesnt_strip() {
        let adj =
            Topology::Product(Box::new(Topology::Path(3)), Box::new(Topology::Path(3))).adjacency();
        let moves = PressPattern::ClosedNeighborhood { delta: 1 }
            .materialize(3, &adj)
            .unwrap();
        assert_eq!(moves[4].deltas.len(), 5);
        for (_, d) in &moves[4].deltas {
            assert_eq!(*d, 1);
        }
    }

    #[test]
    fn custom_press_pattern() {
        // Pressing should flip itself plus the cell two steps clockwise on a cycle.
        let adj = Topology::Cycle(5).adjacency();
        let pattern = PressPattern::Custom(Box::new(|c: CellId, k: u8, adj: &Adjacency| {
            let other = (c.0 + 2) % adj.n;
            MoveVec::new(k, vec![(c, 1), (CellId(other), 1)])
        }));
        let moves = pattern.materialize(2, &adj).unwrap();
        assert_eq!(moves[0].deltas, vec![(CellId(0), 1), (CellId(2), 1)]);
        assert_eq!(moves[3].deltas, vec![(CellId(0), 1), (CellId(3), 1)]);
    }

    #[test]
    fn custom_press_out_of_range_is_rejected() {
        let adj = Topology::Path(3).adjacency();
        let pattern = PressPattern::Custom(Box::new(|c: CellId, k: u8, _: &Adjacency| {
            MoveVec::new(k, vec![(c, 1), (CellId(c.0 + 3), 1)])
        }));
        assert_eq!(
            pattern.materialize(2, &adj),
            Err(PressError::CellOutOfRange {
                n: 3,
                bad: CellId(3)
            })
        );
    }

    #[test]
    fn stencils_list_expected_offsets() {
        assert_eq!(Stencil::Kings.targets(0, 0, 3, 3).len(), 9);
        let knights = Stencil::Knights.targets(2, 2, 5, 5);
        assert_eq!(knights[0], (2, 2));
        assert_eq!(knights.len(), 9);
        // Row then column, with the pressed cell in both.
        assert_eq!(
            Stencil::RowColumn.targets(1, 0, 2, 2),
            vec![(1, 0), (1, 1), (0, 0), (1, 0)]
        );
    }

    #[test]
    fn movevec_new_rejects_zero_modulus() {
        assert_eq!(
            MoveVec::new(0, vec![(CellId(0), 1)]),
            Err(PressError::ZeroModulus)
        );
    }

    #[test]
    fn materialize_propagates_zero_modulus() {
        let adj = Topology::Path(3).adjacency();
        assert_eq!(
            PressPattern::ClosedNeighborhood { delta: 1 }.materialize(0, &adj),
            Err(PressError::ZeroModulus)
        );
    }
}
