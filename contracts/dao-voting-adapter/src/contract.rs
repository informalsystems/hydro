#![allow(unused_imports)]
use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdError,
    StdResult,
};
use cw2::{get_contract_version, set_contract_version};
use dao_interface::voting::{
    InfoResponse, TotalPowerAtHeightResponse, VotingPowerAtHeightResponse,
};

use crate::{
    error::ContractError,
    msg::InstantiateMsg,
    query::{ConfigResponse, QueryMsg},
    state::{Config, CONFIG},
};

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

    // Voting module gets instantiated by the main DAO contract
    let dao_contract = info.sender.clone();
    let hydro_contract = deps.api.addr_validate(&msg.hydro_contract)?;

    CONFIG.save(
        deps.storage,
        &Config {
            dao_contract: dao_contract.clone(),
            hydro_contract: hydro_contract.clone(),
        },
    )?;

    Ok(Response::new()
        .add_attribute("action", "initialisation")
        .add_attribute("sender", dao_contract)
        .add_attribute("hydro_contract", hydro_contract))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::Dao {} => to_json_binary(&query_config(deps)?.config.dao_contract),
        QueryMsg::Info {} => to_json_binary(&query_contract_info(deps)?),
        QueryMsg::TotalPowerAtHeight { height } => {
            to_json_binary(&query_total_power_at_height(deps, height)?)
        }
        QueryMsg::VotingPowerAtHeight { address, height } => {
            to_json_binary(&query_voting_power_at_height(deps, address, height)?)
        }
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

fn query_contract_info(deps: Deps) -> StdResult<InfoResponse> {
    Ok(InfoResponse {
        info: get_contract_version(deps.storage)?,
    })
}

fn query_total_power_at_height(
    deps: Deps,
    height: Option<u64>,
) -> StdResult<TotalPowerAtHeightResponse> {
    let hydro_contract = CONFIG.load(deps.storage)?.hydro_contract;
    let total_power_resp: TotalPowerAtHeightResponse = deps
        .querier
        .query_wasm_smart(hydro_contract, &QueryMsg::TotalPowerAtHeight { height })
        .map_err(|err| {
            StdError::generic_err(format!(
                "Failed to query total voting power from Hydro contract. Error: {}",
                err
            ))
        })?;

    Ok(TotalPowerAtHeightResponse {
        power: total_power_resp.power,
        height: total_power_resp.height,
    })
}

fn query_voting_power_at_height(
    deps: Deps,
    address: String,
    height: Option<u64>,
) -> StdResult<VotingPowerAtHeightResponse> {
    let hydro_contract = CONFIG.load(deps.storage)?.hydro_contract;
    let voting_power_resp: VotingPowerAtHeightResponse = deps
        .querier
        .query_wasm_smart(
            hydro_contract,
            &QueryMsg::VotingPowerAtHeight { address, height },
        )
        .map_err(|err| {
            StdError::generic_err(format!(
                "Failed to query user voting power from Hydro contract. Error: {}",
                err
            ))
        })?;

    Ok(VotingPowerAtHeightResponse {
        power: voting_power_resp.power,
        height: voting_power_resp.height,
    })
}
