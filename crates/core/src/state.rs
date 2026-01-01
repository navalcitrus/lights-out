/// The puzzle state as a column vector (for solver purposes).
/// Built by a `Puzzle`, which checks that there is one value per cell and that each is below the modulus.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct State(Vec<u8>);

impl State {
    /// Callers must have checked the cells against their puzzle.
    pub(crate) fn from_cells_unchecked(cells: Vec<u8>) -> Self {
        State(cells)
    }

    pub fn cells(&self) -> &[u8] {
        &self.0
    }

    pub fn into_cells(self) -> Vec<u8> {
        self.0
    }

    pub(crate) fn cells_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }

    pub fn is_solved(&self) -> bool {
        self.0.iter().all(|&c| c == 0)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
