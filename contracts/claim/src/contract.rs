use std::collections::HashMap;

use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Order,
    Response, StdResult, Timestamp, Uint128,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;

use crate::error::ContractError;
use crate::msg::{ClaimEntry, ExecuteMsg, InstantiateMsg};
use crate::query::{
    ClaimHistoryResponse, ConfigResponse, DistributionResponse, PendingClaimInfo,
    PendingClaimsResponse, QueryMsg,
};
use crate::state::{
    ClaimRecord, Config, Distribution, CLAIMS, CLAIM_HISTORY, CONFIG, DISTRIBUTIONS,
    NEXT_DISTRIBUTION_ID,
};

pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let config = Config {
        admin: deps.api.addr_validate(&msg.admin)?,
        treasury: deps.api.addr_validate(&msg.treasury)?,
    };

    CONFIG.save(deps.storage, &config)?;
    NEXT_DISTRIBUTION_ID.save(deps.storage, &0)?;

    Ok(Response::new().add_attribute("action", "instantiate"))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::CreateDistribution { claims, expiry } => {
            execute_create_distribution(deps, env, info, claims, expiry)
        }
        ExecuteMsg::Claim {} => execute_claim(deps, env, info),
        ExecuteMsg::SweepExpired { distribution_id } => {
            execute_sweep_expired(deps, env, info, distribution_id)
        }
        ExecuteMsg::UpdateConfig { admin, treasury } => {
            execute_update_config(deps, info, admin, treasury)
        }
    }
}

fn execute_create_distribution(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    claims: Vec<ClaimEntry>,
    expiry: Timestamp,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.admin {
        return Err(ContractError::Unauthorized);
    }

    if info.funds.is_empty() {
        return Err(ContractError::NoFundsSent);
    }

    if claims.is_empty() {
        return Err(ContractError::EmptyClaims);
    }

    if expiry <= env.block.time {
        return Err(ContractError::ExpiryInPast);
    }

    // Accumulate weights per address to handle duplicates correctly
    let mut weight_by_addr: HashMap<String, Uint128> = HashMap::new();
    for claim in &claims {
        let addr = deps.api.addr_validate(&claim.address)?;
        let entry = weight_by_addr.entry(addr.to_string()).or_default();
        *entry += claim.weight;
    }

    let total_weight: Uint128 = weight_by_addr.values().copied().sum();
    if total_weight.is_zero() {
        return Err(ContractError::ZeroTotalWeight);
    }

    let id = NEXT_DISTRIBUTION_ID.load(deps.storage)?;
    NEXT_DISTRIBUTION_ID.save(deps.storage, &(id + 1))?;

    let distribution = Distribution {
        id,
        original_funds: info.funds.clone(),
        remaining_funds: info.funds,
        total_weight,
        expiry,
    };

    DISTRIBUTIONS.save(deps.storage, id, &distribution)?;

    for (addr_str, weight) in &weight_by_addr {
        let addr = deps.api.addr_validate(addr_str)?;
        CLAIMS.save(deps.storage, (addr, id), weight)?;
    }

    Ok(Response::new()
        .add_attribute("action", "create_distribution")
        .add_attribute("distribution_id", id.to_string())
        .add_attribute("total_weight", total_weight.to_string()))
}

fn execute_claim(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    let user = info.sender.clone();

    // Collect all claim entries for this user
    let user_claims: Vec<(u64, Uint128)> = CLAIMS
        .prefix(user.clone())
        .range(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    if user_claims.is_empty() {
        return Err(ContractError::NoPendingClaims);
    }

    let mut total_to_send: Vec<Coin> = vec![];

    for (dist_id, weight) in &user_claims {
        let mut dist = DISTRIBUTIONS.load(deps.storage, *dist_id)?;

        // Clean up and skip expired distributions
        if dist.expiry <= env.block.time {
            CLAIMS.remove(deps.storage, (user.clone(), *dist_id));
            continue;
        }

        // Compute share per denom
        let mut dist_funds_claimed: Vec<Coin> = vec![];
        for orig_coin in &dist.original_funds {
            let share = orig_coin.amount.multiply_ratio(*weight, dist.total_weight);

            // Cap at remaining
            let remaining = dist
                .remaining_funds
                .iter()
                .find(|c| c.denom == orig_coin.denom)
                .map(|c| c.amount)
                .unwrap_or_default();

            let actual = share.min(remaining);
            if actual.is_zero() {
                continue;
            }

            // Subtract from remaining
            if let Some(rem_coin) = dist
                .remaining_funds
                .iter_mut()
                .find(|c| c.denom == orig_coin.denom)
            {
                rem_coin.amount -= actual;
            }

            // Accumulate for sending
            add_coin(&mut total_to_send, &orig_coin.denom, actual);
            add_coin(&mut dist_funds_claimed, &orig_coin.denom, actual);
        }

        dist.remaining_funds.retain(|c| !c.amount.is_zero());
        DISTRIBUTIONS.save(deps.storage, *dist_id, &dist)?;
        CLAIMS.remove(deps.storage, (user.clone(), *dist_id));

        // Record claim history
        if !dist_funds_claimed.is_empty() {
            CLAIM_HISTORY.save(
                deps.storage,
                (user.clone(), *dist_id),
                &ClaimRecord {
                    distribution_id: *dist_id,
                    funds_claimed: dist_funds_claimed,
                    claimed_at: env.block.time,
                },
            )?;
        }
    }

    if total_to_send.is_empty() {
        return Err(ContractError::NoPendingClaims);
    }

    let send_msg = BankMsg::Send {
        to_address: user.to_string(),
        amount: total_to_send,
    };

    Ok(Response::new()
        .add_message(send_msg)
        .add_attribute("action", "claim")
        .add_attribute("user", user.to_string()))
}

fn execute_sweep_expired(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    distribution_id: u64,
) -> Result<Response, ContractError> {
    let mut dist = DISTRIBUTIONS
        .may_load(deps.storage, distribution_id)?
        .ok_or(ContractError::DistributionNotFound {
            id: distribution_id,
        })?;

    if dist.expiry > env.block.time {
        return Err(ContractError::DistributionNotExpired {
            id: distribution_id,
        });
    }

    if dist.remaining_funds.is_empty() {
        return Err(ContractError::DistributionAlreadySwept {
            id: distribution_id,
        });
    }

    let config = CONFIG.load(deps.storage)?;
    let to_send = dist.remaining_funds.clone();
    dist.remaining_funds = vec![];
    DISTRIBUTIONS.save(deps.storage, distribution_id, &dist)?;

    let send_msg = BankMsg::Send {
        to_address: config.treasury.to_string(),
        amount: to_send,
    };

    Ok(Response::new()
        .add_message(send_msg)
        .add_attribute("action", "sweep_expired")
        .add_attribute("distribution_id", distribution_id.to_string()))
}

fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    admin: Option<String>,
    treasury: Option<String>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    if info.sender != config.admin {
        return Err(ContractError::Unauthorized);
    }

    if let Some(admin) = admin {
        config.admin = deps.api.addr_validate(&admin)?;
    }
    if let Some(treasury) = treasury {
        config.treasury = deps.api.addr_validate(&treasury)?;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

/// Helper to accumulate coins into a vec, merging same denoms.
fn add_coin(coins: &mut Vec<Coin>, denom: &str, amount: Uint128) {
    if let Some(coin) = coins.iter_mut().find(|c| c.denom == denom) {
        coin.amount += amount;
    } else {
        coins.push(Coin {
            denom: denom.to_string(),
            amount,
        });
    }
}

fn compute_estimated_funds(dist: &Distribution, weight: Uint128) -> Vec<Coin> {
    let mut funds = vec![];
    for orig_coin in &dist.original_funds {
        let share = orig_coin.amount.multiply_ratio(weight, dist.total_weight);
        let remaining = dist
            .remaining_funds
            .iter()
            .find(|c| c.denom == orig_coin.denom)
            .map(|c| c.amount)
            .unwrap_or_default();
        let actual = share.min(remaining);
        if !actual.is_zero() {
            funds.push(Coin {
                denom: orig_coin.denom.clone(),
                amount: actual,
            });
        }
    }
    funds
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::Distribution { id } => to_json_binary(&query_distribution(deps, id)?),
        QueryMsg::PendingClaims { user } => to_json_binary(&query_pending_claims(deps, env, user)?),
        QueryMsg::ClaimHistory {
            user,
            start_after,
            limit,
        } => to_json_binary(&query_claim_history(deps, user, start_after, limit)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

fn query_distribution(deps: Deps, id: u64) -> StdResult<DistributionResponse> {
    let distribution = DISTRIBUTIONS.load(deps.storage, id)?;
    Ok(DistributionResponse { distribution })
}

fn query_pending_claims(deps: Deps, env: Env, user: String) -> StdResult<PendingClaimsResponse> {
    let user_addr = deps.api.addr_validate(&user)?;

    let all_claims: Vec<(u64, Uint128)> = CLAIMS
        .prefix(user_addr)
        .range(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    let mut claims = vec![];
    for (dist_id, weight) in all_claims {
        let dist = DISTRIBUTIONS.load(deps.storage, dist_id)?;
        if dist.expiry <= env.block.time {
            continue;
        }
        let estimated_funds = compute_estimated_funds(&dist, weight);
        claims.push(PendingClaimInfo {
            distribution_id: dist_id,
            weight,
            estimated_funds,
        });
    }

    Ok(PendingClaimsResponse { claims })
}

const DEFAULT_LIMIT: u32 = 30;
const MAX_LIMIT: u32 = 100;

fn query_claim_history(
    deps: Deps,
    user: String,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<ClaimHistoryResponse> {
    let user_addr = deps.api.addr_validate(&user)?;
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let min_bound = start_after.map(Bound::exclusive);

    let claims: Vec<ClaimRecord> = CLAIM_HISTORY
        .prefix(user_addr)
        .range(deps.storage, min_bound, None, Order::Ascending)
        .take(limit)
        .collect::<StdResult<Vec<_>>>()?
        .into_iter()
        .map(|(_, record)| record)
        .collect();

    Ok(ClaimHistoryResponse { claims })
}
