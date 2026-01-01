pub mod layout;
mod modular;
pub mod press;
pub mod puzzle;
pub mod quotient2d;
pub mod solver;
pub mod state;
pub mod topology;

#[cfg(test)]
#[path = "../tests/common/mod.rs"]
mod test_support;

pub use layout::{Ghost, Layout, Rect};
pub use press::{KNIGHT_JUMPS, MoveVec, PressError, PressPattern, Stencil};
pub use puzzle::{MoveId, Puzzle, PuzzleError};
pub use quotient2d::{Generator, LatticeMap, Quotient2D, Quotient2DError};
pub use solver::{SolverError, solve};
pub use state::State;
pub use topology::{Adjacency, CellId, Gluing, Topology, TopologyError, canonicalize};
