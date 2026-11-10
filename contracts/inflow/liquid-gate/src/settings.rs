//! The bounds of every setting (SPEC §2).

use crate::error::ContractError;
use crate::msg::{CurvePoint, Settings, SettingsUpdate};

pub const BPS: u32 = 10_000;
const DAY: u64 = 86_400;
const HOUR: u64 = 3_600;

/// The checks shared by both curves: 2 to 8 points, `f` strictly increasing from 0 to at least
/// 10,000 (the entry curve takes the exit curve's rule: coordinator's answer, 2026-11-10).
fn check_curve_f(curve: &[CurvePoint]) -> bool {
    (2..=8).contains(&curve.len())
        && curve[0].f_bps == 0
        && curve.windows(2).all(|w| w[0].f_bps < w[1].f_bps)
        && curve[curve.len() - 1].f_bps >= BPS
}

fn non_increasing(curve: &[CurvePoint]) -> bool {
    curve.windows(2).all(|w| w[0].bps >= w[1].bps)
}

impl Settings {
    /// The first setting out of bounds is the `InvalidConfig` error.
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = self;
        let checks: [(bool, &'static str); 22] = [
            (!s.buffer_target.is_zero(), "buffer_target must be > 0"),
            (
                (1..=2_000).contains(&s.d_min_bps),
                "d_min_bps must be 1 to 2000",
            ),
            (
                check_curve_f(&s.exit_curve),
                "exit_curve: 2 to 8 points, f strictly increasing from 0 to >= 10000",
            ),
            (
                non_increasing(&s.exit_curve) && s.exit_curve.iter().all(|p| p.bps >= s.d_min_bps),
                "exit_curve: d non-increasing and each >= d_min_bps",
            ),
            (
                s.d_book_min_bps <= s.d_min_bps,
                "d_book_min_bps must be 0 to d_min_bps",
            ),
            (
                check_curve_f(&s.entry_curve),
                "entry_curve: 2 to 8 points, f strictly increasing from 0 to >= 10000",
            ),
            (
                non_increasing(&s.entry_curve)
                    && s.entry_curve.iter().all(|p| p.bps <= s.d_book_min_bps),
                "entry_curve: b non-increasing and each <= d_book_min_bps",
            ),
            (
                (DAY..=60 * DAY).contains(&s.unbonding_time_s),
                "unbonding_time_s must be 1 day to 60 days",
            ),
            (
                (1..=20).contains(&s.max_entries),
                "max_entries must be 1 to 20",
            ),
            (
                (DAY..=s.unbonding_time_s).contains(&s.book_horizon_s),
                "book_horizon_s must be 1 day to unbonding_time_s",
            ),
            (
                (510..=2_000).contains(&s.max_slash_bps),
                "max_slash_bps must be 510 to 2000",
            ),
            (!s.exit_cap_tx.is_zero(), "exit_cap_tx must be > 0"),
            (!s.exit_cap_window.is_zero(), "exit_cap_window must be > 0"),
            (
                (HOUR..=7 * DAY).contains(&s.window_s),
                "window_s must be 1 hour to 7 days",
            ),
            (!s.inventory_cap.is_zero(), "inventory_cap must be > 0"),
            (
                (1..=2_000).contains(&s.price_jump_bps) && s.price_drift_bps_per_day <= 100,
                "price_jump_bps must be 1 to 2000 and price_drift_bps_per_day 0 to 100",
            ),
            (
                (1..=500).contains(&s.reset_tolerance_bps),
                "reset_tolerance_bps must be 1 to 500",
            ),
            (
                s.batch_interval_s >= s.unbonding_time_s / s.max_entries.max(1) as u64 + HOUR,
                "batch_interval_s must be >= unbonding_time_s / max_entries + 1 hour",
            ),
            (
                s.batch_target_min_bps <= s.batch_target_max_bps && s.batch_target_max_bps <= BPS,
                "batch target bounds: 0 <= min <= max <= 10000",
            ),
            (
                (1..=25).contains(&s.burn_max_legs),
                "burn_max_legs must be 1 to 25",
            ),
            (
                s.unwind_max_fee_bps <= 500,
                "unwind_max_fee_bps must be 0 to 500",
            ),
            (
                (1..=80).contains(&s.undelegate_max_validators),
                "undelegate_max_validators must be 1 to 80",
            ),
        ];
        match checks.into_iter().find(|(ok, _)| !ok) {
            Some((_, why)) => Err(ContractError::InvalidConfig(why)),
            None => Ok(()),
        }
    }

    /// The settings with `u` applied (`profit_recipient` is handled by the caller).
    pub fn apply(&self, u: SettingsUpdate) -> Settings {
        let mut s = self.clone();
        macro_rules! set {
            ($($f:ident),*) => { $( if let Some(v) = u.$f { s.$f = v; } )* };
        }
        set!(
            buffer_target,
            exit_curve,
            d_min_bps,
            d_book_min_bps,
            entry_curve,
            unbonding_time_s,
            max_entries,
            book_horizon_s,
            max_slash_bps,
            exit_cap_tx,
            exit_cap_window,
            window_s,
            inventory_cap,
            price_jump_bps,
            price_drift_bps_per_day,
            reset_tolerance_bps,
            batch_interval_s,
            batch_target_min_bps,
            batch_target_max_bps,
            burn_max_legs,
            unwind_max_fee_bps,
            undelegate_max_validators
        );
        s
    }
}
