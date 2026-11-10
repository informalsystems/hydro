//! The gate's tests, on the mocked chain of `testing_mocks` (small inline data only).

use cosmwasm_std::{coin, Addr, Uint128};

use crate::error::ContractError;
use crate::msg::{
    BatchesResponse, BookingsResponse, CurvePoint, EntriesResponse, EntryRoute, ExecuteMsg,
    ExitRoute, PositionResponse, QueryMsg, QuoteEntryResponse, QuoteExitResponse, Settings,
    SettingsUpdate,
};
use crate::state::{Config, OP_GUARD};
use crate::testing_mocks::{App, BOND, HATOM, SHARES};

const ATOM: u128 = 1_000_000;
const DAY: u64 = 86_400;
const SHARE: u128 = 1_000_000;

fn curve(points: &[(u32, u32)]) -> Vec<CurvePoint> {
    points
        .iter()
        .map(|&(f_bps, bps)| CurvePoint { f_bps, bps })
        .collect()
}

fn settings() -> Settings {
    Settings {
        buffer_target: Uint128::new(100_000 * ATOM),
        exit_curve: curve(&[(0, 300), (5_000, 100), (10_000, 50)]),
        d_min_bps: 50,
        d_book_min_bps: 30,
        entry_curve: curve(&[(0, 30), (5_000, 10), (10_000, 0)]),
        unbonding_time_s: 21 * DAY,
        max_entries: 7,
        book_horizon_s: 21 * DAY,
        max_slash_bps: 510,
        exit_cap_tx: Uint128::new(50_000 * ATOM),
        exit_cap_window: Uint128::new(200_000 * ATOM),
        window_s: DAY,
        inventory_cap: Uint128::new(1_000_000 * ATOM),
        price_jump_bps: 100,
        price_drift_bps_per_day: 10,
        reset_tolerance_bps: 50,
        batch_interval_s: 3 * DAY + 3_600,
        batch_target_min_bps: 0,
        batch_target_max_bps: 10_000,
        burn_max_legs: 5,
        unwind_max_fee_bps: 50,
        undelegate_max_validators: 10,
    }
}

fn e(err: ContractError) -> String {
    err.to_string()
}

/// A gate with a 100,000 ATOM buffer.
fn setup() -> App {
    let app = App::new(settings());
    app.fund(&app.gate.clone(), BOND, 100_000 * ATOM);
    app
}

fn exit_instant(app: &mut App, user: &Addr, shares: u128) -> Result<(), String> {
    app.give_shares(user, shares);
    app.execute(
        user,
        ExecuteMsg::ExitInstant {
            recipient: None,
            min_out: None,
        },
        &[coin(shares, SHARES)],
    )
}

fn position(app: &App) -> PositionResponse {
    app.query(QueryMsg::Position {})
}

/// Exits to build an inventory, then unwinds all of it and undelegates: one batch.
fn make_batch(app: &mut App, shares: u128) -> u64 {
    let seller = app.addr("seller");
    exit_instant(app, &seller, shares).unwrap();
    let keeper = app.keeper.clone();
    app.execute(
        &keeper,
        ExecuteMsg::SetBatchTarget {
            shares: Uint128::new(shares),
        },
        &[],
    )
    .unwrap();
    app.execute(&seller, ExecuteMsg::Unwind { shares: None }, &[])
        .unwrap();
    app.execute(&seller, ExecuteMsg::ProcessBatch {}, &[])
        .unwrap();
    app.attr("batch_created").unwrap().parse().unwrap()
}

// ---------------------------------------------------------------------------
// Settings and roles

#[test]
fn config_bounds() {
    let base = settings();
    assert!(base.validate().is_ok());
    type Breaker = Box<dyn Fn(&mut Settings)>;
    let bad: Vec<(Breaker, &str)> = vec![
        (
            Box::new(|s| s.buffer_target = Uint128::zero()),
            "buffer_target",
        ),
        (Box::new(|s| s.d_min_bps = 0), "d_min_bps"),
        (Box::new(|s| s.d_min_bps = 2_001), "d_min_bps"),
        (
            Box::new(|s| s.exit_curve = curve(&[(0, 300)])),
            "exit_curve",
        ),
        (
            Box::new(|s| s.exit_curve = curve(&[(1, 300), (10_000, 50)])),
            "exit_curve",
        ),
        (
            Box::new(|s| s.exit_curve = curve(&[(0, 300), (9_999, 50)])),
            "exit_curve",
        ),
        (
            Box::new(|s| s.exit_curve = curve(&[(0, 300), (0, 50), (10_000, 50)])),
            "exit_curve",
        ),
        (
            Box::new(|s| s.exit_curve = curve(&[(0, 50), (10_000, 60)])),
            "exit_curve: d",
        ),
        (
            Box::new(|s| s.exit_curve = curve(&[(0, 300), (10_000, 40)])),
            "exit_curve: d",
        ),
        (Box::new(|s| s.d_book_min_bps = 51), "d_book_min_bps"),
        (
            Box::new(|s| s.entry_curve = curve(&[(5, 30), (10_000, 0)])),
            "entry_curve",
        ),
        (
            Box::new(|s| s.entry_curve = curve(&[(0, 31), (10_000, 0)])),
            "entry_curve: b",
        ),
        (
            Box::new(|s| s.entry_curve = curve(&[(0, 0), (10_000, 10)])),
            "entry_curve: b",
        ),
        (
            Box::new(|s| s.unbonding_time_s = DAY - 1),
            "unbonding_time_s",
        ),
        (
            Box::new(|s| s.unbonding_time_s = 61 * DAY),
            "unbonding_time_s",
        ),
        (Box::new(|s| s.max_entries = 0), "max_entries"),
        (Box::new(|s| s.max_entries = 21), "max_entries"),
        (Box::new(|s| s.book_horizon_s = 22 * DAY), "book_horizon_s"),
        (Box::new(|s| s.max_slash_bps = 509), "max_slash_bps"),
        (Box::new(|s| s.max_slash_bps = 2_001), "max_slash_bps"),
        (Box::new(|s| s.exit_cap_tx = Uint128::zero()), "exit_cap_tx"),
        (
            Box::new(|s| s.exit_cap_window = Uint128::zero()),
            "exit_cap_window",
        ),
        (Box::new(|s| s.window_s = 3_599), "window_s"),
        (Box::new(|s| s.window_s = 7 * DAY + 1), "window_s"),
        (
            Box::new(|s| s.inventory_cap = Uint128::zero()),
            "inventory_cap",
        ),
        (Box::new(|s| s.price_jump_bps = 0), "price_jump_bps"),
        (
            Box::new(|s| s.price_drift_bps_per_day = 101),
            "price_jump_bps",
        ),
        (
            Box::new(|s| s.reset_tolerance_bps = 501),
            "reset_tolerance_bps",
        ),
        (
            Box::new(|s| s.batch_interval_s = 3 * DAY + 3_599),
            "batch_interval_s",
        ),
        (Box::new(|s| s.batch_target_min_bps = 5_001), "batch target"),
        (
            Box::new(|s| s.batch_target_max_bps = 10_001),
            "batch target",
        ),
        (Box::new(|s| s.burn_max_legs = 26), "burn_max_legs"),
        (
            Box::new(|s| s.unwind_max_fee_bps = 501),
            "unwind_max_fee_bps",
        ),
        (
            Box::new(|s| s.undelegate_max_validators = 81),
            "undelegate_max_validators",
        ),
    ];
    for (f, why) in bad {
        let mut s = base.clone();
        s.batch_target_max_bps = 5_000;
        f(&mut s);
        match s.validate() {
            Err(ContractError::InvalidConfig(m)) => assert!(m.starts_with(why), "{why}: {m}"),
            other => panic!("{why}: {other:?}"),
        }
    }

    // UpdateConfig: admin only, checked against the bounds, the result kept.
    let mut app = setup();
    let admin = app.admin.clone();
    let op = app.operator.clone();
    let upd = |m: u32| ExecuteMsg::UpdateConfig {
        settings: SettingsUpdate {
            max_slash_bps: Some(m),
            ..Default::default()
        },
    };
    assert_eq!(
        app.execute(&op, upd(600), &[]),
        Err(e(ContractError::Unauthorized))
    );
    assert!(app
        .execute(&admin, upd(500), &[])
        .unwrap_err()
        .contains("max_slash_bps"));
    app.execute(&admin, upd(600), &[]).unwrap();
    let cfg: Config = app.query(QueryMsg::Config {});
    assert_eq!(cfg.settings.max_slash_bps, 600);
    assert_eq!(cfg.hatom_denom, HATOM);
    assert_eq!(cfg.share_denom, SHARES);
    assert_eq!(cfg.bond_denom, BOND);
}

#[test]
fn roles_and_payment() {
    let mut app = setup();
    let (admin, op, keeper) = (app.admin.clone(), app.operator.clone(), app.keeper.clone());
    let new_admin = app.addr("new_admin");
    let user = app.addr("user");
    app.fund(&user, BOND, 10 * ATOM);

    // Non-payable messages refuse funds.
    app.fund(&admin, BOND, 1);
    let r = app.execute(&admin, ExecuteMsg::Pause {}, &[coin(1, BOND)]);
    assert!(r.unwrap_err().contains("accept funds"));
    // Trades take exactly one coin of their denom.
    let r = app.execute(
        &user,
        ExecuteMsg::ExitInstant {
            recipient: None,
            min_out: None,
        },
        &[coin(1, BOND)],
    );
    assert!(r.is_err());

    // Two-step admin transfer.
    assert_eq!(
        app.execute(
            &user,
            ExecuteMsg::TransferAdmin {
                admin: user.to_string()
            },
            &[]
        ),
        Err(e(ContractError::Unauthorized))
    );
    app.execute(
        &admin,
        ExecuteMsg::TransferAdmin {
            admin: new_admin.to_string(),
        },
        &[],
    )
    .unwrap();
    assert_eq!(
        app.execute(&user, ExecuteMsg::AcceptAdmin {}, &[]),
        Err(e(ContractError::Unauthorized))
    );
    app.execute(&new_admin, ExecuteMsg::AcceptAdmin {}, &[])
        .unwrap();
    let cfg: Config = app.query(QueryMsg::Config {});
    assert_eq!(cfg.admin, new_admin);
    assert_eq!(cfg.pending_admin, None);

    // Keeper and operators.
    assert_eq!(
        app.execute(
            &op,
            ExecuteMsg::SetBatchTarget {
                shares: Uint128::one()
            },
            &[]
        ),
        Err(e(ContractError::Unauthorized))
    );
    app.execute(
        &keeper,
        ExecuteMsg::SetBatchTarget {
            shares: Uint128::one(),
        },
        &[],
    )
    .unwrap();
    assert_eq!(
        app.execute(
            &admin,
            ExecuteMsg::SetKeeper {
                keeper: user.to_string()
            },
            &[]
        ),
        Err(e(ContractError::Unauthorized))
    );
    app.execute(
        &new_admin,
        ExecuteMsg::SetKeeper {
            keeper: user.to_string(),
        },
        &[],
    )
    .unwrap();
    app.execute(
        &user,
        ExecuteMsg::SetBatchTarget {
            shares: Uint128::one(),
        },
        &[],
    )
    .unwrap();
    app.execute(
        &new_admin,
        ExecuteMsg::SetOperators {
            operators: vec![user.to_string()],
        },
        &[],
    )
    .unwrap();
    assert_eq!(
        app.execute(&op, ExecuteMsg::Pause {}, &[]),
        Err(e(ContractError::Unauthorized))
    );
    app.execute(&user, ExecuteMsg::Pause {}, &[]).unwrap();
    // Only the admin unpauses.
    assert_eq!(
        app.execute(&user, ExecuteMsg::Unpause {}, &[]),
        Err(e(ContractError::Unauthorized))
    );
    app.execute(&new_admin, ExecuteMsg::Unpause {}, &[])
        .unwrap();

    // The internal self-call is the contract's only.
    assert_eq!(
        app.execute(&user, ExecuteMsg::AssertInvariants { swap: None }, &[]),
        Err(e(ContractError::Unauthorized))
    );
}

// ---------------------------------------------------------------------------
// Trades

#[test]
fn exit_instant_matches_quote() {
    let mut app = setup();
    let user = app.addr("user");
    let shares = 10_000 * SHARE;
    let q: QuoteExitResponse = app.query(QueryMsg::QuoteExit {
        shares: Uint128::new(shares),
        route: ExitRoute::Instant,
    });
    assert!(q.available, "{:?}", q.reason);
    // V = 10,000 shares × 1.1 hATOM × 1.25 = 13,750 ATOM.
    assert_eq!(q.value, Some(Uint128::new(13_750 * ATOM)));
    // The fill goes from 100% to 86.25%: the curve's 50 bps to 50 + 50 × 0.1375... averaged.
    let d = q.discount_bps.unwrap();
    assert!(d > 50 && d < 70, "{d}");
    exit_instant(&mut app, &user, shares).unwrap();
    assert_eq!(app.bal(&user, BOND), q.atom.u128());
    assert_eq!(q.atom.u128(), 13_750 * ATOM * (10_000 - d as u128) / 10_000);

    let p = position(&app);
    assert_eq!(p.inventory_shares.u128(), shares);
    assert_eq!(p.inventory_cost, q.atom);
    assert_eq!(
        p.totals.discount_earned.u128(),
        13_750 * ATOM - q.atom.u128()
    );
    assert_eq!(p.free.u128(), 100_000 * ATOM - q.atom.u128());
    assert_eq!(p.price_ref, Some(Uint128::new(1_375_000)));

    // min_out above the payout: refused, nothing changes.
    let r = app.execute(
        &user,
        ExecuteMsg::ExitInstant {
            recipient: None,
            min_out: Some(Uint128::new(u128::MAX)),
        },
        &[],
    );
    assert!(r.is_err());
    app.give_shares(&user, SHARE);
    let r = app.execute(
        &user,
        ExecuteMsg::ExitInstant {
            recipient: None,
            min_out: Some(Uint128::new(10 * ATOM)),
        },
        &[coin(SHARE, SHARES)],
    );
    assert!(r.unwrap_err().contains("below the minimum"));
    assert_eq!(app.bal(&user, SHARES), SHARE);
    assert_eq!(position(&app).inventory_shares.u128(), shares);
}

#[test]
fn exit_refusals() {
    let mut app = setup();
    let user = app.addr("user");
    // Above the free buffer.
    assert_eq!(
        exit_instant(&mut app, &user, 80_000 * SHARE),
        Err(e(ContractError::BufferTooLow))
    );
    // The per-transaction cap (50,000 ATOM ≈ 36,364 shares).
    app.fund(&app.gate.clone(), BOND, 1_000_000 * ATOM);
    assert_eq!(
        exit_instant(&mut app, &user, 40_000 * SHARE),
        Err(e(ContractError::CapExceeded { which: "tx".into() }))
    );
    // The window: 200,000 ATOM per day.
    for _ in 0..4 {
        exit_instant(&mut app, &user, 36_000 * SHARE).unwrap();
    }
    assert_eq!(
        exit_instant(&mut app, &user, 36_000 * SHARE),
        Err(e(ContractError::CapExceeded {
            which: "window".into()
        }))
    );
    app.advance(DAY);
    exit_instant(&mut app, &user, 36_000 * SHARE).unwrap();
    // The inventory cap.
    let admin = app.admin.clone();
    app.execute(
        &admin,
        ExecuteMsg::UpdateConfig {
            settings: SettingsUpdate {
                inventory_cap: Some(Uint128::new(250_000 * ATOM)),
                ..Default::default()
            },
        },
        &[],
    )
    .unwrap();
    assert_eq!(
        exit_instant(&mut app, &user, 36_000 * SHARE),
        Err(e(ContractError::CapExceeded {
            which: "inventory".into()
        }))
    );
    // A dust exit whose value is 0.
    assert_eq!(
        exit_instant(&mut app, &user, 0),
        Err("No funds sent".into())
    );
}

#[test]
fn enter_swap_matches_quote() {
    let mut app = setup();
    let seller = app.addr("seller");
    let buyer = app.addr("buyer");

    // No inventory yet.
    let q: QuoteEntryResponse = app.query(QueryMsg::QuoteEntry {
        amount: Uint128::new(100 * ATOM),
        route: EntryRoute::Swap,
    });
    assert!(!q.available);
    assert_eq!(q.reason, Some(e(ContractError::NoInventory)));

    exit_instant(&mut app, &seller, 20_000 * SHARE).unwrap();
    let before = position(&app);
    let a = 1_000 * ATOM;
    let q: QuoteEntryResponse = app.query(QueryMsg::QuoteEntry {
        amount: Uint128::new(a),
        route: EntryRoute::Swap,
    });
    assert!(q.available, "{:?}", q.reason);
    let b = q.bonus_bps.unwrap();
    // The fill is below full after the exit: some bonus, at most d_book_min.
    assert!(b > 0 && b <= 30, "{b}");
    app.fund(&buyer, BOND, a);
    app.execute(
        &buyer,
        ExecuteMsg::EnterSwap {
            recipient: None,
            min_out: Some(q.shares),
        },
        &[coin(a, BOND)],
    )
    .unwrap();
    assert_eq!(app.bal(&buyer, SHARES), q.shares.u128());
    // The largest s with V(s) ≤ ⌊A × (1 + b̄)⌋.
    let target = a * (10_000 + b as u128) / 10_000;
    let v = |s: u128| s * 11 / 10 * 125 / 100;
    assert!(v(q.shares.u128()) <= target && v(q.shares.u128() + 1) > target);

    let after = position(&app);
    assert_eq!(after.inventory_shares, before.inventory_shares - q.shares);
    let cost_out =
        (before.inventory_cost.u128() * q.shares.u128()).div_ceil(before.inventory_shares.u128());
    assert_eq!(
        after.inventory_cost.u128(),
        before.inventory_cost.u128() - cost_out
    );
    assert_eq!(after.totals.bonus_paid.u128(), v(q.shares.u128()) - a);
    assert_eq!(after.free.u128(), before.free.u128() + a);

    // min_out above the quote: refused.
    app.fund(&buyer, BOND, a);
    let r = app.execute(
        &buyer,
        ExecuteMsg::EnterSwap {
            recipient: None,
            min_out: Some(q.shares + Uint128::new(10 * SHARE)),
        },
        &[coin(a, BOND)],
    );
    assert!(r.unwrap_err().contains("below the minimum"));
    // More than the inventory.
    app.fund(&buyer, BOND, 100_000 * ATOM);
    assert_eq!(
        app.execute(
            &buyer,
            ExecuteMsg::EnterSwap {
                recipient: None,
                min_out: None
            },
            &[coin(30_000 * ATOM, BOND)],
        ),
        Err(e(ContractError::NoInventory))
    );
}

#[test]
fn enter_swap_below_cost() {
    let mut app = setup();
    let seller = app.addr("seller");
    let buyer = app.addr("buyer");
    exit_instant(&mut app, &seller, 20_000 * SHARE).unwrap();
    // The share price falls 3%: an entry now gets more shares than their cost covers.
    app.world.borrow_mut().hatom.holders_value = 1_250_000_000_000 * 97 / 100;
    app.fund(&buyer, BOND, 1_000 * ATOM);
    let enter = ExecuteMsg::EnterSwap {
        recipient: None,
        min_out: None,
    };
    assert_eq!(
        app.execute(&buyer, enter.clone(), &[coin(1_000 * ATOM, BOND)]),
        Err(e(ContractError::PriceJump))
    );
    let op = app.operator.clone();
    let p = Uint128::new(1_375_000 * 97 / 100);
    app.execute(&op, ExecuteMsg::ResetPriceReference { price: p }, &[])
        .unwrap();
    assert_eq!(
        app.execute(&buyer, enter, &[coin(1_000 * ATOM, BOND)]),
        Err(e(ContractError::BelowCost))
    );
}

#[test]
fn enter_direct_matches_quote() {
    let mut app = setup();
    let user = app.addr("user");
    let a = 500 * ATOM;
    let q: QuoteEntryResponse = app.query(QueryMsg::QuoteEntry {
        amount: Uint128::new(a),
        route: EntryRoute::Direct,
    });
    assert!(q.available, "{:?}", q.reason);
    assert_eq!(q.hatom_minted, Some(Uint128::new(400 * ATOM)));
    assert_eq!(q.within_deposit_cap, Some(true));
    app.fund(&user, BOND, 3 * a);
    let gate_before = (
        app.bal(&app.gate.clone(), BOND),
        app.bal(&app.gate.clone(), HATOM),
    );
    app.execute(
        &user,
        ExecuteMsg::EnterDirect {
            recipient: None,
            min_out: Some(q.shares),
        },
        &[coin(a, BOND)],
    )
    .unwrap();
    assert_eq!(app.bal(&user, SHARES), q.shares.u128());
    // The gate keeps nothing.
    let gate = app.gate.clone();
    assert_eq!((app.bal(&gate, BOND), app.bal(&gate, HATOM)), gate_before);
    assert_eq!(app.bal(&gate, SHARES), 0);

    // min_out above what the deposit gives: the whole transaction, mint included, is undone.
    let supply = app.world.borrow().hatom.supply;
    let r = app.execute(
        &user,
        ExecuteMsg::EnterDirect {
            recipient: None,
            min_out: Some(q.shares + Uint128::one()),
        },
        &[coin(a, BOND)],
    );
    assert!(r.unwrap_err().contains("below the minimum"));
    assert_eq!(app.world.borrow().hatom.supply, supply);
    assert_eq!(app.bal(&user, BOND), 2 * a);

    // Above the vault's deposit cap: the vault refuses, everything is undone.
    app.world.borrow_mut().vault.deposit_cap = 1;
    let q: QuoteEntryResponse = app.query(QueryMsg::QuoteEntry {
        amount: Uint128::new(a),
        route: EntryRoute::Direct,
    });
    assert!(!q.available);
    assert_eq!(q.within_deposit_cap, Some(false));
    let r = app.execute(
        &user,
        ExecuteMsg::EnterDirect {
            recipient: None,
            min_out: None,
        },
        &[coin(a, BOND)],
    );
    assert!(r.unwrap_err().contains("deposit cap"));
    assert_eq!(app.world.borrow().hatom.supply, supply);

    // hATOM paused: "not available" with hATOM's reason.
    app.world.borrow_mut().hatom.paused = true;
    let q: QuoteEntryResponse = app.query(QueryMsg::QuoteEntry {
        amount: Uint128::new(a),
        route: EntryRoute::Direct,
    });
    assert!(!q.available);
    assert!(q.reason.unwrap().contains("paused"));
}

// ---------------------------------------------------------------------------
// Unwind and batches

#[test]
fn unwind_and_batch() {
    let mut app = setup();
    let seller = app.addr("seller");
    let anyone = app.addr("anyone");
    exit_instant(&mut app, &seller, 10_000 * SHARE).unwrap();
    app.world.borrow_mut().auto_claim = 7;
    app.world.borrow_mut().hatom.compound_gain = 1_000;
    app.world.borrow_mut().hatom.burn_rebate_bps = 10;

    // Nothing to unwind without a target.
    assert_eq!(
        app.execute(&anyone, ExecuteMsg::Unwind { shares: None }, &[]),
        Err(e(ContractError::NothingToDo))
    );
    let keeper = app.keeper.clone();
    app.execute(
        &keeper,
        ExecuteMsg::SetBatchTarget {
            shares: Uint128::new(6_000 * SHARE),
        },
        &[],
    )
    .unwrap();
    // An explicit amount above the target is refused.
    assert_eq!(
        app.execute(
            &anyone,
            ExecuteMsg::Unwind {
                shares: Some(Uint128::new(6_001 * SHARE))
            },
            &[]
        ),
        Err(e(ContractError::CapExceeded {
            which: "batch_target".into()
        }))
    );
    let before = position(&app);
    app.execute(
        &anyone,
        ExecuteMsg::Unwind {
            shares: Some(Uint128::new(4_000 * SHARE)),
        },
        &[],
    )
    .unwrap();
    // The Compound succeeded (before the withdrawal), nothing skipped.
    assert_eq!(app.world.borrow().hatom.compounds, 1);
    assert_eq!(app.attr("compound_skipped"), None);
    let after = position(&app);
    assert_eq!(after.inventory_shares.u128(), 6_000 * SHARE);
    assert_eq!(after.unwound_in_interval.u128(), 4_000 * SHARE);
    assert_eq!(
        after.inventory_cost.u128(),
        before.inventory_cost.u128() - (before.inventory_cost.u128() * 4).div_ceil(10)
    );
    // The gate holds no hATOM and no LSM share; its delegations are the burn's payout.
    let gate = app.gate.clone();
    assert_eq!(app.bal(&gate, HATOM), 0);
    let delegated: u128 = after.delegated.iter().map(|c| c.amount.u128()).sum();
    assert!(delegated > 0);
    let burn_value: u128 = app.attr("burn_value").unwrap().parse().unwrap();
    let rebate: u128 = app.attr("imbalance_rebate").unwrap().parse().unwrap();
    assert!(rebate > 0);
    assert_eq!(delegated, burn_value + rebate);
    assert_eq!(after.totals.burn_bonus_received.u128(), rebate);
    // Two redeems, 7 each, auto-claimed: the staking rewards.
    assert_eq!(after.totals.rewards_received.u128(), 14);

    // The default amount is what is left of the target.
    app.world.borrow_mut().hatom.compound_refused = true;
    app.execute(&anyone, ExecuteMsg::Unwind { shares: None }, &[])
        .unwrap();
    assert_eq!(
        app.attr("compound_skipped"),
        Some("compound refused".into())
    );
    assert_eq!(app.attr("shares"), Some((2_000 * SHARE).to_string()));
    assert_eq!(
        app.execute(&anyone, ExecuteMsg::Unwind { shares: None }, &[]),
        Err(e(ContractError::NothingToDo))
    );

    // The batch.
    let delegated: u128 = position(&app)
        .delegated
        .iter()
        .map(|c| c.amount.u128())
        .sum();
    app.execute(&anyone, ExecuteMsg::ProcessBatch {}, &[])
        .unwrap();
    let id: u64 = app.attr("batch_created").unwrap().parse().unwrap();
    let batches: BatchesResponse = app.query(QueryMsg::Batches {
        start_after: None,
        limit: None,
    });
    let b = &batches.batches[0];
    assert_eq!(b.id, id);
    assert_eq!(b.amount.u128(), delegated);
    assert_eq!(
        b.capacity,
        b.amount.multiply_ratio(10_000u128 - 510, 10_000u128)
    );
    assert_eq!(
        b.completion.seconds(),
        app.env().block.time.seconds() + 21 * DAY
    );
    let p = position(&app);
    assert!(p.delegated.is_empty());
    assert_eq!(p.totals.unbonded, b.amount);
    // A new interval started: the unwound count was reset.
    assert_eq!(p.unwound_in_interval.u128(), 0);
    let entries: EntriesResponse = app.query(QueryMsg::Entries { validator: None });
    assert_eq!(entries.validators.len(), 2);
    assert_eq!(entries.validators[0].entries, vec![(b.completion, id)]);
    // Nothing delegated left.
    assert_eq!(
        app.execute(&anyone, ExecuteMsg::ProcessBatch {}, &[]),
        Err(e(ContractError::NothingToDo))
    );
}

#[test]
fn unwind_refusals() {
    let mut app = setup();
    let seller = app.addr("seller");
    let anyone = app.addr("anyone");
    exit_instant(&mut app, &seller, 10_000 * SHARE).unwrap();
    let keeper = app.keeper.clone();
    app.execute(
        &keeper,
        ExecuteMsg::SetBatchTarget {
            shares: Uint128::new(10_000 * SHARE),
        },
        &[],
    )
    .unwrap();
    let unwind = ExecuteMsg::Unwind { shares: None };
    let before = position(&app);

    app.world.borrow_mut().vault.non_funded = 1;
    assert_eq!(
        app.execute(&anyone, unwind.clone(), &[]),
        Err(e(ContractError::QueueNotEmpty))
    );
    app.world.borrow_mut().vault.non_funded = 0;

    app.world.borrow_mut().vault.queue_withdrawals = true;
    assert_eq!(
        app.execute(&anyone, unwind.clone(), &[]),
        Err(e(ContractError::WithdrawalQueued))
    );
    app.world.borrow_mut().vault.queue_withdrawals = false;

    app.world.borrow_mut().hatom.burn_legs = (0..6).map(|i| format!("valoper_{i}")).collect();
    assert_eq!(
        app.execute(&anyone, unwind.clone(), &[]),
        Err(e(ContractError::TooManyLegs))
    );
    app.world.borrow_mut().hatom.burn_legs = vec!["valoper_a".into()];

    app.world.borrow_mut().hatom.burn_fee_bps = 51;
    assert_eq!(
        app.execute(&anyone, unwind.clone(), &[]),
        Err(e(ContractError::FeeTooHigh))
    );
    app.world.borrow_mut().hatom.burn_fee_bps = 50;

    // Every refusal changed nothing; the guard is clear.
    let p = position(&app);
    assert_eq!(p.inventory_shares, before.inventory_shares);
    assert_eq!(p.unwound_in_interval.u128(), 0);
    assert!(!OP_GUARD.exists(&app.deps.storage));
    assert_eq!(app.world.borrow().hatom.compounds, 0);

    // At the limit fee it goes through, and the fee is counted.
    app.execute(&anyone, unwind, &[]).unwrap();
    let p = position(&app);
    assert_eq!(p.inventory_shares.u128(), 0);
    assert!(p.totals.burn_fee_paid.u128() > 0);
    assert!(!OP_GUARD.exists(&app.deps.storage));
}

#[test]
fn process_batch_skips_and_spacing() {
    let mut app = setup();
    let anyone = app.addr("anyone");
    {
        let mut w = app.world.borrow_mut();
        let g = w.gate.clone();
        for (v, a) in [
            ("val_a", 300),
            ("val_b", 200),
            ("val_c", 100),
            ("val_d", 50),
        ] {
            w.delegations.insert((g.clone(), v.to_string()), a * ATOM);
        }
        w.refuse_undelegate.insert("val_b".into());
    }
    let admin = app.admin.clone();
    app.execute(
        &admin,
        ExecuteMsg::UpdateConfig {
            settings: SettingsUpdate {
                undelegate_max_validators: Some(2),
                ..Default::default()
            },
        },
        &[],
    )
    .unwrap();

    // Largest first, two at most: val_a and val_b; val_b is refused and skipped.
    app.execute(&anyone, ExecuteMsg::ProcessBatch {}, &[])
        .unwrap();
    assert_eq!(
        app.attrs_all("skipped_validator"),
        vec!["val_b".to_string()]
    );
    let b1: u64 = app.attr("batch_created").unwrap().parse().unwrap();
    // A follow-up at once takes the ones left out, as a new batch; val_a waits an interval.
    app.execute(&anyone, ExecuteMsg::ProcessBatch {}, &[])
        .unwrap();
    assert_eq!(
        app.attrs_all("skipped_validator"),
        vec!["val_b".to_string()]
    );
    let b2: u64 = app.attr("batch_created").unwrap().parse().unwrap();
    assert_ne!(b1, b2);
    let batches: BatchesResponse = app.query(QueryMsg::Batches {
        start_after: None,
        limit: None,
    });
    let amounts: Vec<u128> = batches.batches.iter().map(|b| b.amount.u128()).collect();
    assert_eq!(amounts, vec![300 * ATOM, 100 * ATOM]);
    // Only val_b and val_d are left; every undelegation refused or none due.
    app.world
        .borrow_mut()
        .delegations
        .insert((app.gate.to_string(), "val_a".into()), ATOM);
    app.execute(&anyone, ExecuteMsg::ProcessBatch {}, &[])
        .unwrap();
    assert_eq!(
        app.attr("undelegated"),
        Some(format!("val_d:{}", 50 * ATOM))
    );
    // val_a, val_c, val_d have entries from this interval; val_b is refused: nothing undelegated.
    assert_eq!(
        app.execute(&anyone, ExecuteMsg::ProcessBatch {}, &[]),
        Err(e(ContractError::NothingToDo))
    );
    app.world.borrow_mut().refuse_undelegate.clear();
    app.execute(&anyone, ExecuteMsg::ProcessBatch {}, &[])
        .unwrap();
    assert_eq!(
        app.attr("undelegated"),
        Some(format!("val_b:{}", 200 * ATOM))
    );
    // val_a again only after the interval.
    assert_eq!(
        app.execute(&anyone, ExecuteMsg::ProcessBatch {}, &[]),
        Err(e(ContractError::NotDue))
    );
    app.advance(3 * DAY);
    assert_eq!(
        app.execute(&anyone, ExecuteMsg::ProcessBatch {}, &[]),
        Err(e(ContractError::NotDue))
    );
    app.advance(3_600);
    app.execute(&anyone, ExecuteMsg::ProcessBatch {}, &[])
        .unwrap();
    assert_eq!(app.attr("undelegated"), Some(format!("val_a:{ATOM}")));
}

#[test]
fn max_entries_counted_from_real_completion_times() {
    // The chain's unbonding is longer than the settings say: the spacing is too short, and the
    // gate stops at `max_entries` live entries by itself.
    let mut app = setup();
    app.world.borrow_mut().unbonding_time_s = 42 * DAY;
    let anyone = app.addr("anyone");
    let g = app.gate.to_string();
    for _ in 0..7 {
        app.world
            .borrow_mut()
            .delegations
            .insert((g.clone(), "val_a".into()), ATOM);
        app.execute(&anyone, ExecuteMsg::ProcessBatch {}, &[])
            .unwrap();
        app.advance(3 * DAY + 3_600);
    }
    let entries: EntriesResponse = app.query(QueryMsg::Entries {
        validator: Some("val_a".into()),
    });
    assert_eq!(entries.validators[0].entries.len(), 7);
    app.world
        .borrow_mut()
        .delegations
        .insert((g.clone(), "val_a".into()), ATOM);
    assert_eq!(
        app.execute(&anyone, ExecuteMsg::ProcessBatch {}, &[]),
        Err(e(ContractError::NotDue))
    );
    // Once the first entry has matured (and the margin passed), the validator is eligible again.
    app.advance(42 * DAY - 7 * (3 * DAY + 3_600) + 60);
    app.execute(&anyone, ExecuteMsg::ProcessBatch {}, &[])
        .unwrap();
}

// ---------------------------------------------------------------------------
// Bookings and maturity

#[test]
fn book_exit_curve_capacity_and_claims() {
    let mut app = setup();
    let id = make_batch(&mut app, 10_000 * SHARE);
    let user = app.addr("user");
    let batches: BatchesResponse = app.query(QueryMsg::Batches {
        start_after: None,
        limit: None,
    });
    let b = batches.batches[0].clone();
    // The 94.9% capacity (max_slash_bps 510).
    assert_eq!(b.capacity, b.amount.multiply_ratio(9_490u128, 10_000u128));

    // Two days later: d(t) = d_book_min + (d_inst - d_book_min) × (1 - t / T).
    app.advance(2 * DAY);
    let shares = 1_000 * SHARE;
    let q: QuoteExitResponse = app.query(QueryMsg::QuoteExit {
        shares: Uint128::new(shares),
        route: ExitRoute::Booked { batch_id: id },
    });
    assert!(q.available, "{:?}", q.reason);
    let p = position(&app);
    let f = (p.free.u128() * 10_000 / (100_000 * ATOM)) as u64;
    let d_inst = crate::curve::value_at(&settings().exit_curve, f, crate::curve::Round::Up);
    let t = b.completion.seconds() - app.env().block.time.seconds();
    let d = 30 + ((d_inst - 30) as u128 * (21 * DAY - t) as u128).div_ceil(21 * DAY as u128) as u32;
    assert_eq!(q.discount_bps, Some(d));
    assert_eq!(q.maturity, Some(b.completion));
    assert_eq!(q.capacity_left, Some(b.capacity));
    let v = 1_000 * ATOM * 1_375 / 1_000;
    assert_eq!(q.atom.u128(), v * (10_000 - d as u128) / 10_000);

    app.give_shares(&user, shares);
    app.execute(
        &user,
        ExecuteMsg::BookExit {
            batch_id: id,
            recipient: None,
            min_out: Some(q.atom),
        },
        &[coin(shares, SHARES)],
    )
    .unwrap();
    let bookings: BookingsResponse = app.query(QueryMsg::Bookings {
        owner: Some(user.to_string()),
        batch_id: None,
        start_after: None,
        limit: None,
    });
    assert_eq!(bookings.bookings.len(), 1);
    assert_eq!(bookings.bookings[0].amount, q.atom);
    let booking_id = bookings.bookings[0].id;
    let p2 = position(&app);
    assert_eq!(p2.inventory_cost - p.inventory_cost, q.atom);
    assert_eq!(p2.free, p.free, "a booking does not draw on the buffer");

    // Capacity: a booking above what is left is refused.
    let left = b.capacity - q.atom;
    let big = (left.u128() + 1) * 1_000 / 1_375 * 10_100 / 10_000 + 10 * SHARE;
    app.give_shares(&user, big);
    assert_eq!(
        app.execute(
            &user,
            ExecuteMsg::BookExit {
                batch_id: id,
                recipient: None,
                min_out: None
            },
            &[coin(big, SHARES)],
        ),
        Err(e(ContractError::BatchFull))
    );
    assert_eq!(
        app.execute(
            &user,
            ExecuteMsg::BookExit {
                batch_id: id + 1,
                recipient: None,
                min_out: None
            },
            &[coin(SHARE, SHARES)],
        ),
        Err(e(ContractError::UnknownBatch))
    );

    // Not matured: the claim skips it.
    let anyone = app.addr("anyone");
    app.execute(
        &anyone,
        ExecuteMsg::ClaimBookings {
            booking_ids: vec![booking_id],
        },
        &[],
    )
    .unwrap();
    assert_eq!(app.bal(&user, BOND), 0);

    // Within the maturity margin: no more bookings.
    app.advance(t - 30);
    assert_eq!(
        app.execute(
            &user,
            ExecuteMsg::BookExit {
                batch_id: id,
                recipient: None,
                min_out: None
            },
            &[coin(SHARE, SHARES)],
        ),
        Err(e(ContractError::BatchMatured))
    );
    // Matured and the margin passed: reserved, then paid.
    app.advance(30 + 60);
    let p3 = position(&app);
    assert_eq!(p3.reserved, q.atom);
    assert_eq!(p3.free, p3.atom_balance - q.atom);
    assert!(p3.batches.is_empty());
    let batches: BatchesResponse = app.query(QueryMsg::Batches {
        start_after: None,
        limit: None,
    });
    assert!(batches.batches.is_empty());
    app.execute(
        &anyone,
        ExecuteMsg::ClaimBookings {
            booking_ids: vec![booking_id],
        },
        &[],
    )
    .unwrap();
    assert_eq!(app.bal(&user, BOND), q.atom.u128());
    assert_eq!(position(&app).reserved.u128(), 0);
    assert_eq!(
        app.execute(
            &anyone,
            ExecuteMsg::ClaimBookings {
                booking_ids: vec![booking_id]
            },
            &[]
        ),
        Ok(())
    );
    assert_eq!(app.attrs_all("not_found"), vec![booking_id.to_string()]);
    assert_eq!(app.bal(&user, BOND), q.atom.u128());
}

#[test]
fn claim_bookings_skips_unknown_and_claimed() {
    let mut app = setup();
    let id = make_batch(&mut app, 10_000 * SHARE);
    let (alice, bob) = (app.addr("alice"), app.addr("bob"));
    let book = |app: &mut App, user: &Addr| -> (u64, u128) {
        app.give_shares(user, 1_000 * SHARE);
        app.execute(
            user,
            ExecuteMsg::BookExit {
                batch_id: id,
                recipient: None,
                min_out: None,
            },
            &[coin(1_000 * SHARE, SHARES)],
        )
        .unwrap();
        (
            app.attr("booking_id").unwrap().parse().unwrap(),
            app.attr("atom_owed").unwrap().parse().unwrap(),
        )
    };
    let (a, owed_a) = book(&mut app, &alice);
    let (b, owed_b) = book(&mut app, &bob);
    app.advance(21 * DAY + 60);
    let anyone = app.addr("anyone");
    // A duplicate id in one call is paid once; an unknown id is skipped.
    app.execute(
        &anyone,
        ExecuteMsg::ClaimBookings {
            booking_ids: vec![a, a, 999],
        },
        &[],
    )
    .unwrap();
    assert_eq!(app.bal(&alice, BOND), owed_a);
    assert_eq!(app.attrs_all("paid"), vec![format!("{a}:{owed_a}")]);
    assert_eq!(
        app.attrs_all("not_found"),
        vec![a.to_string(), "999".to_string()]
    );
    assert_eq!(position(&app).reserved.u128(), owed_b);
    // An id claimed in an earlier call is skipped; the others are still paid.
    app.execute(
        &anyone,
        ExecuteMsg::ClaimBookings {
            booking_ids: vec![a, b],
        },
        &[],
    )
    .unwrap();
    assert_eq!(app.attrs_all("not_found"), vec![a.to_string()]);
    assert_eq!(app.bal(&alice, BOND), owed_a);
    assert_eq!(app.bal(&bob, BOND), owed_b);
    assert_eq!(position(&app).reserved.u128(), 0);
}

#[test]
fn instantiate_refuses_a_vault_of_another_denom() {
    let mut app = setup();
    app.world.borrow_mut().vault.deposit_denom = Some("uatom".into());
    let msg = crate::msg::InstantiateMsg {
        admin: app.admin.to_string(),
        operators: vec![],
        keeper: app.keeper.to_string(),
        profit_recipient: app.profit.to_string(),
        hatom: app.addr("hatom").to_string(),
        vault: app.addr("vault").to_string(),
        settings: settings(),
    };
    let info = cosmwasm_std::MessageInfo {
        sender: app.admin.clone(),
        funds: vec![],
    };
    let env = app.env();
    assert_eq!(
        crate::contract::instantiate(app.deps.as_mut(), env, info, msg),
        Err(ContractError::InvalidConfig(
            "the vault's deposit denom is not the hATOM denom"
        ))
    );
}

#[test]
fn booking_discount_falls_with_time() {
    let mut app = setup();
    let id = make_batch(&mut app, 10_000 * SHARE);
    let quote = |app: &App| -> u32 {
        let q: QuoteExitResponse = app.query(QueryMsg::QuoteExit {
            shares: Uint128::new(SHARE),
            route: ExitRoute::Booked { batch_id: id },
        });
        q.discount_bps.unwrap()
    };
    let instant: QuoteExitResponse = app.query(QueryMsg::QuoteExit {
        shares: Uint128::new(SHARE),
        route: ExitRoute::Instant,
    });
    // Right after the batch starts: exactly d_book_min (t = T).
    assert_eq!(quote(&app), 30);
    app.advance(10 * DAY);
    let mid = quote(&app);
    app.advance(10 * DAY);
    let late = quote(&app);
    assert!(30 < mid && mid < late, "{mid} {late}");
    assert!(late <= instant.discount_bps.unwrap() + 1);
}

#[test]
fn cursor_by_completion_reserved_and_profit() {
    let mut app = setup();
    let b1 = make_batch(&mut app, 10_000 * SHARE);
    // The chain's unbonding time drops to 14 days: the next batch matures before the first.
    app.world.borrow_mut().unbonding_time_s = 14 * DAY;
    app.advance(3 * DAY + 3_600);
    let b2 = make_batch(&mut app, 10_000 * SHARE);
    let user = app.addr("user");
    let book = |app: &mut App, id: u64| -> u128 {
        app.give_shares(&user, 1_000 * SHARE);
        app.execute(
            &user,
            ExecuteMsg::BookExit {
                batch_id: id,
                recipient: None,
                min_out: None,
            },
            &[coin(1_000 * SHARE, SHARES)],
        )
        .unwrap();
        app.attr("atom_owed").unwrap().parse().unwrap()
    };
    let owed1 = book(&mut app, b1);
    let owed2 = book(&mut app, b2);
    assert_eq!(position(&app).reserved.u128(), 0);

    // b2 (14 days) matures first: its bookings are reserved, b1's not yet.
    app.advance(14 * DAY + 60);
    let p = position(&app);
    assert_eq!(p.reserved.u128(), owed2);
    assert_eq!(p.batches.iter().map(|b| b.id).collect::<Vec<_>>(), vec![b1]);

    // WithdrawProfit and Sweep never touch reserved ATOM.
    let admin = app.admin.clone();
    let too_much = p.free + Uint128::one();
    assert_eq!(
        app.execute(
            &admin,
            ExecuteMsg::WithdrawProfit {
                atom: Some(too_much),
                shares: None
            },
            &[]
        ),
        Err(e(ContractError::BufferTooLow))
    );
    assert_eq!(
        app.execute(&admin, ExecuteMsg::Sweep { denom: BOND.into() }, &[]),
        Err(e(ContractError::Unauthorized))
    );
    app.execute(
        &admin,
        ExecuteMsg::WithdrawProfit {
            atom: Some(p.free),
            shares: None,
        },
        &[],
    )
    .unwrap();
    let profit = app.profit.clone();
    assert_eq!(app.bal(&profit, BOND), p.free.u128());
    let p = position(&app);
    assert_eq!(p.free.u128(), 0);
    assert_eq!(p.atom_balance.u128(), owed2);
    assert_eq!(p.totals.withdrawn_atom.u128(), app.bal(&profit, BOND));

    // b1 matures: both reserved; the claims pay them.
    app.advance(7 * DAY);
    let p = position(&app);
    assert_eq!(p.reserved.u128(), owed1 + owed2);
    let ids: Vec<u64> = {
        let r: BookingsResponse = app.query(QueryMsg::Bookings {
            owner: None,
            batch_id: None,
            start_after: None,
            limit: None,
        });
        r.bookings.iter().map(|b| b.id).collect()
    };
    let anyone = app.addr("anyone");
    app.execute(&anyone, ExecuteMsg::ClaimBookings { booking_ids: ids }, &[])
        .unwrap();
    assert_eq!(app.bal(&user, BOND), owed1 + owed2);
    assert_eq!(position(&app).reserved.u128(), 0);
}

#[test]
fn slashed_batch_still_pays_its_bookings() {
    let mut app = setup();
    let id = make_batch(&mut app, 10_000 * SHARE);
    let user = app.addr("user");
    let batches: BatchesResponse = app.query(QueryMsg::Batches {
        start_after: None,
        limit: None,
    });
    // Book the whole capacity.
    let cap = batches.batches[0].capacity.u128();
    let admin = app.admin.clone();
    app.execute(
        &admin,
        ExecuteMsg::UpdateConfig {
            settings: SettingsUpdate {
                exit_cap_tx: Some(Uint128::new(cap * 2)),
                ..Default::default()
            },
        },
        &[],
    )
    .unwrap();
    let shares = cap * 1_000 / 1_375 * 10_000 / 9_970;
    app.give_shares(&user, shares);
    app.execute(
        &user,
        ExecuteMsg::BookExit {
            batch_id: id,
            recipient: None,
            min_out: None,
        },
        &[coin(shares, SHARES)],
    )
    .unwrap();
    let owed: u128 = app.attr("atom_owed").unwrap().parse().unwrap();
    assert!(owed <= cap);
    // Withdraw the whole free buffer, then a 5.01% slash on the entries.
    let free = position(&app).free;
    app.execute(
        &admin,
        ExecuteMsg::WithdrawProfit {
            atom: Some(free),
            shares: None,
        },
        &[],
    )
    .unwrap();
    app.world.borrow_mut().slash_bps = 501;
    app.advance(21 * DAY + 60);
    let p = position(&app);
    assert_eq!(p.reserved.u128(), owed);
    assert!(p.atom_balance >= p.reserved);
    let booking = {
        let r: BookingsResponse = app.query(QueryMsg::Bookings {
            owner: None,
            batch_id: Some(id),
            start_after: None,
            limit: None,
        });
        r.bookings[0].id
    };
    app.execute(
        &user,
        ExecuteMsg::ClaimBookings {
            booking_ids: vec![booking],
        },
        &[],
    )
    .unwrap();
    assert_eq!(app.bal(&user, BOND), owed);
}

#[test]
fn sweep_and_withdraw_shares() {
    let mut app = setup();
    let seller = app.addr("seller");
    exit_instant(&mut app, &seller, 10_000 * SHARE).unwrap();
    let gate = app.gate.clone();
    let admin = app.admin.clone();
    let profit = app.profit.clone();
    // Shares sent outside a trade are not inventory: only those are swept.
    app.give_shares(&gate, 5 * SHARE);
    app.fund(&gate, HATOM, 3);
    app.fund(&gate, "ibc/stray", 9);
    app.execute(
        &admin,
        ExecuteMsg::Sweep {
            denom: SHARES.into(),
        },
        &[],
    )
    .unwrap();
    assert_eq!(app.bal(&profit, SHARES), 5 * SHARE);
    assert_eq!(
        app.execute(
            &admin,
            ExecuteMsg::Sweep {
                denom: SHARES.into()
            },
            &[]
        ),
        Err(e(ContractError::NothingToDo))
    );
    app.execute(
        &admin,
        ExecuteMsg::Sweep {
            denom: HATOM.into(),
        },
        &[],
    )
    .unwrap();
    app.execute(
        &admin,
        ExecuteMsg::Sweep {
            denom: "ibc/stray".into(),
        },
        &[],
    )
    .unwrap();
    assert_eq!(
        (app.bal(&profit, HATOM), app.bal(&profit, "ibc/stray")),
        (3, 9)
    );

    // Shares withdrawn at their average cost.
    let before = position(&app);
    app.execute(
        &admin,
        ExecuteMsg::WithdrawProfit {
            atom: None,
            shares: Some(Uint128::new(2_500 * SHARE)),
        },
        &[],
    )
    .unwrap();
    let after = position(&app);
    assert_eq!(after.inventory_shares.u128(), 7_500 * SHARE);
    assert_eq!(
        after.inventory_cost.u128(),
        before.inventory_cost.u128() - before.inventory_cost.u128().div_ceil(4)
    );
    assert_eq!(after.totals.withdrawn_shares.u128(), 2_500 * SHARE);
    assert_eq!(
        app.execute(
            &admin,
            ExecuteMsg::WithdrawProfit {
                atom: None,
                shares: Some(Uint128::new(7_501 * SHARE))
            },
            &[],
        ),
        Err(e(ContractError::NoInventory))
    );
}

// ---------------------------------------------------------------------------
// Guards

#[test]
fn pause_blocks_trades_not_claims() {
    let mut app = setup();
    let id = make_batch(&mut app, 10_000 * SHARE);
    let user = app.addr("user");
    app.give_shares(&user, 10 * SHARE);
    app.execute(
        &user,
        ExecuteMsg::BookExit {
            batch_id: id,
            recipient: None,
            min_out: None,
        },
        &[coin(SHARE, SHARES)],
    )
    .unwrap();
    let op = app.operator.clone();
    app.execute(&op, ExecuteMsg::Pause {}, &[]).unwrap();
    app.fund(&user, BOND, 10 * ATOM);
    let paused = Err(e(ContractError::Paused));
    let atom = [coin(ATOM, BOND)];
    let shares = [coin(SHARE, SHARES)];
    let none = ExecuteMsg::EnterSwap {
        recipient: None,
        min_out: None,
    };
    assert_eq!(app.execute(&user, none, &atom), paused);
    assert_eq!(
        app.execute(
            &user,
            ExecuteMsg::EnterDirect {
                recipient: None,
                min_out: None
            },
            &atom
        ),
        paused
    );
    assert_eq!(
        app.execute(
            &user,
            ExecuteMsg::ExitInstant {
                recipient: None,
                min_out: None
            },
            &shares
        ),
        paused
    );
    assert_eq!(
        app.execute(
            &user,
            ExecuteMsg::BookExit {
                batch_id: id,
                recipient: None,
                min_out: None
            },
            &shares
        ),
        paused
    );
    assert_eq!(
        app.execute(&user, ExecuteMsg::Unwind { shares: None }, &[]),
        paused
    );
    assert_eq!(app.execute(&user, ExecuteMsg::ProcessBatch {}, &[]), paused);
    let q: QuoteExitResponse = app.query(QueryMsg::QuoteExit {
        shares: Uint128::new(SHARE),
        route: ExitRoute::Instant,
    });
    assert_eq!(q.reason, Some(e(ContractError::Paused)));

    // Claims keep working.
    app.advance(21 * DAY + 60);
    app.execute(
        &user,
        ExecuteMsg::ClaimBookings {
            booking_ids: vec![1],
        },
        &[],
    )
    .unwrap();
    assert!(app.bal(&user, BOND) > 10 * ATOM);
}

#[test]
fn hatom_paused_refuses_priced_trades() {
    let mut app = setup();
    let user = app.addr("user");
    app.world.borrow_mut().hatom.paused = true;
    assert_eq!(
        exit_instant(&mut app, &user, SHARE),
        Err(e(ContractError::Paused))
    );
    let q: QuoteExitResponse = app.query(QueryMsg::QuoteExit {
        shares: Uint128::new(SHARE),
        route: ExitRoute::Staked,
    });
    assert!(!q.available);
    app.world.borrow_mut().hatom.paused = false;
    let q: QuoteExitResponse = app.query(QueryMsg::QuoteExit {
        shares: Uint128::new(SHARE),
        route: ExitRoute::Staked,
    });
    assert!(q.available);
    assert_eq!(q.hatom, Some(Uint128::new(1_100_000)));
    assert_eq!(q.atom.u128(), 1_375_000);
    assert_eq!(q.lsm_shares.unwrap().len(), 2);
    assert_eq!(q.instant_withdrawal, Some(true));
}

#[test]
fn price_guard() {
    let mut app = setup();
    let user = app.addr("user");
    // The first priced trade sets the reference without a check.
    exit_instant(&mut app, &user, SHARE).unwrap();
    // +1% is within price_jump_bps (100); +1.5% is not.
    app.world.borrow_mut().hatom.holders_value = 1_250_000_000_000 * 1_015 / 1_000;
    assert_eq!(
        exit_instant(&mut app, &user, SHARE),
        Err(e(ContractError::PriceJump))
    );
    // A refused trade changes nothing: still refused later the same day.
    app.advance(3_600);
    assert_eq!(
        exit_instant(&mut app, &user, SHARE),
        Err(e(ContractError::PriceJump))
    );
    // The drift allowance: 10 bps a day, so 5 days later 150 bps pass.
    app.advance(5 * DAY);
    exit_instant(&mut app, &user, SHARE).unwrap();

    // A jump; the operator's stated price must be within reset_tolerance_bps of P.
    app.world.borrow_mut().hatom.holders_value = 1_250_000_000_000 * 1_100 / 1_000;
    assert_eq!(
        exit_instant(&mut app, &user, SHARE),
        Err(e(ContractError::PriceJump))
    );
    let op = app.operator.clone();
    let p_now = 1_375_000 * 1_100 / 1_000;
    assert_eq!(
        app.execute(
            &op,
            ExecuteMsg::ResetPriceReference {
                price: Uint128::new(1_375_000)
            },
            &[]
        ),
        Err(e(ContractError::ResetTooFar))
    );
    let anyone = app.addr("anyone");
    assert_eq!(
        app.execute(
            &anyone,
            ExecuteMsg::ResetPriceReference {
                price: Uint128::new(p_now)
            },
            &[]
        ),
        Err(e(ContractError::Unauthorized))
    );
    app.execute(
        &op,
        ExecuteMsg::ResetPriceReference {
            price: Uint128::new(p_now + 100),
        },
        &[],
    )
    .unwrap();
    assert_eq!(app.attr("new_price_ref"), Some(p_now.to_string()));
    exit_instant(&mut app, &user, SHARE).unwrap();
    // EnterDirect is never priced by the gate.
    app.world.borrow_mut().hatom.holders_value = 1_250_000_000_000;
    app.fund(&user, BOND, ATOM);
    app.execute(
        &user,
        ExecuteMsg::EnterDirect {
            recipient: None,
            min_out: None,
        },
        &[coin(ATOM, BOND)],
    )
    .unwrap();
}

#[test]
fn operation_guard() {
    let mut app = setup();
    let user = app.addr("user");
    // A message arriving while an operation is open is refused.
    OP_GUARD.save(&mut app.deps.storage, &()).unwrap();
    assert_eq!(
        exit_instant(&mut app, &user, SHARE),
        Err(e(ContractError::OpInProgress))
    );
    assert_eq!(
        app.execute(
            &user,
            ExecuteMsg::ClaimBookings {
                booking_ids: vec![]
            },
            &[]
        ),
        Err(e(ContractError::OpInProgress))
    );
    let admin = app.admin.clone();
    assert_eq!(
        app.execute(&admin, ExecuteMsg::Pause {}, &[]),
        Err(e(ContractError::OpInProgress))
    );
    OP_GUARD.remove(&mut app.deps.storage);
    // Every operation clears it in its final self-call.
    app.fund(&user, BOND, ATOM);
    app.execute(
        &user,
        ExecuteMsg::EnterDirect {
            recipient: None,
            min_out: None,
        },
        &[coin(ATOM, BOND)],
    )
    .unwrap();
    assert!(!OP_GUARD.exists(&app.deps.storage));
    exit_instant(&mut app, &user, SHARE).unwrap();
    assert!(!OP_GUARD.exists(&app.deps.storage));
}

#[test]
fn migrate_keeps_state() {
    let mut app = setup();
    let user = app.addr("user");
    exit_instant(&mut app, &user, SHARE).unwrap();
    let env = app.env();
    let migrate = |app: &mut App| {
        crate::contract::migrate(app.deps.as_mut(), env.clone(), crate::msg::MigrateMsg {})
    };
    let name = crate::contract::CONTRACT_NAME;
    let current = crate::contract::CONTRACT_VERSION;
    // Same version, then a normal upgrade from an older one: both pass, and state is kept.
    migrate(&mut app).unwrap();
    cw2::set_contract_version(&mut app.deps.storage, name, "3.6.9").unwrap();
    migrate(&mut app).unwrap();
    assert_eq!(
        cw2::get_contract_version(&app.deps.storage)
            .unwrap()
            .version,
        current
    );
    assert_eq!(position(&app).price_ref, Some(Uint128::new(1_375_000)));
    // Semver order, not string order: 3.6.10 is newer than 3.6.9.
    let (major_minor, patch) = current.rsplit_once('.').unwrap();
    let newer = format!("{major_minor}.{}", patch.parse::<u64>().unwrap() + 10);
    cw2::set_contract_version(&mut app.deps.storage, name, &newer).unwrap();
    assert!(migrate(&mut app)
        .unwrap_err()
        .to_string()
        .contains("cannot migrate down"));
    // A stored version that does not parse is refused.
    cw2::set_contract_version(&mut app.deps.storage, name, "3.6").unwrap();
    assert!(migrate(&mut app)
        .unwrap_err()
        .to_string()
        .contains("invalid version"));
    cw2::set_contract_version(&mut app.deps.storage, name, current).unwrap();
    let env = app.env();
    cw2::set_contract_version(&mut app.deps.storage, "crates.io:other", "1.0.0").unwrap();
    assert!(
        crate::contract::migrate(app.deps.as_mut(), env.clone(), crate::msg::MigrateMsg {})
            .is_err()
    );
    cw2::set_contract_version(
        &mut app.deps.storage,
        crate::contract::CONTRACT_NAME,
        "999.0.0",
    )
    .unwrap();
    assert!(crate::contract::migrate(app.deps.as_mut(), env, crate::msg::MigrateMsg {}).is_err());
}

#[test]
fn set_batch_target_clamped() {
    let mut app = setup();
    let seller = app.addr("seller");
    exit_instant(&mut app, &seller, 10_000 * SHARE).unwrap();
    let admin = app.admin.clone();
    app.execute(
        &admin,
        ExecuteMsg::UpdateConfig {
            settings: SettingsUpdate {
                batch_target_min_bps: Some(1_000),
                batch_target_max_bps: Some(5_000),
                ..Default::default()
            },
        },
        &[],
    )
    .unwrap();
    let keeper = app.keeper.clone();
    app.execute(
        &keeper,
        ExecuteMsg::SetBatchTarget {
            shares: Uint128::new(u128::MAX),
        },
        &[],
    )
    .unwrap();
    assert_eq!(position(&app).batch_target.u128(), 5_000 * SHARE);
    app.execute(
        &keeper,
        ExecuteMsg::SetBatchTarget {
            shares: Uint128::one(),
        },
        &[],
    )
    .unwrap();
    assert_eq!(position(&app).batch_target.u128(), 1_000 * SHARE);
}
