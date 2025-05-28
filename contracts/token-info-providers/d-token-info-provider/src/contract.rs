use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Response,
    StdError, StdResult, Storage, SubMsg, WasmMsg,
};
use cw2::set_contract_version;
use interface::token_info_provider::DenomInfoResponse;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, HydroExecuteMsg, InstantiateMsg};
use crate::query::{
    ConfigResponse, DropQueryMsg, HydroCurrentRoundResponse, HydroQueryMsg, QueryMsg,
};
use crate::state::{Config, CONFIG, TOKEN_RATIO};

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let drop_staking_core_contract = deps.api.addr_validate(&msg.drop_staking_core_contract)?;
    let config = Config {
        hydro_contract_address: info.sender.clone(),
        d_token_denom: msg.d_token_denom.clone(),
        token_group_id: msg.token_group_id.clone(),
        drop_staking_core_contract: drop_staking_core_contract.clone(),
    };

    CONFIG.save(deps.storage, &config)?;
    TOKEN_RATIO.save(deps.storage, 0, &Decimal::zero())?;

    Ok(Response::new()
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender)
        .add_attribute("d_token_denom", msg.d_token_denom)
        .add_attribute("token_group_id", msg.token_group_id)
        .add_attribute("drop_staking_core_contract", drop_staking_core_contract))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateTokenRatio {} => execute_update_token_ratio(deps, info),
    }
}

fn execute_update_token_ratio(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let current_round = query_current_round_id(&deps.as_ref(), &config.hydro_contract_address)
        .map_err(|e| {
            ContractError::Std(StdError::generic_err(format!(
                "failed to query current round id: {}",
                e
            )))
        })?;

    let new_ratio: Decimal = deps.querier.query_wasm_smart(
        config.drop_staking_core_contract,
        &DropQueryMsg::ExchangeRate {},
    )?;

    // Make sure that the correct old token ratio value is used. If the ratio is being updated for the first time
    // in the current round, then the old token ratio should be copied from the latest round for which the ratio
    // is known, instead of using 0. Using wrong old value would report inaccurate token ratio updates to
    // the main Hydro contract.
    initialize_token_ratio(deps.storage, current_round)?;

    let mut submsgs = vec![];
    let old_ratio = TOKEN_RATIO.load(deps.storage, current_round)?;

    if old_ratio != new_ratio {
        TOKEN_RATIO.save(deps.storage, current_round, &new_ratio)?;

        let update_token_ratio_msg = HydroExecuteMsg::UpdateTokenGroupRatio {
            token_group_id: config.token_group_id.clone(),
            old_ratio,
            new_ratio,
        };

        let wasm_execute_msg = WasmMsg::Execute {
            contract_addr: config.hydro_contract_address.to_string(),
            msg: to_json_binary(&update_token_ratio_msg)?,
            funds: vec![],
        };

        submsgs.push(SubMsg::reply_never(wasm_execute_msg));
    }

    Ok(Response::new()
        .add_submessages(submsgs)
        .add_attribute("action", "update_token_ratio")
        .add_attribute("sender", info.sender)
        .add_attribute("round_id", current_round.to_string())
        .add_attribute("old_ratio", old_ratio.to_string())
        .add_attribute("new_ratio", new_ratio.to_string()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::DenomInfo { round_id } => to_json_binary(&query_denom_info(deps, round_id)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

fn query_denom_info(deps: Deps, round_id: u64) -> StdResult<DenomInfoResponse> {
    let config = CONFIG.load(deps.storage)?;

    Ok(DenomInfoResponse {
        denom: config.d_token_denom,
        token_group_id: config.token_group_id,
        ratio: find_latest_known_token_ratio(deps.storage, round_id)?,
    })
}

pub fn query_current_round_id(deps: &Deps, hydro_contract: &Addr) -> Result<u64, ContractError> {
    let current_round_resp: HydroCurrentRoundResponse = deps
        .querier
        .query_wasm_smart(hydro_contract, &HydroQueryMsg::CurrentRound {})?;

    Ok(current_round_resp.round_id)
}

// Finds the latest known token ratio by going backwards from the given start round until it finds a round
// that has the token ratio initialized. This is useful for our token information provider API in case when
// a new round starts, so that we don't have to initialize the new round data immediately, but without stoping
// our users from locking their tokens in the new round. Once we run the UpdateTokenRatio, it will first copy
// the same old value from the last known round, and then update it with the new value queried from the Drop,
// so there is no risk of using different ratios for the same round in different contexts.
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

// Initializes the token ratio for all rounds up to the current round. Starts from the current round
// and goes backwards until it finds the round that has the token ratio initialized.
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
