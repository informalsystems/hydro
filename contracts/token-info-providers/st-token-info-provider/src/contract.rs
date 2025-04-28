use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Response,
    StdResult,
};
use cw2::set_contract_version;
use interface::token_info_provider::DenomInfoResponse;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg};
use crate::query::{ConfigResponse, QueryMsg};
use crate::state::{Config, CONFIG};

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
    let config = Config {
        hydro_contract_address: info.sender.clone(),
        st_token_denom: msg.st_token_denom.clone(),
        token_group_id: msg.token_group_id.clone(),
    };

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender)
        .add_attribute("st_token_denom", msg.st_token_denom)
        .add_attribute("token_group_id", msg.token_group_id))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    Ok(Response::default())
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

fn query_denom_info(deps: Deps, _round_id: u64) -> StdResult<DenomInfoResponse> {
    let config = CONFIG.load(deps.storage)?;

    Ok(DenomInfoResponse {
        denom: config.st_token_denom,
        token_group_id: config.token_group_id,
        ratio: Decimal::one(),
    })
}
