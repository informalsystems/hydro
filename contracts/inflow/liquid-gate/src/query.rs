//! Queries (SPEC §10). The quotes run the same plans as the trades, so a quote fails exactly as
//! the trade would; a failure is "not available" with the reason.

use cosmwasm_std::{to_json_binary, Binary, Deps, Env, Order, StdResult, Uint128};
use cw_storage_plus::Bound;
use interface::inflow_control_center::{
    ConfigResponse as CcConfigResponse, PoolInfoResponse as CcPoolInfoResponse,
    QueryMsg as CcQueryMsg,
};
use interface::inflow_vault::{ConfigResponse as VaultConfigResponse, QueryMsg as VaultQueryMsg};

use crate::batch::all_entries;
use crate::error::ContractError;
use crate::maturity::is_matured;
use crate::msg::{
    BatchInfo, BatchesResponse, BookingsResponse, EntriesResponse, EntryRoute, ExitRoute,
    PositionResponse, QueryMsg, QuoteEntryResponse, QuoteExitResponse, ValidatorEntries,
};
use crate::pricing::{self, dry_run_deposit, shares_value_hatom, simulate_burn, simulate_mint};
use crate::state::{
    Batch, Config, BATCHES, BATCH_TARGET, BOOKINGS, BOOKINGS_BY_BATCH, BOOKINGS_BY_OWNER,
    BY_COMPLETION, CONFIG, INVENTORY, PRICE_REF, TOTALS, UNWOUND_IN_INTERVAL,
};
use crate::trade::{free_view, inventory_value, plan_book, plan_instant, plan_swap};

const DEFAULT_LIMIT: u32 = 30;
const MAX_LIMIT: u32 = 100;

pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    let cfg = CONFIG.load(deps.storage)?;
    match msg {
        QueryMsg::Config {} => to_json_binary(&cfg),
        QueryMsg::QuoteEntry { amount, route } => {
            to_json_binary(&quote_entry(deps, &env, &cfg, amount, route)?)
        }
        QueryMsg::QuoteExit { shares, route } => {
            to_json_binary(&quote_exit(deps, &env, &cfg, shares, route)?)
        }
        QueryMsg::Batches { start_after, limit } => {
            to_json_binary(&batches(deps, &env, start_after, limit)?)
        }
        QueryMsg::Bookings {
            owner,
            batch_id,
            start_after,
            limit,
        } => to_json_binary(&bookings(deps, owner, batch_id, start_after, limit)?),
        QueryMsg::Position {} => to_json_binary(&position(deps, &env, &cfg)?),
        QueryMsg::Entries { validator } => {
            let all = all_entries(deps.storage, env.block.time)?;
            let validators = all
                .into_iter()
                .filter(|(v, _)| validator.as_ref().is_none_or(|w| w == v))
                .map(|(validator, entries)| ValidatorEntries { validator, entries })
                .collect();
            to_json_binary(&EntriesResponse { validators })
        }
    }
}

fn unavailable<T: Default + Reasoned>(e: impl ToString) -> T {
    let mut t = T::default();
    t.set_reason(e.to_string());
    t
}

trait Reasoned {
    fn set_reason(&mut self, r: String);
}
impl Reasoned for QuoteEntryResponse {
    fn set_reason(&mut self, r: String) {
        self.available = false;
        self.reason = Some(r);
    }
}
impl Reasoned for QuoteExitResponse {
    fn set_reason(&mut self, r: String) {
        self.available = false;
        self.reason = Some(r);
    }
}

fn quote_entry(
    deps: Deps,
    env: &Env,
    cfg: &Config,
    amount: Uint128,
    route: EntryRoute,
) -> StdResult<QuoteEntryResponse> {
    if cfg.paused {
        return Ok(unavailable(ContractError::Paused));
    }
    match route {
        EntryRoute::Direct => {
            // hATOM's refusals (paused, NoMintTarget, ImbalanceTooHigh) as "not available".
            let sim = match simulate_mint(&deps.querier, cfg, amount) {
                Ok(s) => s,
                Err(e) => return Ok(unavailable(e)),
            };
            let shares = match dry_run_deposit(&deps.querier, cfg, sim.minted) {
                Ok(s) => s,
                Err(e) => return Ok(unavailable(e)),
            };
            let within_cap = within_deposit_cap(deps, cfg, sim.minted)?;
            let mut r = QuoteEntryResponse {
                available: within_cap && !shares.is_zero(),
                reason: None,
                shares,
                bonus_bps: None,
                hatom_minted: Some(sim.minted),
                imbalance_fee: Some(sim.imbalance_fee),
                imbalance_rebate: Some(sim.imbalance_rebate),
                within_deposit_cap: Some(within_cap),
            };
            if !within_cap {
                r.reason = Some("above the vault's deposit cap".into());
            } else if shares.is_zero() {
                r.reason = Some(ContractError::ZeroOutput.to_string());
            }
            Ok(r)
        }
        EntryRoute::Swap => {
            let (free, _) = free_view(deps, env, cfg)?;
            match plan_swap(deps, env, cfg, amount, free) {
                Ok(p) => Ok(QuoteEntryResponse {
                    available: true,
                    shares: p.shares,
                    bonus_bps: Some(p.bonus_bps),
                    ..Default::default()
                }),
                Err(e) => Ok(unavailable(e)),
            }
        }
    }
}

/// Whether the vault's deposit cap allows depositing `hatom` (the vault refuses when its pool
/// value with the deposit is above the cap; the deposit is valued at hATOM's rate).
fn within_deposit_cap(deps: Deps, cfg: &Config, hatom: Uint128) -> StdResult<bool> {
    let vault_cfg: VaultConfigResponse = deps
        .querier
        .query_wasm_smart(&cfg.vault, &VaultQueryMsg::Config {})?;
    let cc = vault_cfg.config.control_center_contract;
    let cap = deps
        .querier
        .query_wasm_smart::<CcConfigResponse>(&cc, &CcQueryMsg::Config {})?
        .config
        .deposit_cap;
    let pool: CcPoolInfoResponse = deps
        .querier
        .query_wasm_smart(&cc, &CcQueryMsg::PoolInfo {})?;
    let rate = pricing::rate(&deps.querier, cfg)?;
    Ok(pool.total_pool_value.saturating_add(rate.value_of(hatom)) <= cap)
}

fn quote_exit(
    deps: Deps,
    env: &Env,
    cfg: &Config,
    shares: Uint128,
    route: ExitRoute,
) -> StdResult<QuoteExitResponse> {
    match route {
        ExitRoute::Instant => {
            if cfg.paused {
                return Ok(unavailable(ContractError::Paused));
            }
            let (free, _) = free_view(deps, env, cfg)?;
            match plan_instant(deps, env, cfg, shares, free) {
                Ok(p) => Ok(QuoteExitResponse {
                    available: true,
                    atom: p.payout,
                    value: Some(p.value),
                    discount_bps: Some(p.discount_bps),
                    ..Default::default()
                }),
                Err(e) => Ok(unavailable(e)),
            }
        }
        ExitRoute::Booked { batch_id } => {
            if cfg.paused {
                return Ok(unavailable(ContractError::Paused));
            }
            let (free, m) = free_view(deps, env, cfg)?;
            match plan_book(deps, env, cfg, batch_id, shares, free, &m) {
                Ok(p) => Ok(QuoteExitResponse {
                    available: true,
                    atom: p.exit.payout,
                    value: Some(p.exit.value),
                    discount_bps: Some(p.exit.discount_bps),
                    maturity: Some(p.batch.completion),
                    capacity_left: Some(p.batch.capacity.saturating_sub(p.batch.booked)),
                    ..Default::default()
                }),
                Err(e) => Ok(unavailable(e)),
            }
        }
        ExitRoute::Staked => {
            let hatom = match shares_value_hatom(&deps.querier, cfg, shares) {
                Ok(h) => h,
                Err(e) => return Ok(unavailable(e)),
            };
            let sim = match simulate_burn(&deps.querier, cfg, hatom, None) {
                Ok(s) => s,
                Err(e) => return Ok(unavailable(e)),
            };
            // Conservative: the vault's own balance only, not what automated adapters could pay
            // (coordinator's answer, 2026-11-10).
            let available_now: Uint128 = deps
                .querier
                .query_wasm_smart(&cfg.vault, &VaultQueryMsg::AvailableForDeployment {})?;
            Ok(QuoteExitResponse {
                available: true,
                atom: sim.out,
                hatom: Some(hatom),
                lsm_shares: Some(sim.allocation),
                imbalance_fee: Some(sim.imbalance_fee),
                imbalance_rebate: Some(sim.imbalance_rebate),
                instant_withdrawal: Some(available_now >= hatom),
                ..Default::default()
            })
        }
    }
}

/// The batches in flight (not matured yet), by id.
fn batches(
    deps: Deps,
    env: &Env,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<BatchesResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let m = crate::maturity::matured_view(deps.storage, env.block.time)?;
    let batches = BATCHES
        .range(
            deps.storage,
            start_after.map(Bound::exclusive),
            None,
            Order::Ascending,
        )
        .filter(|r| !matches!(r, Ok((_, b)) if is_matured(&m, b)))
        .take(limit)
        .map(|r| r.map(|(_, b)| BatchInfo::from(&b)))
        .collect::<StdResult<_>>()?;
    Ok(BatchesResponse { batches })
}

fn bookings(
    deps: Deps,
    owner: Option<String>,
    batch_id: Option<u64>,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<BookingsResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let start = start_after.map(Bound::exclusive);
    let ids: Vec<u64> = match (&owner, batch_id) {
        (Some(o), _) => {
            let o = deps.api.addr_validate(o)?;
            BOOKINGS_BY_OWNER
                .prefix(&o)
                .keys(deps.storage, start, None, Order::Ascending)
                .collect::<StdResult<_>>()?
        }
        (None, Some(b)) => BOOKINGS_BY_BATCH
            .prefix(b)
            .keys(deps.storage, start, None, Order::Ascending)
            .take(limit)
            .collect::<StdResult<_>>()?,
        (None, None) => BOOKINGS
            .keys(deps.storage, start, None, Order::Ascending)
            .take(limit)
            .collect::<StdResult<_>>()?,
    };
    let mut bookings = vec![];
    for id in ids {
        let b = BOOKINGS.load(deps.storage, id)?;
        if batch_id.is_some_and(|x| x != b.batch_id) {
            continue;
        }
        bookings.push(b);
        if bookings.len() >= limit {
            break;
        }
    }
    Ok(BookingsResponse { bookings })
}

fn position(deps: Deps, env: &Env, cfg: &Config) -> StdResult<PositionResponse> {
    let (free, m) = free_view(deps, env, cfg)?;
    let atom_balance = pricing::balance(&deps.querier, &env.contract.address, &cfg.bond_denom)?;
    let inv = INVENTORY.load(deps.storage)?;
    let delegated = deps
        .querier
        .query_all_delegations(&env.contract.address)?
        .into_iter()
        .map(|d| d.amount)
        .collect();
    let start = m.cursor.map(Bound::exclusive);
    let mut in_flight = vec![];
    for key in BY_COMPLETION.keys(deps.storage, start, None, Order::Ascending) {
        let (_, id) = key?;
        let b: Batch = BATCHES.load(deps.storage, id)?;
        in_flight.push(BatchInfo::from(&b));
    }
    let price_ref = PRICE_REF.may_load(deps.storage)?;
    Ok(PositionResponse {
        atom_balance,
        free,
        reserved: m.reserved,
        inventory_shares: inv.shares,
        inventory_value: inventory_value(deps, cfg)?,
        inventory_cost: inv.cost,
        delegated,
        batches: in_flight,
        totals: TOTALS.load(deps.storage)?,
        price_ref: price_ref.as_ref().map(|p| p.price),
        price_ref_time: price_ref.map(|p| p.time),
        batch_target: BATCH_TARGET.may_load(deps.storage)?.unwrap_or_default(),
        unwound_in_interval: UNWOUND_IN_INTERVAL
            .may_load(deps.storage)?
            .unwrap_or_default(),
    })
}
