//! The piecewise linear curves of SPEC §3 and §4: the value at a fill and the exact average over
//! a range of fills (trapezoids). Rounding is always against the caller: a discount is rounded
//! up, a bonus down.

use cosmwasm_std::{Int128, Int256, Uint128, Uint256};

use crate::msg::CurvePoint;
use crate::settings::BPS;

/// How a value is rounded to whole bps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Round {
    /// A discount (the gate keeps more when it is higher).
    Up,
    /// A bonus (the gate gives more when it is higher).
    Down,
}

/// Fixed-point scale of the segment integrals. Curve points are whole bps, so with fills that are
/// multiples of `2^a 5^b` (as curve points normally are) each segment's integral is exact.
const SCALE: i128 = 1_000_000_000_000_000_000;

/// The buffer fill of `free` ATOM, in bps of `buffer_target`, capped at the curve's last point.
/// `Round::Up` gives the ceiling (the entry side: a higher fill, a lower bonus), `Down` the floor
/// (the exit side: a lower fill, a higher discount).
pub fn fill_bps(free: Uint128, buffer_target: Uint128, curve: &[CurvePoint], round: Round) -> u64 {
    let last = curve.last().map(|p| p.f_bps as u64).unwrap_or(0);
    let num = Uint256::from(free) * Uint256::from(BPS as u128);
    let den = Uint256::from(buffer_target);
    let mut f = num / den;
    if round == Round::Up && !(num % den).is_zero() {
        f += Uint256::one();
    }
    if f > Uint256::from(last) {
        last
    } else {
        u64::try_from(Uint128::try_from(f).unwrap_or(Uint128::MAX).u128()).unwrap_or(last)
    }
}

fn div_round(n: Int256, d: Int256, round: Round) -> Int256 {
    // n >= 0 and d > 0 here (curve values are non-negative).
    let q = n / d;
    if round == Round::Up && q * d != n {
        q + Int256::one()
    } else {
        q
    }
}

fn to_bps(v: Int256) -> u32 {
    if v <= Int256::zero() {
        0
    } else {
        Int128::try_from(v)
            .ok()
            .and_then(|x| u32::try_from(x.i128()).ok())
            .unwrap_or(u32::MAX)
    }
}

/// The curve's value at fill `f` (at most its last point).
pub fn value_at(curve: &[CurvePoint], f: u64, round: Round) -> u32 {
    for w in curve.windows(2) {
        let (x0, x1) = (w[0].f_bps as u64, w[1].f_bps as u64);
        if f <= x1 {
            let (y0, y1) = (
                Int256::from(w[0].bps as i128),
                Int256::from(w[1].bps as i128),
            );
            let len = Int256::from((x1 - x0) as i128);
            let dx = Int256::from(f.saturating_sub(x0) as i128);
            let n = y0 * len + (y1 - y0) * dx;
            return to_bps(div_round(n, len, round));
        }
    }
    curve.last().map(|p| p.bps).unwrap_or(0)
}

/// The curve's average over the fills `[a, b]` (in either order), exact up to the fixed-point
/// scale; the value at `a` when the range is empty.
pub fn average(curve: &[CurvePoint], a: u64, b: u64, round: Round) -> u32 {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    if a == b {
        return value_at(curve, a, round);
    }
    let scale = Int256::from(SCALE);
    let mut total = Int256::zero();
    for w in curve.windows(2) {
        let (x0, x1) = (w[0].f_bps as u64, w[1].f_bps as u64);
        let (u, v) = (a.max(x0), b.min(x1));
        if u >= v {
            continue;
        }
        let (y0, y1) = (
            Int256::from(w[0].bps as i128),
            Int256::from(w[1].bps as i128),
        );
        let len = Int256::from((x1 - x0) as i128);
        let du = Int256::from((u - x0) as i128);
        let dv = Int256::from((v - x0) as i128);
        // 2 × len × ∫_u^v y = (v - u) × (2 y0 len + (y1 - y0)(du + dv))
        let n = Int256::from((v - u) as i128)
            * (Int256::from(2i128) * y0 * len + (y1 - y0) * (du + dv));
        total += n * scale / (Int256::from(2i128) * len);
    }
    let width = Int256::from((b - a) as i128) * scale;
    to_bps(div_round(total, width, round))
}

/// The booking discount at `t` seconds to maturity (SPEC §4):
/// `d_book_min + (d_inst - d_book_min) × max(0, 1 - t / T)`, rounded up.
pub fn booking_discount(d_inst: u32, d_book_min: u32, t: u64, horizon: u64) -> u32 {
    let span = d_inst.saturating_sub(d_book_min) as u128;
    if t >= horizon || horizon == 0 {
        return d_book_min;
    }
    let n = span * (horizon - t) as u128;
    let extra = n.div_ceil(horizon as u128);
    d_book_min + extra as u32
}

/// `⌊v × (10,000 - d) / 10,000⌋`.
pub fn apply_discount(v: Uint128, d_bps: u32) -> Uint128 {
    v.multiply_ratio(BPS.saturating_sub(d_bps), BPS)
}

/// `⌊a × (10,000 + b) / 10,000⌋`.
pub fn apply_bonus(a: Uint128, b_bps: u32) -> Uint128 {
    a.multiply_ratio(BPS + b_bps, BPS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(points: &[(u32, u32)]) -> Vec<CurvePoint> {
        points
            .iter()
            .map(|&(f_bps, bps)| CurvePoint { f_bps, bps })
            .collect()
    }

    #[test]
    fn values_and_averages() {
        // 200 bps empty, 50 bps at half, 50 bps full.
        let curve = c(&[(0, 200), (5_000, 50), (10_000, 50)]);
        assert_eq!(value_at(&curve, 0, Round::Up), 200);
        assert_eq!(value_at(&curve, 2_500, Round::Up), 125);
        assert_eq!(value_at(&curve, 7_000, Round::Up), 50);
        assert_eq!(value_at(&curve, 10_000, Round::Up), 50);
        // One segment: the midpoint value.
        assert_eq!(average(&curve, 0, 5_000, Round::Up), 125);
        assert_eq!(average(&curve, 5_000, 0, Round::Down), 125);
        // Across segments: (125 × 5000 + 50 × 5000) / 10000 = 87.5.
        assert_eq!(average(&curve, 0, 10_000, Round::Up), 88);
        assert_eq!(average(&curve, 0, 10_000, Round::Down), 87);
        // Flat part.
        assert_eq!(average(&curve, 6_000, 9_000, Round::Up), 50);
    }

    #[test]
    fn fills() {
        let curve = c(&[(0, 200), (10_000, 50)]);
        let t = Uint128::new(1_000);
        assert_eq!(fill_bps(Uint128::new(500), t, &curve, Round::Down), 5_000);
        assert_eq!(
            fill_bps(Uint128::new(5_000), t, &curve, Round::Down),
            10_000
        );
        assert_eq!(
            fill_bps(Uint128::new(1), Uint128::new(3), &curve, Round::Down),
            3_333
        );
        assert_eq!(
            fill_bps(Uint128::new(1), Uint128::new(3), &curve, Round::Up),
            3_334
        );
    }

    #[test]
    fn booking_curve() {
        // d_inst 150, floor 30, T 21 days.
        let t_day = 86_400;
        let h = 21 * t_day;
        assert_eq!(booking_discount(150, 30, 0, h), 150);
        assert_eq!(booking_discount(150, 30, h, h), 30);
        assert_eq!(booking_discount(150, 30, 2 * h, h), 30);
        // 2 days: 30 + 120 × 19/21 = 138.57 → 139 (README example: 1.39%).
        assert_eq!(booking_discount(150, 30, 2 * t_day, h), 139);
        // 11 days: 30 + 120 × 10/21 = 87.14 → 88.
        assert_eq!(booking_discount(150, 30, 11 * t_day, h), 88);
    }
}
