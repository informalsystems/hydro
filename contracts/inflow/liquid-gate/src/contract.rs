//! Entry points, roles (SPEC §1), the admin messages, profit (§8) and the invariants (§9).

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    coins, Addr, BankMsg, Binary, Deps, DepsMut, Env, MessageInfo, Order, Reply, Response,
    StdError, StdResult, Uint128,
};
use interface::inflow_vault::{ConfigResponse as VaultConfigResponse, QueryMsg as VaultQueryMsg};

use crate::batch::{self, REPLY_BURN, REPLY_COMPOUND, REPLY_UNDELEGATE, REPLY_WITHDRAW};
use crate::chain;
use crate::error::ContractError;
use crate::maturity::{self, free_buffer};
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, SettingsUpdate, SwapCheck};
use crate::pricing::{self, balance, hatom_config};
use crate::settings::BPS;
use crate::state::{
    Config, Inventory, Totals, BATCHES, BATCH_TARGET, BY_COMPLETION, CONFIG, INVENTORY, MATURED,
    OP_GUARD, PENDING_DIRECT, PRICE_REF, TOTALS, UNWOUND_IN_INTERVAL,
};
use crate::trade::{self, assert_msg, cost_out, REPLY_MINT};

pub const CONTRACT_NAME: &str = "crates.io:liquid-gate";
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

fn validate_all(deps: Deps, addrs: &[String]) -> StdResult<Vec<Addr>> {
    let mut out: Vec<Addr> = vec![];
    for a in addrs {
        let a = deps.api.addr_validate(a)?;
        if !out.contains(&a) {
            out.push(a);
        }
    }
    Ok(out)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    cw_utils::nonpayable(&info)?;
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    msg.settings.validate()?;

    let hatom = deps.api.addr_validate(&msg.hatom)?;
    let vault = deps.api.addr_validate(&msg.vault)?;
    let hatom_denom = deps
        .querier
        .query_wasm_smart::<crate::hatom::HatomConfig>(
            &hatom,
            &crate::hatom::HatomQueryMsg::Config {},
        )?
        .denom;
    let vault_cfg: VaultConfigResponse = deps
        .querier
        .query_wasm_smart(&vault, &VaultQueryMsg::Config {})?;
    if vault_cfg.config.deposit_denom != hatom_denom {
        return Err(ContractError::InvalidConfig(
            "the vault's deposit denom is not the hATOM denom",
        ));
    }
    let bond_denom = deps.querier.query_bonded_denom()?;

    let cfg = Config {
        admin: deps.api.addr_validate(&msg.admin)?,
        pending_admin: None,
        operators: validate_all(deps.as_ref(), &msg.operators)?,
        keeper: deps.api.addr_validate(&msg.keeper)?,
        profit_recipient: deps.api.addr_validate(&msg.profit_recipient)?,
        hatom,
        vault,
        hatom_denom,
        share_denom: vault_cfg.config.vault_shares_denom,
        bond_denom,
        paused: false,
        settings: msg.settings,
    };
    CONFIG.save(deps.storage, &cfg)?;
    INVENTORY.save(deps.storage, &Inventory::default())?;
    TOTALS.save(deps.storage, &Totals::default())?;
    MATURED.save(deps.storage, &Default::default())?;
    BATCH_TARGET.save(deps.storage, &Uint128::zero())?;
    UNWOUND_IN_INTERVAL.save(deps.storage, &Uint128::zero())?;

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("hatom_denom", &cfg.hatom_denom)
        .add_attribute("share_denom", &cfg.share_denom)
        .add_attribute("bond_denom", &cfg.bond_denom))
}

/// Refuses another contract or an older version; keeps all state (`price_ref` included).
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    let stored = cw2::get_contract_version(deps.storage)?;
    let v = |s: &str| {
        s.split('.')
            .map(|x| x.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    if stored.contract != CONTRACT_NAME || v(&stored.version) > v(CONTRACT_VERSION) {
        return Err(StdError::generic_err(format!(
            "cannot migrate from {} {}",
            stored.contract, stored.version
        ))
        .into());
    }
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::new()
        .add_attribute("action", "migrate")
        .add_attribute("from_version", stored.version)
        .add_attribute("to_version", CONTRACT_VERSION))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    // Funds: only the four trades take them (exactly one coin, checked by their handlers).
    if !matches!(
        msg,
        ExecuteMsg::EnterDirect { .. }
            | ExecuteMsg::EnterSwap { .. }
            | ExecuteMsg::ExitInstant { .. }
            | ExecuteMsg::BookExit { .. }
    ) {
        cw_utils::nonpayable(&info)?;
    }

    // The operation guard: nothing starts between an operation's execute and its final self-call.
    if OP_GUARD.exists(deps.storage) && !matches!(msg, ExecuteMsg::AssertInvariants { .. }) {
        return Err(ContractError::OpInProgress);
    }

    let mut cfg = CONFIG.load(deps.storage)?;

    let pausable = matches!(
        msg,
        ExecuteMsg::EnterSwap { .. }
            | ExecuteMsg::ExitInstant { .. }
            | ExecuteMsg::BookExit { .. }
            | ExecuteMsg::EnterDirect { .. }
            | ExecuteMsg::Unwind { .. }
            | ExecuteMsg::ProcessBatch {}
    );
    if pausable && cfg.paused {
        return Err(ContractError::Paused);
    }

    let admin = info.sender == cfg.admin;
    let operator = admin || cfg.operators.contains(&info.sender);
    let allowed = match &msg {
        ExecuteMsg::UpdateConfig { .. }
        | ExecuteMsg::SetOperators { .. }
        | ExecuteMsg::SetKeeper { .. }
        | ExecuteMsg::TransferAdmin { .. }
        | ExecuteMsg::Unpause {}
        | ExecuteMsg::WithdrawProfit { .. }
        | ExecuteMsg::Sweep { .. } => admin,
        ExecuteMsg::Pause {} | ExecuteMsg::ResetPriceReference { .. } => operator,
        ExecuteMsg::SetBatchTarget { .. } => admin || info.sender == cfg.keeper,
        ExecuteMsg::AcceptAdmin {} => cfg.pending_admin.as_ref() == Some(&info.sender),
        ExecuteMsg::AssertInvariants { .. } => info.sender == env.contract.address,
        _ => true,
    };
    if !allowed {
        return Err(ContractError::Unauthorized);
    }

    match msg {
        ExecuteMsg::EnterDirect { recipient, min_out } => {
            trade::enter_direct(deps, env, info, &cfg, recipient, min_out)
        }
        ExecuteMsg::EnterSwap { recipient, min_out } => {
            trade::enter_swap(deps, env, info, &cfg, recipient, min_out)
        }
        ExecuteMsg::ExitInstant { recipient, min_out } => {
            trade::exit_instant(deps, env, info, &cfg, recipient, min_out)
        }
        ExecuteMsg::BookExit {
            batch_id,
            recipient,
            min_out,
        } => trade::book_exit(deps, env, info, &cfg, batch_id, recipient, min_out),
        ExecuteMsg::ClaimBookings { booking_ids } => {
            trade::claim_bookings(deps, env, &cfg, booking_ids)
        }
        ExecuteMsg::Unwind { shares } => batch::unwind(deps, env, &cfg, shares),
        ExecuteMsg::ProcessBatch {} => batch::process_batch(deps, env, &cfg),
        ExecuteMsg::UpdateConfig { settings } => update_config(deps, &mut cfg, settings),
        ExecuteMsg::SetOperators { operators } => {
            cfg.operators = validate_all(deps.as_ref(), &operators)?;
            CONFIG.save(deps.storage, &cfg)?;
            Ok(Response::new()
                .add_attribute("action", "set_operators")
                .add_attribute("operators", operators.join(",")))
        }
        ExecuteMsg::SetKeeper { keeper } => {
            cfg.keeper = deps.api.addr_validate(&keeper)?;
            CONFIG.save(deps.storage, &cfg)?;
            Ok(Response::new()
                .add_attribute("action", "set_keeper")
                .add_attribute("keeper", keeper))
        }
        ExecuteMsg::TransferAdmin { admin } => {
            cfg.pending_admin = Some(deps.api.addr_validate(&admin)?);
            CONFIG.save(deps.storage, &cfg)?;
            Ok(Response::new()
                .add_attribute("action", "transfer_admin")
                .add_attribute("pending_admin", admin))
        }
        ExecuteMsg::AcceptAdmin {} => {
            cfg.admin = info.sender;
            cfg.pending_admin = None;
            CONFIG.save(deps.storage, &cfg)?;
            Ok(Response::new()
                .add_attribute("action", "accept_admin")
                .add_attribute("admin", cfg.admin))
        }
        ExecuteMsg::Pause {} => {
            cfg.paused = true;
            CONFIG.save(deps.storage, &cfg)?;
            Ok(Response::new()
                .add_attribute("action", "pause")
                .add_attribute("by", info.sender))
        }
        ExecuteMsg::Unpause {} => {
            cfg.paused = false;
            CONFIG.save(deps.storage, &cfg)?;
            Ok(Response::new().add_attribute("action", "unpause"))
        }
        ExecuteMsg::ResetPriceReference { price } => reset_price_reference(deps, env, &cfg, price),
        ExecuteMsg::SetBatchTarget { shares } => set_batch_target(deps, &cfg, shares),
        ExecuteMsg::WithdrawProfit { atom, shares } => {
            withdraw_profit(deps, env, &cfg, atom, shares)
        }
        ExecuteMsg::Sweep { denom } => sweep(deps, env, &cfg, denom),
        ExecuteMsg::AssertInvariants { swap } => assert_invariants(deps, env, &cfg, swap),
    }
}

fn update_config(
    deps: DepsMut,
    cfg: &mut Config,
    u: SettingsUpdate,
) -> Result<Response, ContractError> {
    if let Some(r) = &u.profit_recipient {
        cfg.profit_recipient = deps.api.addr_validate(r)?;
    }
    let settings = cfg.settings.apply(u);
    settings.validate()?;
    cfg.settings = settings;
    CONFIG.save(deps.storage, cfg)?;
    Ok(Response::new().add_attribute("action", "update_config"))
}

/// `ResetPriceReference { price }` (SPEC §5): refused when `|P - price| / price` is above
/// `reset_tolerance_bps`, otherwise `price_ref = P`.
fn reset_price_reference(
    deps: DepsMut,
    env: Env,
    cfg: &Config,
    price: Uint128,
) -> Result<Response, ContractError> {
    let rate = pricing::rate(&deps.querier, cfg)?;
    let p = pricing::price(&deps.querier, cfg, &rate)?;
    if price.is_zero()
        || p.abs_diff(price).full_mul(BPS) > price.full_mul(cfg.settings.reset_tolerance_bps)
    {
        return Err(ContractError::ResetTooFar);
    }
    let old = PRICE_REF.may_load(deps.storage)?;
    PRICE_REF.save(
        deps.storage,
        &crate::state::PriceRef {
            price: p,
            time: env.block.time,
        },
    )?;
    Ok(Response::new()
        .add_attribute("action", "reset_price_reference")
        .add_attribute(
            "old_price_ref",
            old.map(|o| o.price.to_string())
                .unwrap_or_else(|| "none".into()),
        )
        .add_attribute("new_price_ref", p))
}

/// `SetBatchTarget { shares }`: clamped to `[batch_target_min_bps, batch_target_max_bps]` of
/// the inventory.
fn set_batch_target(
    deps: DepsMut,
    cfg: &Config,
    shares: Uint128,
) -> Result<Response, ContractError> {
    let inv = INVENTORY.load(deps.storage)?;
    let s = &cfg.settings;
    let lo = inv.shares.multiply_ratio(s.batch_target_min_bps, BPS);
    let hi = inv.shares.multiply_ratio(s.batch_target_max_bps, BPS);
    let target = shares.clamp(lo, hi);
    BATCH_TARGET.save(deps.storage, &target)?;
    Ok(Response::new()
        .add_attribute("action", "set_batch_target")
        .add_attribute("asked", shares)
        .add_attribute("batch_target", target))
}

/// `WithdrawProfit { atom?, shares? }` (SPEC §8): ATOM from `free`, shares from the inventory
/// at their average cost, to `profit_recipient`. Never reserved ATOM.
fn withdraw_profit(
    deps: DepsMut,
    env: Env,
    cfg: &Config,
    atom: Option<Uint128>,
    shares: Option<Uint128>,
) -> Result<Response, ContractError> {
    let atom = atom.unwrap_or_default();
    let shares = shares.unwrap_or_default();
    if atom.is_zero() && shares.is_zero() {
        return Err(ContractError::NothingToDo);
    }
    let m = maturity::advance(deps.storage, env.block.time)?;
    let mut res = Response::new().add_attribute("action", "withdraw_profit");
    if !atom.is_zero() {
        let free = free_buffer(&deps.querier, &env, cfg, &m, Uint128::zero())?;
        if atom > free {
            return Err(ContractError::BufferTooLow);
        }
        res = res.add_attribute("atom", atom).add_message(BankMsg::Send {
            to_address: cfg.profit_recipient.to_string(),
            amount: coins(atom.u128(), &cfg.bond_denom),
        });
    }
    if !shares.is_zero() {
        let inv = INVENTORY.load(deps.storage)?;
        let cost = cost_out(&inv, shares)?;
        INVENTORY.save(
            deps.storage,
            &Inventory {
                shares: inv.shares - shares,
                cost: inv.cost - cost,
            },
        )?;
        res = res
            .add_attribute("shares", shares)
            .add_attribute("cost_out", cost)
            .add_message(BankMsg::Send {
                to_address: cfg.profit_recipient.to_string(),
                amount: coins(shares.u128(), &cfg.share_denom),
            });
    }
    TOTALS.update(deps.storage, |mut t| -> StdResult<_> {
        t.withdrawn_atom += atom;
        t.withdrawn_shares += shares;
        Ok(t)
    })?;
    Ok(res.add_message(assert_msg(&env, None)?))
}

/// `Sweep { denom }`: a stray balance to `profit_recipient`. Never ATOM; for the share denom,
/// only the balance above the inventory.
fn sweep(deps: DepsMut, env: Env, cfg: &Config, denom: String) -> Result<Response, ContractError> {
    if denom == cfg.bond_denom {
        return Err(ContractError::Unauthorized);
    }
    let bal = balance(&deps.querier, &env.contract.address, &denom)?;
    let amount = if denom == cfg.share_denom {
        bal.saturating_sub(INVENTORY.load(deps.storage)?.shares)
    } else {
        bal
    };
    if amount.is_zero() {
        return Err(ContractError::NothingToDo);
    }
    Ok(Response::new()
        .add_attribute("action", "sweep")
        .add_attribute("denom", &denom)
        .add_attribute("amount", amount)
        .add_message(BankMsg::Send {
            to_address: cfg.profit_recipient.to_string(),
            amount: coins(amount.u128(), &denom),
        })
        .add_message(assert_msg(&env, None)?))
}

/// The final self-call (SPEC §9): finishes the operation in progress, checks the invariants and
/// clears the operation guard.
fn assert_invariants(
    deps: DepsMut,
    env: Env,
    cfg: &Config,
    swap: Option<SwapCheck>,
) -> Result<Response, ContractError> {
    let now = env.block.time;
    let atom = balance(&deps.querier, &env.contract.address, &cfg.bond_denom)?;
    let mut res = Response::new().add_attribute("action", "assert_invariants");

    if let Some(rewards) = batch::finish_unwind(deps.storage, atom)? {
        res = res.add_attribute("rewards_received", rewards);
    }
    if let Some(b) = batch::finish_batch(deps.storage, cfg, now, atom)? {
        res = res
            .add_attribute("batch_created", b.id.to_string())
            .add_attribute("batch_amount", b.amount)
            .add_attribute("batch_capacity", b.capacity)
            .add_attribute("batch_completion", b.completion.seconds().to_string());
    }
    if PENDING_DIRECT.exists(deps.storage) {
        return Err(ContractError::Invariant(
            "direct entry not deposited".into(),
        ));
    }

    // 1. reserved ≤ the ATOM balance, after advancing the cursor.
    let m = maturity::advance(deps.storage, now)?;
    if m.reserved > atom {
        return Err(ContractError::Invariant(format!(
            "reserved {} above the ATOM balance {atom}",
            m.reserved
        )));
    }
    // 2. booked ≤ capacity for each batch in flight (only these can take bookings; a matured
    // batch's booked is frozen).
    let start = m.cursor.map(cw_storage_plus::Bound::exclusive);
    for key in BY_COMPLETION.keys(deps.storage, start, None, Order::Ascending) {
        let (_, id) = key?;
        let b = BATCHES.load(deps.storage, id)?;
        if b.booked > b.capacity {
            return Err(ContractError::Invariant(format!(
                "batch {id} over capacity"
            )));
        }
    }
    // 3. INVENTORY.shares ≤ the gate's share balance.
    let inv = INVENTORY.load(deps.storage)?;
    let shares = balance(&deps.querier, &env.contract.address, &cfg.share_denom)?;
    if inv.shares > shares {
        return Err(ContractError::Invariant(format!(
            "inventory {} above the share balance {shares}",
            inv.shares
        )));
    }
    // 4. A swap entry received at least the cost basis of the shares it gave.
    if let Some(SwapCheck { received, cost_out }) = swap {
        if received < cost_out {
            return Err(ContractError::Invariant("swap below cost".into()));
        }
    }

    OP_GUARD.remove(deps.storage);
    Ok(res)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    let result = msg.result.into_result();
    match msg.id {
        REPLY_MINT => {
            let r = result.map_err(StdError::generic_err)?;
            trade::on_minted(deps, &cfg, chain::execute_response_data(&r)?)
        }
        REPLY_COMPOUND => batch::on_compound(deps, env, &cfg, result),
        REPLY_WITHDRAW => {
            result.map_err(StdError::generic_err)?;
            batch::on_withdrawn(deps, env, &cfg)
        }
        REPLY_BURN => {
            let r = result.map_err(StdError::generic_err)?;
            batch::on_burned(env, chain::execute_response_data(&r)?)
        }
        id if id >= REPLY_UNDELEGATE => {
            batch::on_undelegated(deps, (id - REPLY_UNDELEGATE) as usize, result)
        }
        id => Err(StdError::generic_err(format!("unknown reply id {id}")).into()),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    crate::query::query(deps, env, msg)
}

/// Whether hATOM is paused (for the quotes).
pub fn hatom_paused(deps: Deps, cfg: &Config) -> StdResult<bool> {
    Ok(hatom_config(&deps.querier, cfg)?.paused)
}
