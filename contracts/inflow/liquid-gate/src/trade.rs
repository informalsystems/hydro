//! Entries, instant exits and booked exits (SPEC §3, §4), and their cost basis (§8).
//!
//! Each priced trade is planned by a read-only `plan_*` function, shared with the quotes (§10),
//! then applied by its handler.

use cosmwasm_std::{
    coins, to_json_binary, Addr, BankMsg, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
    Storage, SubMsg, Uint128, WasmMsg,
};
use interface::inflow_vault::ExecuteMsg as VaultExecuteMsg;

use crate::curve::{self, Round};
use crate::error::ContractError;
use crate::hatom::{HatomExecuteMsg, MintResult, RateResponse};
use crate::maturity::{self, free_buffer, is_matured, matured_view, MATURITY_MARGIN_S};
use crate::msg::SwapCheck;
use crate::pricing::{self, dry_run_deposit, max_shares_for, priced_inputs, value};
use crate::state::{
    Batch, Booking, Config, ExitWindow, Inventory, Matured, PendingDirect, PriceRef, BATCHES,
    BOOKINGS, BOOKINGS_BY_BATCH, BOOKINGS_BY_OWNER, EXIT_WINDOW, INVENTORY, NEXT_BOOKING_ID,
    OP_GUARD, PENDING_DIRECT, PRICE_REF, TOTALS,
};

pub const REPLY_MINT: u64 = 1;

/// The cost basis of `s` of the inventory's shares, rounded up (so the cost left is never
/// overstated): `⌈cost × s / shares⌉`.
pub fn cost_out(inv: &Inventory, s: Uint128) -> Result<Uint128, ContractError> {
    if s > inv.shares {
        return Err(ContractError::NoInventory);
    }
    if s.is_zero() {
        return Ok(Uint128::zero());
    }
    Ok(inv.cost.checked_mul_ceil((s, inv.shares))?)
}

pub fn recipient_or_sender(
    deps: Deps,
    info: &MessageInfo,
    recipient: Option<String>,
) -> StdResult<Addr> {
    match recipient {
        Some(r) => deps.api.addr_validate(&r),
        None => Ok(info.sender.clone()),
    }
}

fn check_min_out(got: Uint128, min_out: Option<Uint128>) -> Result<(), ContractError> {
    match min_out {
        Some(min) if got < min => Err(ContractError::BelowMinimum { got, min }),
        _ => Ok(()),
    }
}

/// The final self-call (SPEC §9).
pub fn assert_msg(env: &Env, swap: Option<SwapCheck>) -> StdResult<WasmMsg> {
    Ok(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&crate::msg::ExecuteMsg::AssertInvariants { swap })?,
        funds: vec![],
    })
}

fn save_price_ref(storage: &mut dyn Storage, env: &Env, p: Uint128) -> StdResult<()> {
    PRICE_REF.save(
        storage,
        &PriceRef {
            price: p,
            time: env.block.time,
        },
    )
}

// ---------------------------------------------------------------------------
// Exit caps

/// The caps of every exit (instant or booked): per transaction, per window, and the inventory
/// cap on the inventory with the exit's shares added. Returns the window as it will be saved.
fn exit_caps(
    deps: Deps,
    env: &Env,
    cfg: &Config,
    rate: &RateResponse,
    shares: Uint128,
    v: Uint128,
) -> Result<ExitWindow, ContractError> {
    let s = &cfg.settings;
    if v > s.exit_cap_tx {
        return Err(ContractError::CapExceeded { which: "tx".into() });
    }
    let now = env.block.time;
    let mut w = EXIT_WINDOW.may_load(deps.storage)?.unwrap_or(ExitWindow {
        start: now,
        used: Uint128::zero(),
    });
    if now.seconds() >= w.start.seconds() + s.window_s {
        w = ExitWindow {
            start: now,
            used: Uint128::zero(),
        };
    }
    w.used = w.used.checked_add(v)?;
    if w.used > s.exit_cap_window {
        return Err(ContractError::CapExceeded {
            which: "window".into(),
        });
    }
    let inv = INVENTORY.load(deps.storage)?;
    let after = value(&deps.querier, cfg, rate, inv.shares.checked_add(shares)?)?;
    if after > s.inventory_cap {
        return Err(ContractError::CapExceeded {
            which: "inventory".into(),
        });
    }
    Ok(w)
}

// ---------------------------------------------------------------------------
// Plans

pub struct SwapPlan {
    pub shares: Uint128,
    pub bonus_bps: u32,
    pub value: Uint128,
    pub cost_out: Uint128,
    pub price: Uint128,
}

/// `EnterSwap` of `a` ATOM with `free` the free buffer before the ATOM is added.
pub fn plan_swap(
    deps: Deps,
    env: &Env,
    cfg: &Config,
    a: Uint128,
    free: Uint128,
) -> Result<SwapPlan, ContractError> {
    let (rate, p) = priced_inputs(&deps.querier, deps.storage, env, cfg)?;
    let s = &cfg.settings;
    let f0 = curve::fill_bps(free, s.buffer_target, &s.entry_curve, Round::Up);
    let f1 = curve::fill_bps(
        free.checked_add(a)?,
        s.buffer_target,
        &s.entry_curve,
        Round::Up,
    );
    let bonus_bps = curve::average(&s.entry_curve, f0, f1, Round::Down);
    let target = curve::apply_bonus(a, bonus_bps);
    let shares = max_shares_for(&deps.querier, cfg, &rate, target)?;
    if shares.is_zero() {
        return Err(ContractError::ZeroOutput);
    }
    let inv = INVENTORY.load(deps.storage)?;
    let cost = cost_out(&inv, shares)?;
    if a < cost {
        return Err(ContractError::BelowCost);
    }
    let v = value(&deps.querier, cfg, &rate, shares)?;
    Ok(SwapPlan {
        shares,
        bonus_bps,
        value: v,
        cost_out: cost,
        price: p,
    })
}

pub struct ExitPlan {
    pub value: Uint128,
    pub discount_bps: u32,
    pub payout: Uint128,
    pub window: ExitWindow,
    pub price: Uint128,
}

/// `ExitInstant` of `shares`, with `free` the free buffer.
pub fn plan_instant(
    deps: Deps,
    env: &Env,
    cfg: &Config,
    shares: Uint128,
    free: Uint128,
) -> Result<ExitPlan, ContractError> {
    let (rate, p) = priced_inputs(&deps.querier, deps.storage, env, cfg)?;
    let v = value(&deps.querier, cfg, &rate, shares)?;
    if v.is_zero() {
        return Err(ContractError::ZeroOutput);
    }
    if v > free {
        return Err(ContractError::BufferTooLow);
    }
    let window = exit_caps(deps, env, cfg, &rate, shares, v)?;
    let s = &cfg.settings;
    let f_hi = curve::fill_bps(free, s.buffer_target, &s.exit_curve, Round::Down);
    let f_lo = curve::fill_bps(free - v, s.buffer_target, &s.exit_curve, Round::Down);
    let d = curve::average(&s.exit_curve, f_lo, f_hi, Round::Up);
    let payout = curve::apply_discount(v, d);
    if payout.is_zero() {
        return Err(ContractError::ZeroOutput);
    }
    Ok(ExitPlan {
        value: v,
        discount_bps: d,
        payout,
        window,
        price: p,
    })
}

pub struct BookPlan {
    pub exit: ExitPlan,
    pub batch: Batch,
}

/// `BookExit` of `shares` on `batch_id`, with `free` the free buffer and `m` the cursor.
pub fn plan_book(
    deps: Deps,
    env: &Env,
    cfg: &Config,
    batch_id: u64,
    shares: Uint128,
    free: Uint128,
    m: &Matured,
) -> Result<BookPlan, ContractError> {
    let batch = BATCHES
        .may_load(deps.storage, batch_id)?
        .ok_or(ContractError::UnknownBatch)?;
    let now = env.block.time;
    if is_matured(m, &batch) || batch.completion <= now.plus_seconds(MATURITY_MARGIN_S) {
        return Err(ContractError::BatchMatured);
    }
    let (rate, p) = priced_inputs(&deps.querier, deps.storage, env, cfg)?;
    let v = value(&deps.querier, cfg, &rate, shares)?;
    if v.is_zero() {
        return Err(ContractError::ZeroOutput);
    }
    let window = exit_caps(deps, env, cfg, &rate, shares, v)?;
    let s = &cfg.settings;
    let f = curve::fill_bps(free, s.buffer_target, &s.exit_curve, Round::Down);
    let d_inst = curve::value_at(&s.exit_curve, f, Round::Up);
    let t = batch.completion.seconds().saturating_sub(now.seconds());
    let d = curve::booking_discount(d_inst, s.d_book_min_bps, t, s.book_horizon_s);
    let owed = curve::apply_discount(v, d);
    if owed.is_zero() {
        return Err(ContractError::ZeroOutput);
    }
    if owed > batch.capacity.saturating_sub(batch.booked) {
        return Err(ContractError::BatchFull);
    }
    Ok(BookPlan {
        exit: ExitPlan {
            value: v,
            discount_bps: d,
            payout: owed,
            window,
            price: p,
        },
        batch,
    })
}

// ---------------------------------------------------------------------------
// Handlers

/// `EnterDirect`: mint hATOM to the gate; the reply deposits it for the recipient.
pub fn enter_direct(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cfg: &Config,
    recipient: Option<String>,
    min_out: Option<Uint128>,
) -> Result<Response, ContractError> {
    let amount = cw_utils::must_pay(&info, &cfg.bond_denom)?;
    let recipient = recipient_or_sender(deps.as_ref(), &info, recipient)?;
    PENDING_DIRECT.save(
        deps.storage,
        &PendingDirect {
            recipient: recipient.clone(),
            min_out,
        },
    )?;
    OP_GUARD.save(deps.storage, &())?;
    let mint = WasmMsg::Execute {
        contract_addr: cfg.hatom.to_string(),
        msg: to_json_binary(&HatomExecuteMsg::MintFromNative {
            validator: None,
            recipient: Some(env.contract.address.to_string()),
            max_legs: None,
            min_out: None,
        })?,
        funds: coins(amount.u128(), &cfg.bond_denom),
    };
    Ok(Response::new()
        .add_attribute("action", "enter_direct")
        .add_attribute("recipient", recipient)
        .add_attribute("atom_in", amount)
        .add_submessage(SubMsg::reply_on_success(mint, REPLY_MINT))
        .add_message(assert_msg(&env, None)?))
}

/// The mint's reply: check the shares the deposit gives, then deposit for the recipient.
pub fn on_minted(
    deps: DepsMut,
    cfg: &Config,
    data: cosmwasm_std::Binary,
) -> Result<Response, ContractError> {
    let MintResult { minted } = cosmwasm_std::from_json(&data)?;
    let pending = PENDING_DIRECT.load(deps.storage)?;
    PENDING_DIRECT.remove(deps.storage);
    let shares = dry_run_deposit(&deps.querier, cfg, minted)?;
    if shares.is_zero() {
        return Err(ContractError::ZeroOutput);
    }
    check_min_out(shares, pending.min_out)?;
    let deposit = WasmMsg::Execute {
        contract_addr: cfg.vault.to_string(),
        msg: to_json_binary(&VaultExecuteMsg::Deposit {
            on_behalf_of: Some(pending.recipient.to_string()),
        })?,
        funds: coins(minted.u128(), &cfg.hatom_denom),
    };
    Ok(Response::new()
        .add_attribute("hatom_minted", minted)
        .add_attribute("shares_expected", shares)
        .add_message(deposit))
}

pub fn enter_swap(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cfg: &Config,
    recipient: Option<String>,
    min_out: Option<Uint128>,
) -> Result<Response, ContractError> {
    let a = cw_utils::must_pay(&info, &cfg.bond_denom)?;
    let recipient = recipient_or_sender(deps.as_ref(), &info, recipient)?;
    let m = maturity::advance(deps.storage, env.block.time)?;
    // The ATOM sent is already in the balance: the fill before the entry leaves it out.
    let free = free_buffer(&deps.querier, &env, cfg, &m, a)?;
    let plan = plan_swap(deps.as_ref(), &env, cfg, a, free)?;
    check_min_out(plan.shares, min_out)?;

    INVENTORY.update(deps.storage, |mut inv| -> StdResult<_> {
        inv.shares -= plan.shares;
        inv.cost -= plan.cost_out;
        Ok(inv)
    })?;
    TOTALS.update(deps.storage, |mut t| -> StdResult<_> {
        t.bonus_paid += plan.value.saturating_sub(a);
        Ok(t)
    })?;
    save_price_ref(deps.storage, &env, plan.price)?;

    Ok(Response::new()
        .add_attribute("action", "enter_swap")
        .add_attribute("recipient", recipient.as_str())
        .add_attribute("atom_in", a)
        .add_attribute("shares_out", plan.shares)
        .add_attribute("bonus_bps", plan.bonus_bps.to_string())
        .add_attribute("value", plan.value)
        .add_attribute("cost_out", plan.cost_out)
        .add_message(BankMsg::Send {
            to_address: recipient.to_string(),
            amount: coins(plan.shares.u128(), &cfg.share_denom),
        })
        .add_message(assert_msg(
            &env,
            Some(SwapCheck {
                received: a,
                cost_out: plan.cost_out,
            }),
        )?))
}

fn record_exit(
    storage: &mut dyn Storage,
    env: &Env,
    shares: Uint128,
    plan: &ExitPlan,
) -> StdResult<()> {
    INVENTORY.update(storage, |mut inv| -> StdResult<_> {
        inv.shares += shares;
        inv.cost += plan.payout;
        Ok(inv)
    })?;
    TOTALS.update(storage, |mut t| -> StdResult<_> {
        t.discount_earned += plan.value - plan.payout;
        Ok(t)
    })?;
    EXIT_WINDOW.save(storage, &plan.window)?;
    save_price_ref(storage, env, plan.price)
}

pub fn exit_instant(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cfg: &Config,
    recipient: Option<String>,
    min_out: Option<Uint128>,
) -> Result<Response, ContractError> {
    let shares = cw_utils::must_pay(&info, &cfg.share_denom)?;
    let recipient = recipient_or_sender(deps.as_ref(), &info, recipient)?;
    let m = maturity::advance(deps.storage, env.block.time)?;
    let free = free_buffer(&deps.querier, &env, cfg, &m, Uint128::zero())?;
    let plan = plan_instant(deps.as_ref(), &env, cfg, shares, free)?;
    check_min_out(plan.payout, min_out)?;
    record_exit(deps.storage, &env, shares, &plan)?;

    Ok(Response::new()
        .add_attribute("action", "exit_instant")
        .add_attribute("recipient", recipient.as_str())
        .add_attribute("shares_in", shares)
        .add_attribute("value", plan.value)
        .add_attribute("discount_bps", plan.discount_bps.to_string())
        .add_attribute("atom_out", plan.payout)
        .add_message(BankMsg::Send {
            to_address: recipient.to_string(),
            amount: coins(plan.payout.u128(), &cfg.bond_denom),
        })
        .add_message(assert_msg(&env, None)?))
}

#[allow(clippy::too_many_arguments)]
pub fn book_exit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cfg: &Config,
    batch_id: u64,
    recipient: Option<String>,
    min_out: Option<Uint128>,
) -> Result<Response, ContractError> {
    let shares = cw_utils::must_pay(&info, &cfg.share_denom)?;
    let recipient = recipient_or_sender(deps.as_ref(), &info, recipient)?;
    let m = maturity::advance(deps.storage, env.block.time)?;
    let free = free_buffer(&deps.querier, &env, cfg, &m, Uint128::zero())?;
    let BookPlan { exit, mut batch } =
        plan_book(deps.as_ref(), &env, cfg, batch_id, shares, free, &m)?;
    check_min_out(exit.payout, min_out)?;
    record_exit(deps.storage, &env, shares, &exit)?;

    batch.booked += exit.payout;
    BATCHES.save(deps.storage, batch.id, &batch)?;
    let id = NEXT_BOOKING_ID.may_load(deps.storage)?.unwrap_or(1);
    NEXT_BOOKING_ID.save(deps.storage, &(id + 1))?;
    let booking = Booking {
        id,
        batch_id,
        owner: recipient.clone(),
        amount: exit.payout,
    };
    BOOKINGS.save(deps.storage, id, &booking)?;
    BOOKINGS_BY_BATCH.save(deps.storage, (batch_id, id), &())?;
    BOOKINGS_BY_OWNER.save(deps.storage, (&recipient, id), &())?;

    Ok(Response::new()
        .add_attribute("action", "book_exit")
        .add_attribute("booking_id", id.to_string())
        .add_attribute("batch_id", batch_id.to_string())
        .add_attribute("owner", recipient.as_str())
        .add_attribute("shares_in", shares)
        .add_attribute("value", exit.value)
        .add_attribute("discount_bps", exit.discount_bps.to_string())
        .add_attribute("atom_owed", exit.payout)
        .add_attribute("maturity", batch.completion.seconds().to_string())
        .add_message(assert_msg(&env, None)?))
}

/// `ClaimBookings` (anyone): pays each listed booking of a matured batch, each by its own send.
/// Bookings of batches not matured yet are skipped (attribute `not_matured`), and so are unknown
/// or already claimed ids, a repeat in the same call included (attribute `not_found`).
pub fn claim_bookings(
    deps: DepsMut,
    env: Env,
    cfg: &Config,
    booking_ids: Vec<u64>,
) -> Result<Response, ContractError> {
    let mut m = maturity::advance(deps.storage, env.block.time)?;
    let mut res = Response::new().add_attribute("action", "claim_bookings");
    for id in booking_ids {
        let Some(booking) = BOOKINGS.may_load(deps.storage, id)? else {
            res = res.add_attribute("not_found", id.to_string());
            continue;
        };
        let mut batch = BATCHES.load(deps.storage, booking.batch_id)?;
        if !is_matured(&m, &batch) {
            res = res.add_attribute("not_matured", id.to_string());
            continue;
        }
        batch.claimed += booking.amount;
        BATCHES.save(deps.storage, batch.id, &batch)?;
        m.reserved = m.reserved.checked_sub(booking.amount)?;
        BOOKINGS.remove(deps.storage, id);
        BOOKINGS_BY_BATCH.remove(deps.storage, (booking.batch_id, id));
        BOOKINGS_BY_OWNER.remove(deps.storage, (&booking.owner, id));
        res = res
            .add_attribute("paid", format!("{id}:{}", booking.amount))
            .add_message(BankMsg::Send {
                to_address: booking.owner.to_string(),
                amount: coins(booking.amount.u128(), &cfg.bond_denom),
            });
    }
    crate::state::MATURED.save(deps.storage, &m)?;
    Ok(res.add_message(assert_msg(&env, None)?))
}

/// The free buffer as a query sees it (the cursor advanced without saving).
pub fn free_view(deps: Deps, env: &Env, cfg: &Config) -> StdResult<(Uint128, Matured)> {
    let m = matured_view(deps.storage, env.block.time)?;
    let free = free_buffer(&deps.querier, env, cfg, &m, Uint128::zero())?;
    Ok((free, m))
}

/// `V` of the whole inventory, for `Position`.
pub fn inventory_value(deps: Deps, cfg: &Config) -> StdResult<Uint128> {
    let inv = INVENTORY.load(deps.storage)?;
    let rate = pricing::rate(&deps.querier, cfg)?;
    value(&deps.querier, cfg, &rate, inv.shares)
}
