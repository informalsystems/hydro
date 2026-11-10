//! Unwinding and batches (SPEC §6): `Unwind` turns inventory shares into the gate's own
//! delegations, `ProcessBatch` undelegates them into a batch whose ATOM backs bookings.

use cosmwasm_std::{
    coins, to_json_binary, DepsMut, Env, Order, Response, StakingMsg, StdResult, Storage, SubMsg,
    SubMsgResponse, Timestamp, Uint128, WasmMsg,
};
use interface::inflow_vault::ExecuteMsg as VaultExecuteMsg;

use crate::chain;
use crate::error::ContractError;
use crate::hatom::{BurnResult, HatomExecuteMsg};
use crate::maturity::{completion_key, MATURITY_MARGIN_S};
use crate::pricing::{balance, shares_value_hatom, simulate_burn, withdrawal_queue};
use crate::settings::BPS;
use crate::state::{
    Batch, Config, PendingBatch, PendingUnwind, BATCHES, BATCH_TARGET, BY_COMPLETION, ENTRIES,
    INVENTORY, LAST_BATCH_TIME, NEXT_BATCH_ID, OP_GUARD, PENDING_BATCH, PENDING_UNWIND, TOTALS,
    UNWOUND_IN_INTERVAL,
};
use crate::trade::{assert_msg, cost_out};

pub const REPLY_COMPOUND: u64 = 2;
pub const REPLY_WITHDRAW: u64 = 3;
pub const REPLY_BURN: u64 = 4;
/// `REPLY_UNDELEGATE + i` is the reply of the `i`-th undelegation of a `ProcessBatch`.
pub const REPLY_UNDELEGATE: u64 = 1_000;

// ---------------------------------------------------------------------------
// Unwind

/// `Unwind { shares? }` steps (1) and (2), then the `Compound` (3).
pub fn unwind(
    deps: DepsMut,
    env: Env,
    cfg: &Config,
    shares: Option<Uint128>,
) -> Result<Response, ContractError> {
    let inv = INVENTORY.load(deps.storage)?;
    let target = BATCH_TARGET.may_load(deps.storage)?.unwrap_or_default();
    let unwound = UNWOUND_IN_INTERVAL
        .may_load(deps.storage)?
        .unwrap_or_default();
    let left = target.saturating_sub(unwound);

    // (1) The amount: what is left of the target in this interval, never above the inventory.
    let shares = match shares {
        Some(s) => {
            if s > left {
                return Err(ContractError::CapExceeded {
                    which: "batch_target".into(),
                });
            }
            if s > inv.shares {
                return Err(ContractError::NoInventory);
            }
            s
        }
        None => left.min(inv.shares),
    };
    if shares.is_zero() {
        return Err(ContractError::NothingToDo);
    }

    // (2) An instant withdrawal would take hATOM ahead of unfunded queued withdrawals.
    if !withdrawal_queue(&deps.querier, cfg)?
        .info
        .non_funded_withdrawal_amount
        .is_zero()
    {
        return Err(ContractError::QueueNotEmpty);
    }

    // The shares leave the inventory with their cost basis.
    let cost = cost_out(&inv, shares)?;
    INVENTORY.update(deps.storage, |mut i| -> StdResult<_> {
        i.shares -= shares;
        i.cost -= cost;
        Ok(i)
    })?;
    UNWOUND_IN_INTERVAL.save(deps.storage, &(unwound + shares))?;

    PENDING_UNWIND.save(
        deps.storage,
        &PendingUnwind {
            shares,
            atom_before: balance(&deps.querier, &env.contract.address, &cfg.bond_denom)?,
            expected: Uint128::zero(),
            hatom_before: Uint128::zero(),
        },
    )?;
    OP_GUARD.save(deps.storage, &())?;

    // (3) Compound first, so the burn is paid at a rate that includes the pending rewards. Its
    // failure never blocks the unwind (`reply_always`, the error ignored).
    let compound = WasmMsg::Execute {
        contract_addr: cfg.hatom.to_string(),
        msg: to_json_binary(&HatomExecuteMsg::Compound {})?,
        funds: vec![],
    };
    Ok(Response::new()
        .add_attribute("action", "unwind")
        .add_attribute("shares", shares)
        .add_attribute("cost_out", cost)
        .add_submessage(SubMsg::reply_always(compound, REPLY_COMPOUND))
        .add_message(assert_msg(&env, None)?))
}

/// The Compound's reply, whatever its result: read what the vault pays and withdraw.
pub fn on_compound(
    deps: DepsMut,
    env: Env,
    cfg: &Config,
    result: Result<SubMsgResponse, String>,
) -> Result<Response, ContractError> {
    let mut res = Response::new();
    if let Err(e) = result {
        res = res.add_attribute("compound_skipped", e);
    }
    let mut p = PENDING_UNWIND.load(deps.storage)?;
    p.expected = shares_value_hatom(&deps.querier, cfg, p.shares)?;
    if p.expected.is_zero() {
        return Err(ContractError::ZeroOutput);
    }
    p.hatom_before = balance(&deps.querier, &env.contract.address, &cfg.hatom_denom)?;
    PENDING_UNWIND.save(deps.storage, &p)?;

    let withdraw = WasmMsg::Execute {
        contract_addr: cfg.vault.to_string(),
        msg: to_json_binary(&VaultExecuteMsg::Withdraw { on_behalf_of: None })?,
        funds: coins(p.shares.u128(), &cfg.share_denom),
    };
    Ok(res
        .add_attribute("hatom_expected", p.expected)
        .add_submessage(SubMsg::reply_on_success(withdraw, REPLY_WITHDRAW)))
}

/// The withdrawal's reply: it must have paid exactly `expected` hATOM; then steps (4) and (5).
pub fn on_withdrawn(deps: DepsMut, env: Env, cfg: &Config) -> Result<Response, ContractError> {
    let p = PENDING_UNWIND.load(deps.storage)?;
    let now = balance(&deps.querier, &env.contract.address, &cfg.hatom_denom)?;
    if now.checked_sub(p.hatom_before).ok() != Some(p.expected) {
        return Err(ContractError::WithdrawalQueued);
    }

    // (4) The burn's plan, in the same transaction.
    let s = &cfg.settings;
    let sim = simulate_burn(&deps.querier, cfg, p.expected, Some(s.burn_max_legs))?;
    let legs = sim
        .allocation
        .iter()
        .filter(|a| !a.amount.is_zero())
        .count();
    if legs > s.burn_max_legs as usize {
        return Err(ContractError::TooManyLegs);
    }
    if sim.imbalance_fee.u128() * BPS as u128 > sim.value.u128() * s.unwind_max_fee_bps as u128 {
        return Err(ContractError::FeeTooHigh);
    }
    TOTALS.update(deps.storage, |mut t| -> StdResult<_> {
        t.burn_bonus_received += sim.imbalance_rebate;
        t.burn_fee_paid += sim.imbalance_fee;
        Ok(t)
    })?;

    // (5) The burn, to LSM shares owned by the gate.
    let me = env.contract.address.to_string();
    let burn = WasmMsg::Execute {
        contract_addr: cfg.hatom.to_string(),
        msg: to_json_binary(&HatomExecuteMsg::Burn {
            recipient: Some(me.clone()),
            share_owner: Some(me),
            max_legs: Some(s.burn_max_legs),
            split: None,
            min_out: Some(sim.out),
        })?,
        funds: coins(p.expected.u128(), &cfg.hatom_denom),
    };
    Ok(Response::new()
        .add_attribute("burn_value", sim.value)
        .add_attribute("burn_out", sim.out)
        .add_attribute("imbalance_fee", sim.imbalance_fee)
        .add_attribute("imbalance_rebate", sim.imbalance_rebate)
        .add_attribute("legs", legs.to_string())
        .add_submessage(SubMsg::reply_on_success(burn, REPLY_BURN)))
}

/// The burn's reply: redeem each LSM share into the gate's delegations.
pub fn on_burned(env: Env, data: cosmwasm_std::Binary) -> Result<Response, ContractError> {
    let BurnResult { shares } = cosmwasm_std::from_json(&data)?;
    let me = env.contract.address.to_string();
    let mut res = Response::new();
    for coin in shares.iter().filter(|c| !c.amount.is_zero()) {
        res = res
            .add_attribute("redeem", coin.to_string())
            .add_message(chain::redeem_tokens(&me, coin));
    }
    Ok(res)
}

/// The unwind's part of the final self-call: the staking rewards auto-claimed on the redeems.
pub fn finish_unwind(storage: &mut dyn Storage, atom_now: Uint128) -> StdResult<Option<Uint128>> {
    let Some(p) = PENDING_UNWIND.may_load(storage)? else {
        return Ok(None);
    };
    PENDING_UNWIND.remove(storage);
    let rewards = atom_now.saturating_sub(p.atom_before);
    TOTALS.update(storage, |mut t| -> StdResult<_> {
        t.rewards_received += rewards;
        Ok(t)
    })?;
    Ok(Some(rewards))
}

// ---------------------------------------------------------------------------
// ProcessBatch

/// Drops the entries that are no longer live (`now ≥ completion + maturity_margin`).
pub fn live_entries(entries: Vec<(Timestamp, u64)>, now: Timestamp) -> Vec<(Timestamp, u64)> {
    entries
        .into_iter()
        .filter(|(c, _)| now < c.plus_seconds(MATURITY_MARGIN_S))
        .collect()
}

/// `ProcessBatch {}`: undelegate, largest first, up to `undelegate_max_validators`, from each
/// validator with no entry from a batch created in the last `batch_interval_s` and fewer than
/// `max_entries` live entries.
pub fn process_batch(deps: DepsMut, env: Env, cfg: &Config) -> Result<Response, ContractError> {
    let s = &cfg.settings;
    let now = env.block.time;

    // A new interval starts when the previous batch is older than `batch_interval_s`.
    let last = LAST_BATCH_TIME.may_load(deps.storage)?;
    if last.is_none_or(|t| now.seconds() >= t.seconds() + s.batch_interval_s) {
        UNWOUND_IN_INTERVAL.save(deps.storage, &Uint128::zero())?;
    }

    let mut delegations: Vec<_> = deps
        .querier
        .query_all_delegations(&env.contract.address)?
        .into_iter()
        .filter(|d| d.amount.denom == cfg.bond_denom && !d.amount.amount.is_zero())
        .collect();
    if delegations.is_empty() {
        return Err(ContractError::NothingToDo);
    }
    delegations.sort_by(|a, b| {
        b.amount
            .amount
            .cmp(&a.amount.amount)
            .then_with(|| a.validator.cmp(&b.validator))
    });

    let mut chosen = vec![];
    for d in delegations {
        if chosen.len() >= s.undelegate_max_validators as usize {
            break;
        }
        let stored = ENTRIES.may_load(deps.storage, &d.validator)?;
        let entries = live_entries(stored.clone().unwrap_or_default(), now);
        if let Some(st) = &stored {
            if st.len() != entries.len() {
                if entries.is_empty() {
                    ENTRIES.remove(deps.storage, &d.validator);
                } else {
                    ENTRIES.save(deps.storage, &d.validator, &entries)?;
                }
            }
        }
        // (b) fewer than `max_entries` live entries.
        if entries.len() >= s.max_entries as usize {
            continue;
        }
        // (a) no entry from a batch created in the last `batch_interval_s`.
        let mut recent = false;
        for (_, batch_id) in &entries {
            let b = BATCHES.load(deps.storage, *batch_id)?;
            if now.seconds() < b.created.seconds() + s.batch_interval_s {
                recent = true;
                break;
            }
        }
        if recent {
            continue;
        }
        chosen.push(d);
    }
    if chosen.is_empty() {
        return Err(ContractError::NotDue);
    }

    let batch_id = NEXT_BATCH_ID.may_load(deps.storage)?.unwrap_or(1);
    PENDING_BATCH.save(
        deps.storage,
        &PendingBatch {
            batch_id,
            atom_before: balance(&deps.querier, &env.contract.address, &cfg.bond_denom)?,
            validators: chosen.iter().map(|d| d.validator.clone()).collect(),
            done: vec![],
        },
    )?;
    OP_GUARD.save(deps.storage, &())?;

    let mut res = Response::new()
        .add_attribute("action", "process_batch")
        .add_attribute("batch_id", batch_id.to_string());
    for (i, d) in chosen.into_iter().enumerate() {
        res = res.add_submessage(SubMsg::reply_always(
            StakingMsg::Undelegate {
                validator: d.validator,
                amount: d.amount,
            },
            REPLY_UNDELEGATE + i as u64,
        ));
    }
    Ok(res.add_message(assert_msg(&env, None)?))
}

/// One undelegation's reply: a refused one is skipped, a successful one recorded.
pub fn on_undelegated(
    deps: DepsMut,
    index: usize,
    result: Result<SubMsgResponse, String>,
) -> Result<Response, ContractError> {
    let mut p = PENDING_BATCH.load(deps.storage)?;
    let validator = p
        .validators
        .get(index)
        .cloned()
        .ok_or_else(|| cosmwasm_std::StdError::generic_err("unknown undelegation reply"))?;
    match result {
        Err(e) => Ok(Response::new()
            .add_attribute("skipped_validator", validator)
            .add_attribute("skipped_reason", e)),
        Ok(r) => {
            let (completion, amount) = chain::undelegate_response(&r)?;
            p.done.push((validator.clone(), completion, amount.amount));
            PENDING_BATCH.save(deps.storage, &p)?;
            Ok(Response::new()
                .add_attribute("undelegated", format!("{validator}:{}", amount.amount))
                .add_attribute("completion", completion.seconds().to_string()))
        }
    }
}

/// The batch's part of the final self-call: create the batch (refused with nothing undelegated),
/// record its entries, and count the auto-claimed rewards.
pub fn finish_batch(
    storage: &mut dyn Storage,
    cfg: &Config,
    now: Timestamp,
    atom_now: Uint128,
) -> Result<Option<Batch>, ContractError> {
    let Some(p) = PENDING_BATCH.may_load(storage)? else {
        return Ok(None);
    };
    PENDING_BATCH.remove(storage);
    if p.done.is_empty() {
        return Err(ContractError::NothingToDo);
    }
    let amount: Uint128 = p.done.iter().map(|(_, _, a)| *a).sum();
    let completion = p.done.iter().map(|(_, c, _)| *c).max().unwrap_or(now);
    let capacity = amount.multiply_ratio(BPS - cfg.settings.max_slash_bps, BPS);
    let batch = Batch {
        id: p.batch_id,
        created: now,
        completion,
        amount,
        capacity,
        booked: Uint128::zero(),
        claimed: Uint128::zero(),
    };
    BATCHES.save(storage, batch.id, &batch)?;
    BY_COMPLETION.save(storage, completion_key(&batch), &())?;
    NEXT_BATCH_ID.save(storage, &(batch.id + 1))?;
    LAST_BATCH_TIME.save(storage, &now)?;
    for (validator, c, _) in &p.done {
        let mut e = live_entries(
            ENTRIES.may_load(storage, validator)?.unwrap_or_default(),
            now,
        );
        e.push((*c, batch.id));
        ENTRIES.save(storage, validator, &e)?;
    }
    let rewards = atom_now.saturating_sub(p.atom_before);
    TOTALS.update(storage, |mut t| -> StdResult<_> {
        t.unbonded += amount;
        t.rewards_received += rewards;
        Ok(t)
    })?;
    Ok(Some(batch))
}

/// A validator and its live `(completion_time, batch_id)` entries.
pub type ValidatorLiveEntries = (String, Vec<(Timestamp, u64)>);

/// Every validator's live entries (for the `Entries` query).
pub fn all_entries(storage: &dyn Storage, now: Timestamp) -> StdResult<Vec<ValidatorLiveEntries>> {
    ENTRIES
        .range(storage, None, None, Order::Ascending)
        .map(|r| r.map(|(v, e)| (v, live_entries(e, now))))
        .filter(|r| !matches!(r, Ok((_, e)) if e.is_empty()))
        .collect()
}
