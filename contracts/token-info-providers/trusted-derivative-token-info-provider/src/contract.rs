use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Order,
    Response, StdError, StdResult, Storage, SubMsg, WasmMsg,
};
use cw2::set_contract_version;
use interface::hydro::{
    CurrentRoundResponse as HydroCurrentRoundResponse, ExecuteMsg as HydroExecuteMsg,
    QueryMsg as HydroQueryMsg, TokenGroupRatioChange,
};
use interface::token_info_provider::DenomInfoResponse;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg};
use crate::query::{ConfigResponse, QueryMsg, WhitelistResponse};
use crate::state::{Config, CONFIG, TOKEN_RATIO, WHITELIST};

pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    if msg.initial_whitelist.is_empty() {
        return Err(ContractError::WhitelistEmpty {});
    }

    let hydro_contract_address = msg
        .hydro_contract_address
        .map(|addr| deps.api.addr_validate(&addr))
        .transpose()?
        .unwrap_or(info.sender.clone());

    let config = Config {
        hydro_contract_address: hydro_contract_address.clone(),
        derivative_token_denom: msg.derivative_token_denom.clone(),
        token_group_id: msg.token_group_id.clone(),
        max_ratio_diff: msg.max_ratio_diff,
        first_submission: true,
    };

    CONFIG.save(deps.storage, &config)?;
    TOKEN_RATIO.save(deps.storage, 0, &Decimal::zero())?;

    // All whitlisted addresses must be valid, otherwise the contract instantiation will fail.
    for addr_str in &msg.initial_whitelist {
        let addr = deps.api.addr_validate(addr_str)?;
        WHITELIST.save(deps.storage, addr, &())?;
    }

    Ok(Response::new()
        .add_attribute("action", "instantiation")
        .add_attribute("sender", info.sender)
        .add_attribute("hydro_contract_address", hydro_contract_address)
        .add_attribute("derivative_token_denom", msg.derivative_token_denom)
        .add_attribute("token_group_id", msg.token_group_id))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::SubmitTokenRatio { ratio } => execute_submit_token_ratio(deps, info, ratio),
        ExecuteMsg::AddToWhitelist { address } => execute_add_to_whitelist(deps, info, address),
        ExecuteMsg::RemoveFromWhitelist { address } => {
            execute_remove_from_whitelist(deps, info, address)
        }
    }
}

fn execute_submit_token_ratio(
    deps: DepsMut,
    info: MessageInfo,
    new_ratio: Decimal,
) -> Result<Response, ContractError> {
    ensure_whitelisted(deps.storage, &info.sender)?;

    let mut config = CONFIG.load(deps.storage)?;

    let current_round = query_current_round_id(&deps.as_ref(), &config.hydro_contract_address)
        .map_err(|e| {
            ContractError::Std(StdError::generic_err(format!(
                "failed to query current round id: {e}"
            )))
        })?;

    initialize_token_ratio(deps.storage, current_round)?;

    let old_ratio = TOKEN_RATIO.load(deps.storage, current_round)?;
    let mut submsgs = vec![];

    if old_ratio != new_ratio {
        let diff = if new_ratio > old_ratio {
            new_ratio - old_ratio
        } else {
            old_ratio - new_ratio
        };

        if config.first_submission {
            config.first_submission = false;
            CONFIG.save(deps.storage, &config)?;
        } else if diff > config.max_ratio_diff {
            return Err(ContractError::RatioDiffExceedsThreshold {
                old_ratio,
                new_ratio,
                max_diff: config.max_ratio_diff,
                actual_diff: diff,
            });
        }

        TOKEN_RATIO.save(deps.storage, current_round, &new_ratio)?;

        let update_msg = HydroExecuteMsg::UpdateTokenGroupsRatios {
            changes: vec![TokenGroupRatioChange {
                token_group_id: config.token_group_id.clone(),
                old_ratio,
                new_ratio,
            }],
        };

        submsgs.push(SubMsg::reply_never(WasmMsg::Execute {
            contract_addr: config.hydro_contract_address.to_string(),
            msg: to_json_binary(&update_msg)?,
            funds: vec![],
        }));
    }

    Ok(Response::new()
        .add_submessages(submsgs)
        .add_attribute("action", "submit_token_ratio")
        .add_attribute("sender", info.sender)
        .add_attribute("round_id", current_round.to_string())
        .add_attribute("old_ratio", old_ratio.to_string())
        .add_attribute("new_ratio", new_ratio.to_string()))
}

fn execute_add_to_whitelist(
    deps: DepsMut,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    ensure_whitelisted(deps.storage, &info.sender)?;

    let addr = deps.api.addr_validate(&address)?;
    if WHITELIST.has(deps.storage, addr.clone()) {
        return Err(ContractError::AddressAlreadyInWhitelist {
            address: addr.to_string(),
        });
    }

    WHITELIST.save(deps.storage, addr, &())?;

    Ok(Response::new()
        .add_attribute("action", "add_to_whitelist")
        .add_attribute("sender", info.sender)
        .add_attribute("address", address))
}

fn execute_remove_from_whitelist(
    deps: DepsMut,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    ensure_whitelisted(deps.storage, &info.sender)?;

    let addr = deps.api.addr_validate(&address)?;

    let is_address_whitelisted = WHITELIST.has(deps.storage, addr.clone());
    if !is_address_whitelisted {
        return Err(ContractError::AddressNotInWhitelist {
            address: addr.to_string(),
        });
    }

    let count = WHITELIST
        .keys(deps.storage, None, None, Order::Ascending)
        .count();

    if count <= 1 {
        return Err(ContractError::WhitelistEmpty {});
    }

    WHITELIST.remove(deps.storage, addr);

    Ok(Response::new()
        .add_attribute("action", "remove_from_whitelist")
        .add_attribute("sender", info.sender)
        .add_attribute("address", address))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::Whitelist {} => to_json_binary(&query_whitelist(deps)?),
        QueryMsg::DenomInfo { round_id } => to_json_binary(&query_denom_info(deps, round_id)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

fn query_whitelist(deps: Deps) -> StdResult<WhitelistResponse> {
    let addresses = WHITELIST
        .keys(deps.storage, None, None, Order::Ascending)
        .map(|r| r.map(|addr| addr.to_string()))
        .collect::<StdResult<Vec<_>>>()?;
    Ok(WhitelistResponse { addresses })
}

fn query_denom_info(deps: Deps, round_id: u64) -> StdResult<DenomInfoResponse> {
    let config = CONFIG.load(deps.storage)?;
    Ok(DenomInfoResponse {
        denom: config.derivative_token_denom,
        token_group_id: config.token_group_id,
        ratio: find_latest_known_token_ratio(deps.storage, round_id)?,
    })
}

pub fn query_current_round_id(deps: &Deps, hydro_contract: &Addr) -> Result<u64, ContractError> {
    let resp: HydroCurrentRoundResponse = deps
        .querier
        .query_wasm_smart(hydro_contract, &HydroQueryMsg::CurrentRound {})?;
    Ok(resp.round_id)
}

pub fn find_latest_known_token_ratio(
    storage: &dyn Storage,
    start_round: u64,
) -> StdResult<Decimal> {
    let mut round = start_round;
    while !is_token_ratio_initialized(storage, round) {
        if round == 0 {
            return Err(StdError::generic_err(
                "first round must be initialized during contract instantiation",
            ));
        }
        round -= 1;
    }
    TOKEN_RATIO.load(storage, round)
}

pub fn initialize_token_ratio(storage: &mut dyn Storage, current_round: u64) -> StdResult<()> {
    let mut round = current_round;
    while !is_token_ratio_initialized(storage, round) {
        if round == 0 {
            return Err(StdError::generic_err(
                "first round must be initialized during contract instantiation",
            ));
        }
        round -= 1;
    }

    let last_known_ratio = TOKEN_RATIO.load(storage, round)?;

    while round < current_round {
        round += 1;
        TOKEN_RATIO.save(storage, round, &last_known_ratio)?;
    }

    Ok(())
}

pub fn is_token_ratio_initialized(storage: &dyn Storage, round_id: u64) -> bool {
    TOKEN_RATIO.has(storage, round_id)
}

fn ensure_whitelisted(storage: &dyn Storage, sender: &Addr) -> Result<(), ContractError> {
    if !WHITELIST.has(storage, sender.clone()) {
        return Err(ContractError::Unauthorized {});
    }
    Ok(())
}
