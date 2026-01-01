//! # Building usual topologies
//!
//! ```text
//! SquareGrid n m  = Product(Path n,  Path m)              // n-by-m grid
//! Torus n m       = Product(Cycle n, Cycle m)             // wraps both axes
//! Cylinder n m    = Product(Cycle n, Path m)              // wraps one axis
//!
//! let grid        = Topology::Product(Box::new(Path(n)), Box::new(Path(m)));
//! let glue        = Gluing(
//!     (0..m).map(|j| vec![CellId(j), CellId((n - 1) * m + (m - 1 - j))]).collect()
//! );
//! let mobius  = Topology::Quotient(Box::new(grid), glue); // wraps one axis with reflection
//! ```

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TopologyError {
    /// A `Gluing` referenced a cell index outside its underlying topology.
    GluingOutOfRange { underlying_n: usize, bad: CellId },
    /// A `Quotient2D` collapse listed a cell outside its rectangle.
    Quotient2DCollapseOutOfRange { n: usize, bad: CellId },
}

impl From<crate::quotient2d::Quotient2DError> for TopologyError {
    fn from(e: crate::quotient2d::Quotient2DError) -> Self {
        match e {
            crate::quotient2d::Quotient2DError::CollapseOutOfRange { n, bad } => {
                TopologyError::Quotient2DCollapseOutOfRange { n, bad }
            }
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Ord, PartialOrd)]
pub struct CellId(pub usize);

/// A generating relation on cells.
/// Each inner `Vec` is a clause: all cells listed in it are declared equivalent.
///
/// Empty and singleton clauses are harmless no-ops.
/// Out-of-range `CellId`s lead to a `TopologyError` from `Topology::validate`.
#[derive(Clone, Debug, Default)]
pub struct Gluing(pub Vec<Vec<CellId>>);

#[derive(Clone, Debug)]
pub enum Topology {
    Empty,
    Single,
    Path(usize),
    Cycle(usize),
    Product(Box<Topology>, Box<Topology>),
    Sum(Box<Topology>, Box<Topology>),
    Quotient(Box<Topology>, Gluing),
    /// A 2-D quotient with explicit deck action and/or collapses.
    /// Carries the structure allowing press patterns to wrap across seams.
    Lifted2D(crate::quotient2d::Quotient2D),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Adjacency {
    pub n: usize,
    pub neighbors: Vec<Vec<CellId>>,
}

impl Topology {
    /// Total cell count, accounting for quotient collapses.
    pub fn cell_count(&self) -> usize {
        match self {
            Topology::Empty => 0,
            Topology::Single => 1,
            Topology::Path(n) | Topology::Cycle(n) => *n,
            Topology::Product(a, b) => a.cell_count() * b.cell_count(),
            Topology::Sum(a, b) => a.cell_count() + b.cell_count(),
            Topology::Quotient(t, g) => glue(t.cell_count(), g).canonical_labels().1,
            Topology::Lifted2D(q) => q.cell_count(),
        }
    }

    /// Full adjacency.
    pub fn adjacency(&self) -> Adjacency {
        match self {
            Topology::Empty => Adjacency {
                n: 0,
                neighbors: vec![],
            },
            Topology::Single => Adjacency {
                n: 1,
                neighbors: vec![vec![]],
            },
            Topology::Path(n) => path_adj(*n),
            Topology::Cycle(n) => cycle_adj(*n),
            Topology::Product(a, b) => product(&a.adjacency(), &b.adjacency()),
            Topology::Sum(a, b) => sum(&a.adjacency(), &b.adjacency()),
            Topology::Quotient(t, g) => quotient(&t.adjacency(), g),
            Topology::Lifted2D(q) => q.adjacency(),
        }
    }

    /// Recursively check that all `CellId`s in any `Gluing` are in range for their underlying topology.
    pub fn validate(&self) -> Result<(), TopologyError> {
        match self {
            Topology::Empty | Topology::Single | Topology::Path(_) | Topology::Cycle(_) => Ok(()),
            Topology::Product(a, b) | Topology::Sum(a, b) => {
                a.validate()?;
                b.validate()
            }
            Topology::Quotient(t, g) => {
                t.validate()?;
                let n = t.cell_count();
                match g.0.iter().flatten().find(|c| c.0 >= n) {
                    Some(&bad) => Err(TopologyError::GluingOutOfRange {
                        underlying_n: n,
                        bad,
                    }),
                    None => Ok(()),
                }
            }
            Topology::Lifted2D(q) => q.validate().map_err(Into::into),
        }
    }
}

/// In the 1d path adjacency matrix, node `m` is adjacent to nodes `m-1` and `m+1` when they exist
fn path_adj(n: usize) -> Adjacency {
    let mut neighbors = vec![Vec::new(); n];
    for (i, ns) in neighbors.iter_mut().enumerate() {
        if i > 0 {
            ns.push(CellId(i - 1));
        }
        if i + 1 < n {
            ns.push(CellId(i + 1));
        }
    }
    Adjacency { n, neighbors }
}

/// In the 1d cycle adjacency matrix, node `m` is adjacent to nodes `m-1` and `m+1` where addition is calculated in `\mathbb{Z}/n\mathbb{Z}`.
/// Like `Quotient2D::adjacency` this is a multigraph: `Cycle(2)` lists the other cell twice and `Cycle(1)` lists itself twice, matching a 2- or 1-row cylinder of width 1.
fn cycle_adj(n: usize) -> Adjacency {
    let mut neighbors = vec![Vec::new(); n];
    for (i, ns) in neighbors.iter_mut().enumerate() {
        // `+ n` to avoid underflow
        ns.extend([(i + n - 1) % n, (i + 1) % n].map(CellId));
        ns.sort();
    }
    Adjacency { n, neighbors }
}

/// Append an adjacency matrix to another.
fn sum(a: &Adjacency, b: &Adjacency) -> Adjacency {
    let mut neighbors = a.neighbors.clone();
    neighbors.extend(
        b.neighbors
            .iter()
            .map(|ns| ns.iter().map(|c| CellId(c.0 + a.n)).collect()),
    );
    Adjacency {
        n: a.n + b.n,
        neighbors,
    }
}

/// Graph Cartesian product.
/// Cells are pairs `(i, j)` linearised as `i * b.n + j` (row-major).
/// An edge connects two cells iff they agree on one coordinate and their other coordinates are adjacent in the corresponding factor.
fn product(a: &Adjacency, b: &Adjacency) -> Adjacency {
    let n = a.n * b.n;
    let mut neighbors = vec![Vec::new(); n];
    for i in 0..a.n {
        for j in 0..b.n {
            let idx = i * b.n + j;
            for ai in &a.neighbors[i] {
                neighbors[idx].push(CellId(ai.0 * b.n + j));
            }
            for bj in &b.neighbors[j] {
                neighbors[idx].push(CellId(i * b.n + bj.0));
            }
        }
    }
    Adjacency { n, neighbors }
}

/// Disjoint-set forest with path halving.
/// Used to perform quotienting over equivalence classes.
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    /// Fresh equivalence classes have only singletons
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    /// Find the representative of `x` and update all touched nodes along the path
    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    /// Place `a` and `b` into the same equivalence class.
    /// Chooses `b`'s representative as the new representative for the class.
    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[ra] = rb;
        }
    }

    /// Determine a "minimal" representative for each element and the number of classes
    pub fn canonical_labels(&mut self) -> (Vec<usize>, usize) {
        let n = self.parent.len();
        let mut root_to_label = vec![usize::MAX; n];
        let mut labels = Vec::with_capacity(n);
        let mut classes = 0;
        for i in 0..n {
            let repr = self.find(i);
            if root_to_label[repr] == usize::MAX {
                root_to_label[repr] = classes;
                classes += 1;
            }
            labels.push(root_to_label[repr]);
        }
        (labels, classes)
    }
}

/// Reduce `0..n` modulo the equivalence relation generated by `gluing`.
pub fn canonicalize(n: usize, gluing: &Gluing) -> (Vec<usize>, usize) {
    glue(n, gluing).canonical_labels()
}

/// Build a union-find on `0..n` whose classes are the transitive closure of the gluing's clauses.
/// Empty and singleton clauses are no-ops.
fn glue(n: usize, gluing: &Gluing) -> UnionFind {
    let mut uf = UnionFind::new(n);
    for class in &gluing.0 {
        if let Some((first, rest)) = class.split_first() {
            for c in rest {
                uf.union(first.0, c.0);
            }
        }
    }
    uf
}

/// Construct an fresh adjacency by performing a "gluing" associating nodes in the same equivalence class to a single representative node
fn quotient(adj: &Adjacency, g: &Gluing) -> Adjacency {
    let (reprs, classes) = glue(adj.n, g).canonical_labels();
    let mut neighbors = vec![Vec::new(); classes];
    for i in 0..adj.n {
        let src = reprs[i];
        for neighbor in &adj.neighbors[i] {
            let dst = reprs[neighbor.0];
            // Drop edges crushed by the gluing, i.e. between two different nodes of one class, as `Quotient2D` does for collapses.
            // Loops already in the underlying graph join a node to itself and are kept.
            if dst != src || neighbor.0 == i {
                neighbors[src].push(CellId(dst));
            }
        }
    }
    // Parallel edges produced by gluing are kept, so the result is a multigraph like `Quotient2D::adjacency`.
    for v in &mut neighbors {
        v.sort();
    }
    Adjacency {
        n: classes,
        neighbors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_cells(ids: &[usize]) -> Vec<CellId> {
        ids.iter().copied().map(CellId).collect()
    }

    #[test]
    fn path_three() {
        let adj = Topology::Path(3).adjacency();
        assert_eq!(adj.n, 3);
        assert_eq!(adj.neighbors[0], to_cells(&[1]));
        assert_eq!(adj.neighbors[1], to_cells(&[0, 2]));
        assert_eq!(adj.neighbors[2], to_cells(&[1]));
    }

    #[test]
    fn cycle_edge_cases() {
        assert_eq!(Topology::Cycle(0).adjacency(), Topology::Empty.adjacency());
        // `Cycle(1)` is a single cell with a loop in each direction.
        assert_eq!(
            Topology::Cycle(1).adjacency().neighbors,
            vec![to_cells(&[0, 0])]
        );
        // `Cycle(2)` keeps its two parallel edges.
        assert_eq!(
            Topology::Cycle(2).adjacency().neighbors,
            vec![to_cells(&[1, 1]), to_cells(&[0, 0])]
        );
    }

    #[test]
    fn cycle_matches_one_column_cylinder() {
        // An `n` by 1 cylinder (rows wrap, one column) is the cycle of length `n`, multi-edges and loops included.
        for n in 1..6 {
            assert_eq!(
                Topology::Cycle(n).adjacency(),
                crate::quotient2d::Quotient2D::cylinder(n, 1).adjacency(),
                "n = {n}"
            );
        }
    }

    #[test]
    fn square_three_by_three() {
        let t = Topology::Product(Box::new(Topology::Path(3)), Box::new(Topology::Path(3)));
        let adj = t.adjacency();
        assert_eq!(adj.n, 9);
        let mut center = adj.neighbors[4].clone();
        center.sort();
        assert_eq!(center, to_cells(&[1, 3, 5, 7]));
        let mut corner = adj.neighbors[0].clone();
        corner.sort();
        assert_eq!(corner, to_cells(&[1, 3]));
    }

    #[test]
    fn torus_three_by_three_uniform_degree() {
        let t = Topology::Product(Box::new(Topology::Cycle(3)), Box::new(Topology::Cycle(3)));
        let adj = t.adjacency();
        assert_eq!(adj.n, 9);
        for ns in &adj.neighbors {
            assert_eq!(ns.len(), 4);
        }
    }

    #[test]
    fn sum_disjoint_union() {
        let t = Topology::Sum(Box::new(Topology::Path(2)), Box::new(Topology::Single));
        let adj = t.adjacency();
        assert_eq!(adj.n, 3);
        assert_eq!(adj.neighbors[0], to_cells(&[1]));
        assert_eq!(adj.neighbors[1], to_cells(&[0]));
        assert_eq!(adj.neighbors[2], to_cells(&[]));
    }

    #[test]
    fn quotient_collapses_endpoints_to_cycle_two() {
        let g = Gluing(vec![vec![CellId(0), CellId(2)]]);
        let t = Topology::Quotient(Box::new(Topology::Path(3)), g);
        assert_eq!(t.adjacency(), Topology::Cycle(2).adjacency());
        assert_eq!(t.cell_count(), 2);
    }

    #[test]
    fn cell_count_handles_overlapping_clauses() {
        // `[[0,1], [1,2]]` should transitively merge all three.
        let g = Gluing(vec![vec![CellId(0), CellId(1)], vec![CellId(1), CellId(2)]]);
        let t = Topology::Quotient(Box::new(Topology::Path(3)), g);
        assert_eq!(t.cell_count(), 1);
        assert_eq!(t.adjacency().n, 1);
    }

    #[test]
    fn cell_count_handles_redundant_clauses() {
        let g = Gluing(vec![vec![CellId(0), CellId(1)], vec![CellId(0), CellId(1)]]);
        let t = Topology::Quotient(Box::new(Topology::Path(3)), g);
        assert_eq!(t.cell_count(), 2);
        assert_eq!(t.adjacency().n, 2);
    }

    #[test]
    fn cell_count_matches_adjacency_n() {
        let t = Topology::Product(Box::new(Topology::Path(4)), Box::new(Topology::Cycle(5)));
        assert_eq!(t.cell_count(), t.adjacency().n);
    }

    #[test]
    fn validate_catches_out_of_range_gluing() {
        let g = Gluing(vec![vec![CellId(0), CellId(99)]]);
        let t = Topology::Quotient(Box::new(Topology::Path(3)), g);
        assert_eq!(
            t.validate(),
            Err(TopologyError::GluingOutOfRange {
                underlying_n: 3,
                bad: CellId(99)
            })
        );
    }

    #[test]
    fn validate_passes_for_in_range_gluing() {
        let g = Gluing(vec![vec![CellId(0), CellId(2)]]);
        let t = Topology::Quotient(Box::new(Topology::Path(3)), g);
        assert!(t.validate().is_ok());
    }
}
