use crate::modular::{add_mod, mul_mod};
use crate::press::{MoveVec, PressError, PressPattern};
use crate::state::State;
use crate::topology::{Topology, TopologyError};

/// Index into a `Puzzle`'s moves.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Ord, PartialOrd)]
pub struct MoveId(pub usize);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PuzzleError {
    Topology(TopologyError),
    Press(PressError),
    /// A state didn't have one value per cell.
    StateSizeMismatch {
        expected: usize,
        got: usize,
    },
    /// A state held a value outside `[0, k)`.
    CellValueOutOfRange {
        value: u8,
        modulus: u8,
    },
    /// Press counts didn't have one entry per move.
    CountsSizeMismatch {
        expected: usize,
        got: usize,
    },
    MoveOutOfRange {
        moves: usize,
        bad: MoveId,
    },
}

impl From<TopologyError> for PuzzleError {
    fn from(e: TopologyError) -> Self {
        PuzzleError::Topology(e)
    }
}

impl From<PressError> for PuzzleError {
    fn from(e: PressError) -> Self {
        PuzzleError::Press(e)
    }
}

/// A board of `cell_count` lights over `\mathbb{Z}/k\mathbb{Z}` and its moves.
/// The fields are private so that `k > 0` and every move stays inside the board, which lets states and moves be checked against the puzzle that made them.
#[derive(Clone, Debug)]
pub struct Puzzle {
    modulus: u8,
    n: usize,
    moves: Vec<MoveVec>,
}

impl Puzzle {
    /// `Topology -> Adjacency -> Vec<MoveVec> -> Puzzle`
    pub fn build(
        modulus: u8,
        topology: &Topology,
        press: &PressPattern,
    ) -> Result<Self, PuzzleError> {
        topology.validate()?;
        let adj = topology.adjacency();
        let moves = press.materialize(modulus, &adj)?;
        Ok(Puzzle {
            modulus,
            n: adj.n,
            moves,
        })
    }

    pub fn modulus(&self) -> u8 {
        self.modulus
    }

    pub fn cell_count(&self) -> usize {
        self.n
    }

    pub fn moves(&self) -> &[MoveVec] {
        &self.moves
    }

    pub fn zero_state(&self) -> State {
        State::from_cells_unchecked(vec![0; self.n])
    }

    /// A state of this puzzle, checking there is one value per cell and each is below the modulus.
    pub fn state(&self, cells: Vec<u8>) -> Result<State, PuzzleError> {
        let s = State::from_cells_unchecked(cells);
        self.check(&s)?;
        Ok(s)
    }

    /// Reject a state built for a different puzzle.
    pub(crate) fn check(&self, s: &State) -> Result<(), PuzzleError> {
        if s.len() != self.n {
            return Err(PuzzleError::StateSizeMismatch {
                expected: self.n,
                got: s.len(),
            });
        }
        match s.cells().iter().find(|&&v| v >= self.modulus) {
            Some(&value) => Err(PuzzleError::CellValueOutOfRange {
                value,
                modulus: self.modulus,
            }),
            None => Ok(()),
        }
    }

    fn move_at(&self, id: MoveId) -> Result<&MoveVec, PuzzleError> {
        self.moves.get(id.0).ok_or(PuzzleError::MoveOutOfRange {
            moves: self.moves.len(),
            bad: id,
        })
    }

    /// Press `id` once.
    pub fn apply(&self, s: &State, id: MoveId) -> Result<State, PuzzleError> {
        self.play(s, &[id])
    }

    /// Fold a sequence of presses into a final state.
    pub fn play(&self, s0: &State, ids: &[MoveId]) -> Result<State, PuzzleError> {
        self.check(s0)?;
        let mut s = s0.clone();
        for &id in ids {
            self.add_scaled(&mut s, self.move_at(id)?, 1);
        }
        Ok(s)
    }

    /// The state `M a` reached from zero by pressing move `j` `counts[j]` times.
    pub fn press_counts(&self, counts: &[u8]) -> Result<State, PuzzleError> {
        if counts.len() != self.moves.len() {
            return Err(PuzzleError::CountsSizeMismatch {
                expected: self.moves.len(),
                got: counts.len(),
            });
        }
        let mut s = self.zero_state();
        for (mv, &c) in self.moves.iter().zip(counts) {
            self.add_scaled(&mut s, mv, c % self.modulus);
        }
        Ok(s)
    }

    /// `s += times * mv` modulo `k`.
    /// The invariants on `Puzzle` keep every index in range.
    fn add_scaled(&self, s: &mut State, mv: &MoveVec, times: u8) {
        let k = self.modulus;
        let cells = s.cells_mut();
        for &(cell, delta) in &mv.deltas {
            cells[cell.0] = add_mod(cells[cell.0], mul_mod(delta, times, k), k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::{CellId, Gluing};

    fn square(n: usize) -> Topology {
        Topology::Product(Box::new(Topology::Path(n)), Box::new(Topology::Path(n)))
    }

    fn torus(n: usize) -> Topology {
        Topology::Product(Box::new(Topology::Cycle(n)), Box::new(Topology::Cycle(n)))
    }

    fn classic(k: u8, n: usize) -> Puzzle {
        Puzzle::build(
            k,
            &square(n),
            &PressPattern::ClosedNeighborhood { delta: 1 },
        )
        .unwrap()
    }

    #[test]
    fn classic_five_by_five() {
        let p = classic(2, 5);
        assert_eq!(p.modulus(), 2);
        assert_eq!(p.cell_count(), 25);
        assert_eq!(p.moves().len(), 25);
        // Center cell (2,2) at index 12 toggles 5 cells.
        assert_eq!(p.moves()[12].deltas.len(), 5);
        // Corner (0,0) toggles 3 cells; edge (0,2) toggles 4.
        assert_eq!(p.moves()[0].deltas.len(), 3);
        assert_eq!(p.moves()[2].deltas.len(), 4);
    }

    #[test]
    fn build_rejects_zero_modulus() {
        let err = Puzzle::build(
            0,
            &Topology::Path(3),
            &PressPattern::ClosedNeighborhood { delta: 1 },
        )
        .unwrap_err();
        assert_eq!(err, PuzzleError::Press(PressError::ZeroModulus));
    }

    #[test]
    fn build_rejects_invalid_topology() {
        let bad = Topology::Quotient(
            Box::new(Topology::Path(3)),
            Gluing(vec![vec![CellId(0), CellId(99)]]),
        );
        let err =
            Puzzle::build(2, &bad, &PressPattern::ClosedNeighborhood { delta: 1 }).unwrap_err();
        assert!(matches!(err, PuzzleError::Topology(_)));
    }

    #[test]
    fn build_works_on_torus() {
        let p =
            Puzzle::build(2, &torus(3), &PressPattern::ClosedNeighborhood { delta: 1 }).unwrap();
        assert_eq!(p.cell_count(), 9);
        // Every move on a torus toggles 5 cells (self + 4 neighbours).
        for mv in p.moves() {
            assert_eq!(mv.deltas.len(), 5);
        }
    }

    #[test]
    fn zero_state_is_solved() {
        assert!(classic(2, 3).zero_state().is_solved());
    }

    #[test]
    fn z2_double_press_is_identity() {
        let p = classic(2, 3);
        let pressed = p.apply(&p.zero_state(), MoveId(4)).unwrap();
        assert!(!pressed.is_solved());
        assert!(p.apply(&pressed, MoveId(4)).unwrap().is_solved());
    }

    #[test]
    fn z3_triple_press_is_identity() {
        let p = classic(3, 3);
        let once = p.apply(&p.zero_state(), MoveId(4)).unwrap();
        let twice = p.apply(&once, MoveId(4)).unwrap();
        let thrice = p.apply(&twice, MoveId(4)).unwrap();
        assert!(!once.is_solved());
        assert!(!twice.is_solved());
        assert!(thrice.is_solved());
    }

    #[test]
    fn play_folds_moves() {
        let p = classic(2, 3);
        let s0 = p.zero_state();
        // Pressing 0, 4, 0 in `\mathbb{Z}/2\mathbb{Z}` is the same as pressing 4.
        let folded = p.play(&s0, &[MoveId(0), MoveId(4), MoveId(0)]).unwrap();
        assert_eq!(folded, p.apply(&s0, MoveId(4)).unwrap());
    }

    #[test]
    fn press_counts_matches_repeated_play() {
        let p = classic(5, 3);
        let counts = [0, 1, 2, 3, 4, 5, 6, 7, 8];
        let ids: Vec<MoveId> = counts
            .iter()
            .enumerate()
            .flat_map(|(j, &c)| std::iter::repeat_n(MoveId(j), c as usize))
            .collect();
        assert_eq!(
            p.press_counts(&counts).unwrap(),
            p.play(&p.zero_state(), &ids).unwrap()
        );
    }

    #[test]
    fn foreign_states_and_moves_are_rejected() {
        let p = classic(3, 3);
        assert_eq!(
            p.state(vec![0; 4]),
            Err(PuzzleError::StateSizeMismatch {
                expected: 9,
                got: 4
            })
        );
        assert_eq!(
            p.state(vec![3; 9]),
            Err(PuzzleError::CellValueOutOfRange {
                value: 3,
                modulus: 3
            })
        );
        let other = classic(3, 2).zero_state();
        assert!(p.apply(&other, MoveId(0)).is_err());
        assert_eq!(
            p.apply(&p.zero_state(), MoveId(9)),
            Err(PuzzleError::MoveOutOfRange {
                moves: 9,
                bad: MoveId(9)
            })
        );
        assert!(p.press_counts(&[1; 8]).is_err());
    }
}
