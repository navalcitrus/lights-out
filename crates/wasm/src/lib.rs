//! wasm-bindgen surface for `lights-out-core`.

use std::collections::BTreeSet;
use std::fmt::Debug;

use lights_out_core::{
    Adjacency, Layout, MoveId, PressPattern, Puzzle as CorePuzzle, Quotient2D, State, Stencil,
    Topology, solve as core_solve,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
#[derive(Copy, Clone)]
pub enum TopologyKind {
    /// Open square grid
    Grid,
    /// Torus has both axes wrap
    Torus,
    /// Cylinder has rows wrap but cols don't
    Cylinder,
    /// Mobius strip has rows wrap with a twist.
    Mobius,
    /// Klein bottle has both axes wrap, but rows twist
    Klein,
    /// Real projective plane has both axes wrap with a twist
    Projective,
    /// Sphere has the entire boundary of the rectangle collapse to a single point
    Sphere,
    /// 1-D path has no wrapping
    Path,
    /// 1-D cycle has wrapping
    Cycle,
}

#[wasm_bindgen]
#[derive(Copy, Clone)]
pub enum PressKind {
    /// Self + open neighbours
    Closed,
    /// Open neighbours only.
    /// Pressed cell does not toggle itself.
    Open,
    /// Self + 8 surrounding cells (king's-move).
    /// 2-D only.
    Kings,
    /// Self + 8 chess-knight destinations.
    /// 2-D only.
    Knights,
    /// Entire row + entire column of the pressed cell.
    /// 2-D only.
    RowColumn,
}

#[wasm_bindgen]
pub struct Puzzle {
    inner: CorePuzzle,
    board: Board,
    layout: Layout,
    /// Undirected adjacency edges as flat `(a, b)` pairs with `a < b`.
    /// Cached since `topology().adjacency()` is non-trivial for `Lifted2D`.
    edges: Vec<u32>,
}

#[wasm_bindgen]
impl Puzzle {
    pub fn build(
        topology: TopologyKind,
        press: PressKind,
        rows: usize,
        cols: usize,
        k: u8,
    ) -> Result<Puzzle, JsError> {
        let board = board_for(topology, rows, cols)?;
        let press_pattern = press_for(press, &board)?;
        let topo = board.topology();
        let layout = topo.layout();
        let edges = adjacency_edges(&topo.adjacency());
        let inner = CorePuzzle::build(k, &topo, &press_pattern).map_err(js_err)?;
        Ok(Puzzle {
            inner,
            board,
            layout,
            edges,
        })
    }

    #[wasm_bindgen(getter, js_name = moveCount)]
    pub fn move_count(&self) -> usize {
        self.inner.moves().len()
    }

    #[wasm_bindgen(getter)]
    pub fn modulus(&self) -> u8 {
        self.inner.modulus()
    }

    #[wasm_bindgen(getter, js_name = visualRows)]
    pub fn visual_rows(&self) -> usize {
        self.board.visual_rows()
    }

    #[wasm_bindgen(getter, js_name = visualCols)]
    pub fn visual_cols(&self) -> usize {
        self.board.visual_cols()
    }

    #[wasm_bindgen(js_name = visualLayout)]
    pub fn visual_layout(&self) -> Box<[u32]> {
        self.board.visual_layout().into_boxed_slice()
    }

    /// Canonical 2-D drawing position per cell, interleaved `[x0, y0, x1, y1, ...]`.
    #[wasm_bindgen(js_name = layoutPositions)]
    pub fn layout_positions(&self) -> Box<[f32]> {
        self.layout
            .positions
            .iter()
            .flat_map(|&(x, y)| [x, y])
            .collect()
    }

    /// Drawing positions of ghosts, interleaved `[x0, y0, x1, y1, ...]`.
    #[wasm_bindgen(js_name = layoutGhostXY)]
    pub fn layout_ghost_xy(&self) -> Box<[f32]> {
        self.layout
            .ghosts
            .iter()
            .flat_map(|g| [g.xy.0, g.xy.1])
            .collect()
    }

    /// Canonical cell id of each ghost (parallel to `layoutGhostXY`).
    #[wasm_bindgen(js_name = layoutGhostCells)]
    pub fn layout_ghost_cells(&self) -> Box<[u32]> {
        self.layout.ghosts.iter().map(|g| g.cell.0 as u32).collect()
    }

    /// Bounding box `[minX, minY, maxX, maxY]` of canonical positions.
    #[wasm_bindgen(js_name = layoutBounds)]
    pub fn layout_bounds(&self) -> Box<[f32]> {
        let b = self.layout.bounds;
        Box::new([b.min.0, b.min.1, b.max.0, b.max.1])
    }

    /// Undirected adjacency edges as flat pairs `[a0, b0, a1, b1, ...]` with `a < b`.
    #[wasm_bindgen(js_name = adjacencyEdges)]
    pub fn adjacency_edges(&self) -> Box<[u32]> {
        self.edges.clone().into_boxed_slice()
    }

    /// Glued boundary steps of a 2-D board as flat `[src, dr, dc, dst, ...]` quadruples.
    /// Cell ids are raw `row * cols + col` positions in the visual grid, and the list is empty for 1-D boards.
    #[wasm_bindgen(js_name = seamSteps)]
    pub fn seam_steps(&self) -> Box<[i32]> {
        self.board
            .quotient2d()
            .map(|q| q.seams())
            .unwrap_or_default()
            .into_iter()
            .flat_map(|(src, (dr, dc), dst)| [src as i32, dr, dc, dst as i32])
            .collect()
    }

    #[wasm_bindgen(js_name = zeroState)]
    pub fn zero_state(&self) -> Box<[u8]> {
        self.inner.zero_state().into_cells().into_boxed_slice()
    }

    /// Apply move `move_id` to `state`, returning the new state.
    pub fn apply(&self, state: &[u8], move_id: usize) -> Result<Box<[u8]>, JsError> {
        let s = self.state(state)?;
        let next = self.inner.apply(&s, MoveId(move_id)).map_err(js_err)?;
        Ok(next.into_cells().into_boxed_slice())
    }

    /// Press move `j` `counts[j]` times from the zero state, returning `M a` where `M` is the move matrix and `a` is the press-count vector.
    pub fn scramble(&self, counts: &[u8]) -> Result<Box<[u8]>, JsError> {
        let s = self.inner.press_counts(counts).map_err(js_err)?;
        Ok(s.into_cells().into_boxed_slice())
    }

    #[wasm_bindgen(js_name = isSolved)]
    pub fn is_solved(&self, state: &[u8]) -> Result<bool, JsError> {
        Ok(self.state(state)?.is_solved())
    }

    pub fn solve(&self, s0: &[u8], target: &[u8]) -> Result<Option<Vec<u32>>, JsError> {
        let s0 = self.state(s0)?;
        let target = self.state(target)?;
        let plan = core_solve(&self.inner, &s0, &target).map_err(js_err)?;
        Ok(plan.map(|p| p.into_iter().map(|MoveId(i)| i as u32).collect()))
    }

    /// Checks the length and that every cell is below the modulus.
    /// Without the latter a cell holding `k` would be unsolved to `isSolved` but zero to `solve`.
    fn state(&self, s: &[u8]) -> Result<State, JsError> {
        self.inner.state(s.to_vec()).map_err(js_err)
    }
}

/// How a board is laid out: either a 2-D rectangle quotiented by some group of isometries (every 2-D topology), or a 1-D line of cells (Path / Cycle).
enum Board {
    Lifted(Quotient2D),
    Linear { topo: Topology, len: usize },
}

impl Board {
    fn topology(&self) -> Topology {
        match self {
            Board::Lifted(q) => Topology::Lifted2D(q.clone()),
            Board::Linear { topo, .. } => topo.clone(),
        }
    }

    fn visual_rows(&self) -> usize {
        match self {
            Board::Lifted(q) => q.rows,
            Board::Linear { .. } => 1,
        }
    }

    fn visual_cols(&self) -> usize {
        match self {
            Board::Lifted(q) => q.cols,
            Board::Linear { len, .. } => *len,
        }
    }

    fn visual_layout(&self) -> Vec<u32> {
        match self {
            Board::Lifted(q) => q.collapse_canon().iter().map(|&i| i as u32).collect(),
            Board::Linear { len, .. } => (0..*len as u32).collect(),
        }
    }

    fn quotient2d(&self) -> Option<&Quotient2D> {
        match self {
            Board::Lifted(q) => Some(q),
            Board::Linear { .. } => None,
        }
    }
}

fn board_for(kind: TopologyKind, rows: usize, cols: usize) -> Result<Board, JsError> {
    Ok(match kind {
        TopologyKind::Grid => Board::Lifted(Quotient2D::grid(rows, cols)),
        TopologyKind::Torus => Board::Lifted(Quotient2D::torus(rows, cols)),
        TopologyKind::Cylinder => Board::Lifted(Quotient2D::cylinder(rows, cols)),
        TopologyKind::Mobius => {
            if rows < 2 {
                return Err(JsError::new("Mobius requires rows ≥ 2"));
            }
            Board::Lifted(Quotient2D::mobius(rows, cols))
        }
        TopologyKind::Klein => {
            require_2d(rows, cols, "Klein bottle")?;
            Board::Lifted(Quotient2D::klein(rows, cols))
        }
        TopologyKind::Projective => {
            require_2d(rows, cols, "Projective plane")?;
            Board::Lifted(Quotient2D::projective_plane(rows, cols))
        }
        TopologyKind::Sphere => {
            require_2d(rows, cols, "Sphere")?;
            Board::Lifted(Quotient2D::sphere(rows, cols))
        }
        TopologyKind::Path => Board::Linear {
            topo: Topology::Path(rows),
            len: rows,
        },
        TopologyKind::Cycle => Board::Linear {
            topo: Topology::Cycle(rows),
            len: rows,
        },
    })
}

fn require_2d(rows: usize, cols: usize, name: &str) -> Result<(), JsError> {
    if rows < 2 || cols < 2 {
        return Err(JsError::new(&format!(
            "{name} requires rows ≥ 2 and cols ≥ 2"
        )));
    }
    Ok(())
}

/// Flatten an undirected adjacency to `(a, b)` pairs with `a < b` so each pair of neighbours is drawn once.
/// The adjacency is a multigraph, so parallel edges are merged here since they would overlap in the drawing anyway.
fn adjacency_edges(adj: &Adjacency) -> Vec<u32> {
    let pairs: BTreeSet<(usize, usize)> = adj
        .neighbors
        .iter()
        .enumerate()
        .flat_map(|(i, ns)| ns.iter().filter(move |n| n.0 > i).map(move |n| (i, n.0)))
        .collect();
    pairs
        .into_iter()
        .flat_map(|(a, b)| [a as u32, b as u32])
        .collect()
}

fn press_for(kind: PressKind, board: &Board) -> Result<PressPattern, JsError> {
    let lifted = |stencil| {
        let q = board
            .quotient2d()
            .ok_or_else(|| JsError::new("this press pattern requires a 2-D topology"))?;
        Ok(PressPattern::lifted(q.clone(), stencil, 1))
    };
    match kind {
        PressKind::Closed => Ok(PressPattern::ClosedNeighborhood { delta: 1 }),
        PressKind::Open => Ok(PressPattern::OpenNeighborhood { delta: 1 }),
        PressKind::Kings => lifted(Stencil::Kings),
        PressKind::Knights => lifted(Stencil::Knights),
        PressKind::RowColumn => lifted(Stencil::RowColumn),
    }
}

fn js_err<E: Debug>(e: E) -> JsError {
    JsError::new(&format!("{:?}", e))
}
