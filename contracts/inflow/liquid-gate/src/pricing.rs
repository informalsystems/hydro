//! Pricing inputs and the price guard (SPEC §5): `V` and `P` read in the same transaction from
//! the vault and hATOM, no oracle and no cached price.

use cosmwasm_std::{Addr, Env, QuerierWrapper, StdResult, Storage, Uint128, Uint256};
use interface::inflow_control_center::PoolInfoResponse as CcPoolInfoResponse;
use interface::inflow_vault::{QueryMsg as VaultQueryMsg, WithdrawalQueueInfoResponse};

use crate::error::ContractError;
use crate::hatom::{BurnSimulation, HatomConfig, HatomQueryMsg, MintSimulation, RateResponse};
use crate::settings::BPS;
use crate::state::{Config, PriceRef, PRICE_REF};

/// The share units whose value is `P` (one share, 6 decimals).
pub const PRICE_UNIT: u128 = 1_000_000;
/// The most value queries one inverse may take.
const MAX_SEARCH_STEPS: u32 = 200;

pub fn balance(q: &QuerierWrapper, addr: &Addr, denom: &str) -> StdResult<Uint128> {
    Ok(q.query_balance(addr, denom)?.amount)
}

pub fn hatom_config(q: &QuerierWrapper, cfg: &Config) -> StdResult<HatomConfig> {
    q.query_wasm_smart(&cfg.hatom, &HatomQueryMsg::Config {})
}

pub fn rate(q: &QuerierWrapper, cfg: &Config) -> StdResult<RateResponse> {
    q.query_wasm_smart(&cfg.hatom, &HatomQueryMsg::Rate {})
}

pub fn simulate_burn(
    q: &QuerierWrapper,
    cfg: &Config,
    amount: Uint128,
    max_legs: Option<u32>,
) -> StdResult<BurnSimulation> {
    q.query_wasm_smart(
        &cfg.hatom,
        &HatomQueryMsg::SimulateBurn {
            amount,
            max_legs,
            split: None,
        },
    )
}

pub fn simulate_mint(
    q: &QuerierWrapper,
    cfg: &Config,
    amount: Uint128,
) -> StdResult<MintSimulation> {
    q.query_wasm_smart(
        &cfg.hatom,
        &HatomQueryMsg::SimulateMintNative {
            amount,
            validator: None,
            max_legs: None,
        },
    )
}

/// The vault's `SharesEquivalentValue`: hATOM, floored, exactly what a withdrawal pays.
pub fn shares_value_hatom(q: &QuerierWrapper, cfg: &Config, shares: Uint128) -> StdResult<Uint128> {
    if shares.is_zero() {
        return Ok(Uint128::zero());
    }
    q.query_wasm_smart(&cfg.vault, &VaultQueryMsg::SharesEquivalentValue { shares })
}

pub fn dry_run_deposit(q: &QuerierWrapper, cfg: &Config, amount: Uint128) -> StdResult<Uint128> {
    q.query_wasm_smart(&cfg.vault, &VaultQueryMsg::DryRunDeposit { amount })
}

pub fn withdrawal_queue(
    q: &QuerierWrapper,
    cfg: &Config,
) -> StdResult<WithdrawalQueueInfoResponse> {
    q.query_wasm_smart(&cfg.vault, &VaultQueryMsg::WithdrawalQueueInfo {})
}

/// `V(s)`: `⌊SharesEquivalentValue(s) × holders_value / supply⌋` ATOM.
pub fn value(
    q: &QuerierWrapper,
    cfg: &Config,
    rate: &RateResponse,
    shares: Uint128,
) -> StdResult<Uint128> {
    Ok(rate.value_of(shares_value_hatom(q, cfg, shares)?))
}

/// `P = V(10^6)`.
pub fn price(q: &QuerierWrapper, cfg: &Config, rate: &RateResponse) -> StdResult<Uint128> {
    value(q, cfg, rate, Uint128::new(PRICE_UNIT))
}

/// The vault's issued shares (its `ControlCenterPoolInfo`): `SharesEquivalentValue` refuses more.
pub fn shares_issued(q: &QuerierWrapper, cfg: &Config) -> StdResult<Uint128> {
    let r: CcPoolInfoResponse =
        q.query_wasm_smart(&cfg.vault, &VaultQueryMsg::ControlCenterPoolInfo {})?;
    Ok(r.total_shares_issued)
}

/// The largest `s` with `V(s) ≤ target`: an estimate from the value of all issued shares (the
/// most precise reference the vault accepts), then a galloping and binary search on the real `V`
/// (a few queries: the estimate is off only by the floors). Never above the issued shares.
pub fn max_shares_for(
    q: &QuerierWrapper,
    cfg: &Config,
    rate: &RateResponse,
    target: Uint128,
) -> Result<Uint128, ContractError> {
    let issued = shares_issued(q, cfg)?;
    let reference = value(q, cfg, rate, issued)?;
    if reference.is_zero() || target.is_zero() {
        return Ok(Uint128::zero());
    }
    let est =
        Uint128::try_from(Uint256::from(target) * Uint256::from(issued) / Uint256::from(reference))
            .unwrap_or(Uint128::MAX)
            .min(issued);

    let mut steps = 0u32;
    let mut v = |s: Uint128| -> Result<Uint128, ContractError> {
        steps += 1;
        if steps > MAX_SEARCH_STEPS {
            return Err(ContractError::Std(cosmwasm_std::StdError::generic_err(
                "share search did not converge",
            )));
        }
        if s > issued {
            // Above every share issued: more than any target this search can meet.
            return Ok(Uint128::MAX);
        }
        Ok(value(q, cfg, rate, s)?)
    };

    // Bracket: lo with V(lo) ≤ target (or lo = 0), hi with V(hi) > target.
    let (mut lo, mut hi);
    if v(est)? <= target {
        lo = est;
        let mut step = Uint128::one();
        loop {
            let probe = lo.saturating_add(step);
            if probe == lo {
                return Ok(lo);
            }
            if v(probe)? <= target {
                lo = probe;
                step = step.saturating_mul(Uint128::new(2));
            } else {
                hi = probe;
                break;
            }
        }
    } else {
        hi = est;
        let mut step = Uint128::one();
        loop {
            let probe = hi.saturating_sub(step);
            if probe.is_zero() {
                lo = Uint128::zero();
                break;
            }
            if v(probe)? > target {
                hi = probe;
                step = step.saturating_mul(Uint128::new(2));
            } else {
                lo = probe;
                break;
            }
        }
    }
    while hi - lo > Uint128::one() {
        let mid = lo + (hi - lo) / Uint128::new(2);
        if v(mid)? <= target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok(lo)
}

/// The guard (SPEC §5): refused (`PriceJump`) when `|P - price_ref| / price_ref` is above
/// `price_jump_bps + price_drift_bps_per_day × days since price_ref_time` (pro rata, in seconds:
/// coordinator's answer, 2026-11-10). No reference yet: no check.
pub fn check_price_guard(
    storage: &dyn Storage,
    env: &Env,
    cfg: &Config,
    p: Uint128,
) -> Result<(), ContractError> {
    let Some(PriceRef { price, time }) = PRICE_REF.may_load(storage)? else {
        return Ok(());
    };
    let elapsed = env.block.time.seconds().saturating_sub(time.seconds()) as u128;
    let s = &cfg.settings;
    let allowed_bps =
        s.price_jump_bps as u128 + s.price_drift_bps_per_day as u128 * elapsed / 86_400;
    let diff = p.abs_diff(price);
    // diff / price > allowed / BPS
    if Uint256::from(diff) * Uint256::from(BPS as u128)
        > Uint256::from(allowed_bps) * Uint256::from(price)
    {
        return Err(ContractError::PriceJump);
    }
    Ok(())
}

/// The inputs of a priced trade: refused while hATOM is paused or by the guard.
pub fn priced_inputs(
    q: &QuerierWrapper,
    storage: &dyn Storage,
    env: &Env,
    cfg: &Config,
) -> Result<(RateResponse, Uint128), ContractError> {
    if hatom_config(q, cfg)?.paused {
        return Err(ContractError::Paused);
    }
    let rate = rate(q, cfg)?;
    let p = price(q, cfg, &rate)?;
    check_price_guard(storage, env, cfg, p)?;
    Ok((rate, p))
}
