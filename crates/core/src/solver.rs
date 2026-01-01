//! Solving a generalised lights-out puzzle is just linear algebra over `\mathbb{Z}/k\mathbb{Z}`.
//! The puzzle's `moves` form columns of an `n` by `m` matrix `M` and our question is "given a target state difference `b`, find a press-count vector `a` such that `M a \equiv b \pmod{k}`?".

use crate::modular::{mod_inverse, mul_mod, sub_mod};
use crate::press::MoveVec;
use crate::puzzle::{MoveId, Puzzle, PuzzleError};
use crate::state::State;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SolverError {
    /// A state didn't belong to the puzzle.
    Puzzle(PuzzleError),
    /// CRT inputs weren't pairwise coprime.
    NonCoprime(u8),
    /// CRT product would have overflowed `u8`.
    ProductOverflow,
}

impl From<PuzzleError> for SolverError {
    fn from(e: PuzzleError) -> Self {
        SolverError::Puzzle(e)
    }
}

/// Find a sequence of presses taking `s0` to `target`, or `None` if unreachable.
/// The returned plan is a flat `Vec<MoveId>` where `MoveId(j)` appears `presses[j]` times.
/// The goal is that reapplying the plan via `Puzzle::play` reproduces `target`.
pub fn solve(
    puzzle: &Puzzle,
    s0: &State,
    target: &State,
) -> Result<Option<Vec<MoveId>>, SolverError> {
    puzzle.check(s0)?;
    puzzle.check(target)?;
    let n = puzzle.cell_count();
    let moves = puzzle.moves();

    // Every state is the zero state modulo 1.
    let k = puzzle.modulus();
    if k == 1 {
        return Ok(Some(Vec::new()));
    }

    let mat = build_dense_move_matrix(moves, n);

    // Solve modulo each prime power `p^e` exactly dividing `k`, then recombine with CRT.
    let prime_powers: Vec<(u8, u8, u8)> = prime_factors(k)
        .into_iter()
        .map(|(p, e)| (p, e, p.pow(e as u32)))
        .collect();
    let b = vec_diff_mod_k(target.cells(), s0.cells(), k);
    let residues: Option<Vec<Vec<u8>>> = prime_powers
        .iter()
        .map(|&(p, e, pe)| {
            let mat_pe = mat_mod(&mat, pe);
            let b_pe = vec_mod(&b, pe);
            let sol = solve_zpe(&mat_pe, &b_pe, p, e)?;
            debug_assert_eq!(
                matvec_mod(&mat_pe, &sol, pe),
                b_pe,
                "per-prime-power solve returned a non-solution (modulo {pe})",
            );
            Some(sol)
        })
        .collect();
    let Some(residues) = residues else {
        return Ok(None);
    };

    let presses: Vec<u8> = (0..moves.len())
        .map(|j| {
            let eq: Vec<(u8, u8)> = residues
                .iter()
                .zip(&prime_powers)
                .map(|(a, &(_, _, pe))| (a[j], pe))
                .collect();
            forward_crt(&eq).map(|(x, _)| x)
        })
        .collect::<Result<_, _>>()?;

    // Re-expand solution into something that `play` can consume
    let plan: Vec<MoveId> = presses
        .iter()
        .enumerate()
        .flat_map(|(j, &c)| std::iter::repeat_n(MoveId(j), c as usize))
        .collect();
    Ok(Some(plan))
}

/// Returns the prime factors of `n` as `(prime, multiplicity)`.
/// Trial division by primes up to `13` suffices since `17^2 > 255`, so whatever remains is `1` or prime.
fn prime_factors(mut n: u8) -> Vec<(u8, u8)> {
    let mut factors = Vec::new();
    for p in [2, 3, 5, 7, 11, 13] {
        let mut e = 0;
        while n > 0 && n.is_multiple_of(p) {
            e += 1;
            n /= p;
        }
        if e > 0 {
            factors.push((p, e));
        }
    }
    if n > 1 {
        factors.push((n, 1));
    }
    factors
}

/// Takes `(remainder, modulus)` pairs and returns the unique `(x, product)` with `x \equiv r_i \pmod{m_i}` for every input.
/// Errors if the moduli aren't pairwise coprime, or if their product would overflow `u8`.
fn forward_crt(equivalences: &[(u8, u8)]) -> Result<(u8, u8), SolverError> {
    let total_m_check: u32 = equivalences.iter().map(|&(_, m)| m as u32).product();
    if total_m_check > 255 {
        return Err(SolverError::ProductOverflow);
    }
    let total_m = total_m_check as u8;

    let x = equivalences.iter().try_fold(0u32, |acc, &(a, m)| {
        let m_i = total_m / m;
        let inv = mod_inverse(m_i, m).ok_or(SolverError::NonCoprime(m))?;
        Ok::<u32, SolverError>(acc + a as u32 * m_i as u32 * inv as u32)
    })?;

    Ok((x.rem_euclid(total_m as u32) as u8, total_m))
}

/// Componentwise `(a - b) \bmod k`.
fn vec_diff_mod_k(a: &[u8], b: &[u8], k: u8) -> Vec<u8> {
    a.iter()
        .zip(b)
        .map(|(&ai, &bi)| sub_mod(ai, bi, k))
        .collect()
}

fn vec_mod(v: &[u8], m: u8) -> Vec<u8> {
    v.iter().map(|&x| x % m).collect()
}

fn mat_mod(mat: &[Vec<u8>], m: u8) -> Vec<Vec<u8>> {
    mat.iter().map(|row| vec_mod(row, m)).collect()
}

/// `M v \bmod m`.
fn matvec_mod(mat: &[Vec<u8>], v: &[u8], m: u8) -> Vec<u8> {
    let m_u32 = m as u32;
    mat.iter()
        .map(|row| {
            let s: u32 = row
                .iter()
                .zip(v.iter())
                .map(|(&a, &b)| a as u32 * b as u32)
                .sum();
            (s % m_u32) as u8
        })
        .collect()
}

/// Transform sparse `MoveVec` columns into a dense `n` by `moves.len()` matrix `mat[cell][move]`.
fn build_dense_move_matrix(moves: &[MoveVec], n: usize) -> Vec<Vec<u8>> {
    let mut mat = vec![vec![0u8; moves.len()]; n];
    for (j, mv) in moves.iter().enumerate() {
        for &(cell, delta) in &mv.deltas {
            mat[cell.0][j] = delta;
        }
    }
    mat
}

/// An integer modulo `p^e` that has been checked to be divisible by `p`.
#[derive(Clone, Copy, Debug)]
struct MultipleOfP {
    p: u8,
    x: u8,
}

impl MultipleOfP {
    fn new(x: u8, p: u8) -> Option<Self> {
        x.is_multiple_of(p).then_some(Self { p, x })
    }

    /// Integer-divide by `p`.
    /// Forced to be exact.
    fn shift_down(self) -> u8 {
        self.x / self.p
    }
}

/// Reduced Row Echelon Form (RREF) of `M` over the prime field `\mathbb{F}_p`, paired with the row transform `T` such that `T (M \bmod p) = \mathrm{rref}` in `\mathbb{F}_p`.
struct FpRref {
    p: u8,
    rref: Vec<Vec<u8>>,
    transform: Vec<Vec<u8>>,
    /// `pivots[i]` is the pivot column of row `i`, or `None` if row `i` is zero in `rref`.
    pivots: Vec<Option<usize>>,
}

impl FpRref {
    /// Gauss-Jordan elimination on `[M \bmod p \mid I_n]` over `\mathbb{F}_p`.
    /// `p` must be prime so that every nonzero element is a unit.
    fn new(mat: &[Vec<u8>], p: u8) -> Self {
        debug_assert!(p >= 2, "F_p requires a prime p >= 2");
        let n_rows = mat.len();
        let n_cols = mat.first().map_or(0, Vec::len);
        let aug_cols = n_cols + n_rows;

        // Build `[M \bmod p \mid I_n]`.
        let mut aug: Vec<Vec<u8>> = mat
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let mut aug_row = vec_mod(row, p);
                aug_row.resize(aug_cols, 0);
                aug_row[n_cols + i] = 1;
                aug_row
            })
            .collect();

        let mut pivots = vec![None; n_rows];
        let mut pivot_row = 0usize;
        for col in 0..n_cols {
            if pivot_row == n_rows {
                break;
            }
            let Some(swap_with) = (pivot_row..n_rows).find(|&r| aug[r][col] != 0) else {
                // No pivot in this column, so it's a free variable.
                continue;
            };
            aug.swap(swap_with, pivot_row);

            // Scale pivot row so the pivot equals 1.
            let inv = mod_inverse(aug[pivot_row][col], p).expect("nonzero in F_p is a unit");
            for x in aug[pivot_row].iter_mut() {
                *x = mul_mod(*x, inv, p);
            }

            // Eliminate `col` in every other row.
            let pivot = aug[pivot_row].clone();
            for (_, row) in aug
                .iter_mut()
                .enumerate()
                .filter(|(r, row)| *r != pivot_row && row[col] != 0)
            {
                let factor = row[col];
                for (x, &pv) in row.iter_mut().zip(&pivot) {
                    *x = sub_mod(*x, mul_mod(factor, pv, p), p);
                }
            }

            pivots[pivot_row] = Some(col);
            pivot_row += 1;
        }

        // Split each row into its left (rref) and right (transform) halves.
        let (rref, transform): (Vec<_>, Vec<_>) = aug
            .into_iter()
            .map(|mut row| {
                let t = row.split_off(n_cols);
                (row, t)
            })
            .unzip();

        FpRref {
            p,
            rref,
            transform,
            pivots,
        }
    }

    fn n_rows(&self) -> usize {
        self.rref.len()
    }

    fn n_cols(&self) -> usize {
        self.rref.first().map_or(0, Vec::len)
    }

    /// A particular solution of `M x \equiv b \pmod{p}`, or `None` if `b` lies outside `\mathrm{colspace}(M \bmod p)`.
    /// Free variables are set to `0`.
    fn solve(&self, b: &[u8]) -> Option<Vec<u8>> {
        if b.len() != self.n_rows() {
            return None;
        }
        let p = self.p;
        // Apply `T` to `b`: `\mathrm{rref} \, x = T b` is the row-reduced system equivalent to `M x = b`.
        let y = matvec_mod(&self.transform, &vec_mod(b, p), p);
        let mut x = vec![0u8; self.n_cols()];
        for (i, &pc) in self.pivots.iter().enumerate() {
            match pc {
                Some(col) => x[col] = y[i],
                None if y[i] != 0 => return None, // `0 = nonzero` is inconsistent.
                None => {}
            }
        }
        Some(x)
    }

    /// Standard free-variable basis of `\ker(M \bmod p)`.
    /// Each non-pivot column `j` gives one basis vector and we set its `j`-th entry to `1` and force every pivot entry by `v[c_i] = -rref[i][j]` leaving other free entries `0`.
    fn kernel_basis(&self) -> Vec<Vec<u8>> {
        let p = self.p;
        let n_cols = self.n_cols();
        let mut is_pivot_col = vec![false; n_cols];
        for &c in self.pivots.iter().flatten() {
            is_pivot_col[c] = true;
        }
        (0..n_cols)
            .filter(|&j| !is_pivot_col[j])
            .map(|j| {
                let mut v = vec![0u8; n_cols];
                v[j] = 1;
                let pivot_rows = self.rref.iter().zip(&self.pivots);
                for (row, c) in pivot_rows.filter_map(|(row, pc)| pc.map(|c| (row, c))) {
                    v[c] = sub_mod(0, row[j], p);
                }
                v
            })
            .collect()
    }
}

/// Solve `M x \equiv b \pmod{p^e}` by Hensel lifting on the kernel of `M \bmod p`.
fn solve_zpe(mat: &[Vec<u8>], b: &[u8], p: u8, e: u8) -> Option<Vec<u8>> {
    let m = mat.first().map_or(0, Vec::len);
    if e == 0 {
        return Some(vec![0u8; m]);
    }

    // Base step: solve in `\mathbb{F}_p`.
    let rref = FpRref::new(mat, p);
    let x0 = rref.solve(b)?;
    if e == 1 {
        return Some(x0);
    }

    // Lift step.
    let kernel = rref.kernel_basis();
    let pe = p.pow(e as u32);
    let pe_prev = p.pow((e - 1) as u32);

    // Residual `r = (b - M x_0) / p` in `\mathbb{Z}/p^{e-1}\mathbb{Z}`.
    let r: Vec<u8> = matvec_mod(mat, &x0, pe)
        .iter()
        .zip(b)
        .map(|(&mx, &bi)| {
            MultipleOfP::new(sub_mod(bi % pe, mx, pe), p)
                .expect("residual divisible by p is guaranteed by F_p solve")
                .shift_down()
                % pe_prev
        })
        .collect();

    // Augmented operator `[A \mid M]` over `\mathbb{Z}/p^{e-1}\mathbb{Z}`, with `A = (M K) / p`.
    let aug: Vec<Vec<u8>> = mat
        .iter()
        .map(|row| {
            let a = kernel.iter().map(|v| {
                // Compute `(M v)_i` exactly in `i32` to avoid truncation modulo `p^e`, then divide by `p` (exact, by definition of `kernel`).
                let s: i32 = row.iter().zip(v).map(|(&x, &y)| x as i32 * y as i32).sum();
                MultipleOfP::new(s.rem_euclid(pe as i32) as u8, p)
                    .expect("M * K divisible by p is guaranteed by kernel definition")
                    .shift_down()
                    % pe_prev
            });
            a.chain(row.iter().map(|&x| x % pe_prev)).collect()
        })
        .collect();

    let combined = solve_zpe(&aug, &r, p, e - 1)?;
    let (s_part, z_part) = combined.split_at(kernel.len());

    // Reassemble `x = (x_0 + K s + p z) \bmod p^e`.
    let x = (0..m)
        .map(|i| {
            let ks: i32 = kernel
                .iter()
                .zip(s_part)
                .map(|(v, &s)| s as i32 * v[i] as i32)
                .sum();
            (x0[i] as i32 + ks + p as i32 * z_part[i] as i32).rem_euclid(pe as i32) as u8
        })
        .collect();
    Some(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::press::PressPattern;
    use crate::test_support::Lcg;
    use crate::topology::Topology;

    fn check(puzzle: &Puzzle, s0: &State, target: &State) {
        let plan = solve(puzzle, s0, target).unwrap().expect("solvable");
        assert_eq!(
            &puzzle.play(s0, &plan).unwrap(),
            target,
            "plan did not reproduce target"
        );
    }

    fn grid(k: u8, n: usize) -> Puzzle {
        let topo = Topology::Product(Box::new(Topology::Path(n)), Box::new(Topology::Path(n)));
        Puzzle::build(k, &topo, &PressPattern::ClosedNeighborhood { delta: 1 }).unwrap()
    }

    fn reachable(puzzle: &Puzzle, presses: &[usize]) -> State {
        let ids: Vec<MoveId> = presses.iter().copied().map(MoveId).collect();
        puzzle.play(&puzzle.zero_state(), &ids).unwrap()
    }

    fn random_mat(lcg: &mut Lcg, n: usize, m: usize, pe: u8) -> Vec<Vec<u8>> {
        (0..n).map(|_| lcg.vec(m, pe)).collect()
    }

    #[test]
    fn solves_classic_five_by_five() {
        let puzzle = grid(2, 5);
        let all_on = puzzle.state(vec![1; 25]).unwrap();
        check(&puzzle, &all_on, &puzzle.zero_state());
    }

    #[test]
    fn solves_z3_three_by_three() {
        let puzzle = grid(3, 3);
        let s0 = reachable(&puzzle, &[0, 1, 4, 4, 7]);
        check(&puzzle, &s0, &puzzle.zero_state());
    }

    #[test]
    fn z4_three_by_three_lifts_through_two_levels() {
        // `\mathbb{Z}/4\mathbb{Z}` with `4 = 2^2`.
        let puzzle = grid(4, 3);
        let s0 = reachable(&puzzle, &[0, 4, 4, 4, 7, 8]);
        check(&puzzle, &s0, &puzzle.zero_state());
    }

    #[test]
    fn crt_z6_three_by_three() {
        // `\mathbb{Z}/6\mathbb{Z}` with `6 = 2 \cdot 3`.
        let puzzle = grid(6, 3);
        let s0 = reachable(&puzzle, &[0, 4, 7, 7, 2, 5]);
        check(&puzzle, &s0, &puzzle.zero_state());
    }

    #[test]
    fn z12_combines_prime_power_and_crt() {
        // `\mathbb{Z}/12\mathbb{Z}` with `12 = 2^2 \cdot 3`.
        let puzzle = grid(12, 4);
        let s0 = reachable(&puzzle, &[0, 5, 9, 14, 7, 2, 11, 11, 11]);
        check(&puzzle, &s0, &puzzle.zero_state());
    }

    #[test]
    fn unsolvable_returns_none() {
        // Cycle(3) closed-N over `\mathbb{F}_2`.
        let puzzle = Puzzle::build(
            2,
            &Topology::Cycle(3),
            &PressPattern::ClosedNeighborhood { delta: 1 },
        )
        .unwrap();
        let target = puzzle.state(vec![1, 0, 0]).unwrap();
        assert_eq!(solve(&puzzle, &puzzle.zero_state(), &target).unwrap(), None);
    }

    #[test]
    fn already_at_target_returns_no_op_plan() {
        let puzzle = grid(5, 4);
        let s0 = reachable(&puzzle, &[1, 2, 3]);
        check(&puzzle, &s0, &s0);
    }

    #[test]
    fn foreign_state_errors() {
        let puzzle = grid(2, 3);
        let other = grid(2, 2).zero_state();
        assert!(matches!(
            solve(&puzzle, &other, &puzzle.zero_state()),
            Err(SolverError::Puzzle(PuzzleError::StateSizeMismatch { .. }))
        ));
    }

    #[test]
    fn k_one_is_trivially_solved() {
        let puzzle = grid(1, 3);
        let zero = puzzle.zero_state();
        assert_eq!(solve(&puzzle, &zero, &zero).unwrap(), Some(Vec::new()));
    }

    #[test]
    fn prime_factors_of_every_modulus() {
        for k in 1u8..=255 {
            let factors = prime_factors(k);
            let product: u32 = factors
                .iter()
                .map(|&(p, e)| (p as u32).pow(e as u32))
                .product();
            assert_eq!(product, k as u32, "{k} = {factors:?}");
            assert!(factors.windows(2).all(|w| w[0].0 < w[1].0), "{factors:?}");
        }
    }

    #[test]
    fn divisible_by_p_round_trips() {
        // `new(x, p) = Some` iff `p | x`
        for p in [2u8, 3, 5] {
            for x in 0u8..=240 {
                match MultipleOfP::new(x, p) {
                    Some(d) => {
                        assert_eq!(x % p, 0, "new({x}, {p}) accepted non-divisible");
                        assert_eq!(d.shift_down() * p, x, "shift_down lost information");
                    }
                    None => assert_ne!(x % p, 0, "new({x}, {p}) rejected divisible"),
                }
            }
        }
    }

    #[test]
    fn fp_rref_transform_reproduces_rref() {
        // The invariant `T (M \bmod p) = \mathrm{rref}`.
        let mut lcg = Lcg::new(0xBEEF);
        for &p in &[2u8, 3, 5, 7] {
            for &(n, m) in &[(2, 3), (3, 3), (3, 5), (4, 2), (5, 7)] {
                let mat = random_mat(&mut lcg, n, m, p);
                let rref = FpRref::new(&mat, p);
                // Compute `T (M \bmod p)` column-by-column.
                let m_mod = mat_mod(&mat, p);
                for col in 0..m {
                    let col_vec: Vec<u8> = m_mod.iter().map(|row| row[col]).collect();
                    let lhs = matvec_mod(&rref.transform, &col_vec, p);
                    let rhs: Vec<u8> = rref.rref.iter().map(|row| row[col]).collect();
                    assert_eq!(
                        lhs, rhs,
                        "T * M != rref at column {col} (p={p}, n={n}, m={m})"
                    );
                }
                // Pivot columns are strictly increasing where present.
                let pivot_cols: Vec<usize> = rref.pivots.iter().filter_map(|p| *p).collect();
                assert!(
                    pivot_cols.windows(2).all(|w| w[0] < w[1]),
                    "pivots not strictly increasing: {:?} (p={p}, n={n}, m={m})",
                    pivot_cols
                );
            }
        }
    }

    #[test]
    fn fp_rref_kernel_satisfies_m_k_zero() {
        // Every basis vector `v` of the returned kernel satisfies `M v \equiv 0 \pmod{p}`.
        let mut lcg = Lcg::new(0xCAFE);
        for &p in &[2u8, 3, 5, 7] {
            for &(n, m) in &[(2, 3), (3, 3), (3, 5), (4, 2), (5, 7)] {
                let mat = random_mat(&mut lcg, n, m, p);
                let rref = FpRref::new(&mat, p);
                let kernel = rref.kernel_basis();
                for v in &kernel {
                    let mv = matvec_mod(&mat, v, p);
                    assert!(
                        mv.iter().all(|&x| x == 0),
                        "kernel vector {:?} has nonzero image M * v = {:?} (p={p}, n={n}, m={m})",
                        v,
                        mv,
                    );
                }
                // Rank-nullity: pivots + kernel dim = n_cols.
                let pivots = rref.pivots.iter().filter(|p| p.is_some()).count();
                assert_eq!(
                    pivots + kernel.len(),
                    m,
                    "rank-nullity violated (p={p}, n={n}, m={m})"
                );
            }
        }
    }

    fn check_solve_in_span(p: u8, e: u8, n: usize, m: usize, seed: u64) {
        let pe = p.pow(e as u32);
        let mut lcg = Lcg::new(seed);
        let mat = random_mat(&mut lcg, n, m, pe);
        let c0 = lcg.vec(m, pe);
        let b = matvec_mod(&mat, &c0, pe);

        let sol = solve_zpe(&mat, &b, p, e).unwrap_or_else(|| {
            panic!(
                "solve_zpe returned None on in-span b (p={p}, e={e}, n={n}, m={m}, seed={seed}) mat={:?} b={:?} c0={:?}",
                mat, b, c0
            )
        });
        assert_eq!(
            matvec_mod(&mat, &sol, pe),
            b,
            "solve_zpe returned non-solution (p={p}, e={e}, n={n}, m={m}, seed={seed})"
        );
    }

    fn check_unsolvable_detected(p: u8, e: u8, n: usize, m: usize, seed: u64) {
        let pe = p.pow(e as u32);
        let mut lcg = Lcg::new(seed);
        let mat = random_mat(&mut lcg, n, m, pe);
        let b = lcg.vec(n, pe);

        match solve_zpe(&mat, &b, p, e) {
            Some(sol) => assert_eq!(
                matvec_mod(&mat, &sol, pe),
                b,
                "solve_zpe claimed success with wrong solution (p={p}, e={e}, seed={seed})"
            ),
            None => {
                if (m as u32) * (pe as u32).ilog2().max(1) <= 14 {
                    let total = (pe as u64).pow(m as u32);
                    let mut c = vec![0u8; m];
                    let reachable = (0..total).any(|k| {
                        let mut k = k;
                        for ci in c.iter_mut() {
                            *ci = (k % pe as u64) as u8;
                            k /= pe as u64;
                        }
                        matvec_mod(&mat, &c, pe) == b
                    });
                    assert!(
                        !reachable,
                        "solve_zpe returned None but b is reachable (p={p}, e={e}, n={n}, m={m}, seed={seed}) mat={:?} b={:?}",
                        mat, b
                    );
                }
            }
        }
    }

    #[test]
    fn solve_recovers_in_span_b() {
        // Sweep small (p, e) and shapes including rank-deficient cases.
        let cases = [
            (2u8, 1u8),
            (2, 2),
            (2, 3),
            (2, 4),
            (3, 1),
            (3, 2),
            (3, 3),
            (5, 1),
            (5, 2),
            (7, 1),
        ];
        for &(p, e) in &cases {
            for &(n, m) in &[(1, 1), (2, 2), (3, 3), (4, 4), (3, 5), (5, 3), (4, 6)] {
                for seed in 0..16u64 {
                    check_solve_in_span(p, e, n, m, seed);
                }
            }
        }
    }

    #[test]
    fn solve_correctly_detects_unsolvable() {
        let cases = [(2u8, 1u8), (2, 2), (2, 3), (3, 1), (3, 2), (5, 1)];
        for &(p, e) in &cases {
            for &(n, m) in &[(2, 2), (3, 2), (3, 3), (4, 3), (5, 4)] {
                for seed in 100..132u64 {
                    check_unsolvable_detected(p, e, n, m, seed);
                }
            }
        }
    }
}
