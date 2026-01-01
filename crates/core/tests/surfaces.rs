mod common;

use common::Lcg;
use lights_out_core::{
    CellId, MoveVec, PressPattern, Puzzle, Quotient2D, State, Stencil, Topology, solve,
};
use std::collections::HashSet;

type Build = fn(usize, usize) -> Quotient2D;

const SURFACES: [(&str, Build); 7] = [
    ("grid", Quotient2D::grid),
    ("cylinder", Quotient2D::cylinder),
    ("mobius", Quotient2D::mobius),
    ("torus", Quotient2D::torus),
    ("klein", Quotient2D::klein),
    ("projective", Quotient2D::projective_plane),
    ("sphere", Quotient2D::sphere),
];

fn patterns(q: &Quotient2D) -> Vec<(&'static str, PressPattern)> {
    let lifted = |stencil| PressPattern::lifted(q.clone(), stencil, 1);
    vec![
        ("closed", PressPattern::ClosedNeighborhood { delta: 1 }),
        ("open", PressPattern::OpenNeighborhood { delta: 1 }),
        ("kings", lifted(Stencil::Kings)),
        ("knights", lifted(Stencil::Knights)),
        ("row+col", lifted(Stencil::RowColumn)),
    ]
}

/// Pack a state (at most 16 cells, `k <= 16`) into a nibble per cell.
fn pack(cells: &[u8]) -> u64 {
    assert!(cells.len() <= 16);
    cells.iter().rev().fold(0, |acc, &v| (acc << 4) | v as u64)
}

/// Every packed state reachable from zero, by closing `{0}` under adding move columns.
fn reachable(p: &Puzzle) -> HashSet<u64> {
    let k = p.modulus() as u64;
    let columns: Vec<Vec<(usize, u64)>> = p
        .moves()
        .iter()
        .map(|mv| {
            mv.deltas
                .iter()
                .map(|&(c, d)| (4 * c.0, d as u64))
                .collect()
        })
        .collect();
    let mut seen = HashSet::from([0u64]);
    let mut frontier = vec![0u64];
    while let Some(s) = frontier.pop() {
        for col in &columns {
            let t = col.iter().fold(s, |t, &(shift, d)| {
                let v = ((t >> shift) & 0xF) + d;
                (t & !(0xF << shift)) | ((v % k) << shift)
            });
            if seen.insert(t) {
                frontier.push(t);
            }
        }
    }
    seen
}

fn check_plan(p: &Puzzle, s0: &State, target: &State, label: &str) -> bool {
    match solve(p, s0, target).unwrap() {
        Some(plan) => {
            assert_eq!(&p.play(s0, &plan).unwrap(), target, "bad plan: {label}");
            true
        }
        None => false,
    }
}

#[test]
fn solver_agrees_with_brute_force() {
    let mut lcg = Lcg::new(0x5EED);
    for (rows, cols, ks) in [(3, 3, &[2u8, 3, 4][..]), (3, 4, &[2][..]), (4, 4, &[2][..])] {
        for (name, build) in SURFACES {
            let q = build(rows, cols);
            for &k in ks {
                for (press, pattern) in patterns(&q) {
                    let label = format!("{name} {rows}x{cols} {press} k={k}");
                    let p = Puzzle::build(k, &Topology::Lifted2D(q.clone()), &pattern).unwrap();
                    let n = p.cell_count();
                    let image = reachable(&p);
                    for _ in 0..64 {
                        let s0 = p.state(lcg.vec(n, k)).unwrap();
                        let b = p.state(lcg.vec(n, k)).unwrap();
                        // `s0 -> b` is solvable iff `b - s0` is reachable from zero.
                        let diff: Vec<u8> = (b.cells().iter().zip(s0.cells()))
                            .map(|(x, y)| (x + k - y) % k)
                            .collect();
                        assert_eq!(
                            check_plan(&p, &s0, &b, &label),
                            image.contains(&pack(&diff)),
                            "solvability disagrees with brute force: {label}"
                        );
                    }
                    // Random reachable targets must always be solved.
                    for t in image.into_iter().take(64) {
                        let t = p.state((0..n).map(|i| ((t >> (4 * i)) & 0xF) as u8).collect());
                        assert!(check_plan(&p, &p.zero_state(), &t.unwrap(), &label));
                    }
                }
            }
        }
    }
}

#[test]
fn solver_round_trips_composite_moduli() {
    // Prime powers (Hensel) and mixed factors (CRT) on every surface.
    let mut lcg = Lcg::new(0xC0FFEE);
    for (name, build) in SURFACES {
        let q = build(4, 5);
        for k in [4u8, 6, 8, 9, 12, 25, 30, 255] {
            for (press, pattern) in patterns(&q) {
                let label = format!("{name} 4x5 {press} k={k}");
                let p = Puzzle::build(k, &Topology::Lifted2D(q.clone()), &pattern).unwrap();
                let target = p.press_counts(&lcg.vec(p.moves().len(), k)).unwrap();
                assert!(
                    check_plan(&p, &p.zero_state(), &target, &label),
                    "reachable target reported unsolvable: {label}"
                );
            }
        }
    }
}

#[test]
fn row_column_toggles_pressed_cell_once() {
    // The pressed cell is in both its row and its column.
    // It must still be hit exactly once, otherwise it never toggles modulo `2`.
    let q = Quotient2D::grid(3, 3);
    let pattern = PressPattern::lifted(q.clone(), Stencil::RowColumn, 1);
    let p = Puzzle::build(2, &Topology::Lifted2D(q), &pattern).unwrap();
    let cells: Vec<usize> = p.moves()[4]
        .deltas
        .iter()
        .map(|(CellId(c), _)| *c)
        .collect();
    assert_eq!(cells, vec![1, 3, 4, 5, 7]);
}

#[test]
fn projective_corner_is_pressed_once_per_edge() {
    // On the default 5x5 projective plane, cell 1 (index 0) reaches cell 25 (index 24) across both the top and the left seam.
    // Those are two different edges, so a closed press moves cell 25 by 2.
    let q = Quotient2D::projective_plane(5, 5);
    let p = Puzzle::build(
        3,
        &Topology::Lifted2D(q),
        &PressPattern::ClosedNeighborhood { delta: 1 },
    )
    .unwrap();
    assert!(p.moves()[0].deltas.contains(&(CellId(24), 2)));
    assert!(p.moves()[0].deltas.contains(&(CellId(0), 1)));
}

#[test]
fn symmetric_patterns_give_symmetric_move_matrices() {
    // Every pattern here is symmetric under `(dr, dc) \mapsto (-dr, -dc)`, so each step from `a` to `b` has a reverse step from `b` to `a`.
    // Counting every step therefore means pressing `a` moves `b` exactly as much as pressing `b` moves `a`.
    for (name, build) in SURFACES {
        for (rows, cols) in [(3, 3), (4, 5), (5, 5), (6, 4)] {
            let q = build(rows, cols);
            for (press, pattern) in patterns(&q) {
                let p = Puzzle::build(251, &Topology::Lifted2D(q.clone()), &pattern).unwrap();
                let moves: &[MoveVec] = p.moves();
                for (j, mv) in moves.iter().enumerate() {
                    for &(CellId(i), d) in &mv.deltas {
                        assert!(
                            moves[i].deltas.contains(&(CellId(j), d)),
                            "{name} {rows}x{cols} {press}: pressing {j} moves {i} by {d}, but not the reverse"
                        );
                    }
                }
            }
        }
    }
}
