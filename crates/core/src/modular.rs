//! Integer and modular arithmetic shared by the solver, move normalisation and the lattice code.

/// `(a + b) \bmod m`, widened so the sum can't overflow.
#[inline]
pub(crate) fn add_mod(a: u8, b: u8, m: u8) -> u8 {
    ((a as u16 + b as u16) % m as u16) as u8
}

/// `(a - b) \bmod m`, always in `[0, m)`.
#[inline]
pub(crate) fn sub_mod(a: u8, b: u8, m: u8) -> u8 {
    (a as i16 - b as i16).rem_euclid(m as i16) as u8
}

/// `(a b) \bmod m`, widened so the product can't overflow.
#[inline]
pub(crate) fn mul_mod(a: u8, b: u8, m: u8) -> u8 {
    ((a as u16 * b as u16) % m as u16) as u8
}

/// Non-negative greatest common divisor.
pub(crate) fn gcd(a: i32, b: i32) -> i32 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Extended Euclidean: returns `(g, s, t)` with `s a + t b = g`.
/// `g` is a gcd up to sign, and is negative for some negative inputs.
pub(crate) fn xgcd(a: i32, b: i32) -> (i32, i32, i32) {
    let (mut old_r, mut r) = (a, b);
    let (mut old_s, mut s) = (1, 0);
    let (mut old_t, mut t) = (0, 1);

    while r != 0 {
        let q = old_r / r;
        (old_r, r) = (r, old_r - q * r);
        (old_s, s) = (s, old_s - q * s);
        (old_t, t) = (t, old_t - q * t);
    }
    (old_r, old_s, old_t)
}

/// The `x` with `a x \equiv 1 \pmod{m}`, or `None` if `a` and `m` aren't coprime.
pub(crate) fn mod_inverse(a: u8, m: u8) -> Option<u8> {
    let (g, s, _) = xgcd(a as i32, m as i32);
    (g == 1).then(|| s.rem_euclid(m as i32) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xgcd_satisfies_bezout() {
        for a in 1i32..30 {
            for b in 1i32..30 {
                let (g, s, t) = xgcd(a, b);
                assert_eq!(s * a + t * b, g, "xgcd({a}, {b}) returned bad coefficients");
            }
        }
    }

    #[test]
    fn mod_inverse_inverts_units_only() {
        for m in 2u8..30 {
            for a in 0..m {
                match mod_inverse(a, m) {
                    Some(x) => assert_eq!(mul_mod(a, x, m), 1, "{a}^-1 mod {m}"),
                    None => assert_ne!(gcd(a as i32, m as i32), 1, "{a} is a unit mod {m}"),
                }
            }
        }
    }

    #[test]
    fn add_sub_mod_do_not_overflow() {
        assert_eq!(add_mod(254, 254, 255), 253);
        assert_eq!(sub_mod(0, 254, 255), 1);
        assert_eq!(mul_mod(254, 254, 255), 1);
    }
}
