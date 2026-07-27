use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdResult,
};
use cw2::set_contract_version;
use interface::token_info_provider::ValidatorsInfoResponse;

use crate::error::ContractError;
use crate::msg::{ExecuteContext, ExecuteMsg, InstantiateMsg};
use crate::query::{AdminsResponse, ConfigResponse, QueryMsg};
use crate::state::{Config, ADMINS, CONFIG, VALIDATORS_INFO, VALIDATORS_STORE_INITIALIZED};
use crate::utils::{
    get_nearest_store_initialized_round, query_current_round_id, run_on_each_transaction,
};
use crate::validators::update_validators_ratios;

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

    let hydro_contract_address = match msg.hydro_contract_address {
        Some(addr) => deps.api.addr_validate(&addr)?,
        None => info.sender.clone(),
    };

    let config = Config {
        hydro_contract_address: hydro_contract_address.clone(),
        max_validator_shares_participating: msg.max_validator_shares_participating,
    };

    CONFIG.save(deps.storage, &config)?;

    // Mark round 0 as initialized so the lazy copy logic has a valid starting point.
    VALIDATORS_STORE_INITIALIZED.save(deps.storage, 0, &true)?;

    for admin in msg.admins {
        let admin_addr = deps.api.addr_validate(&admin)?;
        ADMINS.save(deps.storage, admin_addr, &true)?;
    }

    Ok(Response::new()
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender)
        .add_attribute("hydro_contract_address", hydro_contract_address)
        .add_attribute(
            "max_validator_shares_participating",
            msg.max_validator_shares_participating.to_string(),
        ))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    mut deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let current_round_id = query_current_round_id(&deps.as_ref(), &config.hydro_contract_address)?;

    run_on_each_transaction(&mut deps, current_round_id)?;

    let context = ExecuteContext {
        current_round_id,
        config,
    };

    match msg {
        ExecuteMsg::UpdateValidatorsRatios { validators } => {
            update_validators_ratios(deps, validators, context)
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::Admins {} => to_json_binary(&query_admins(deps)?),
        QueryMsg::ValidatorsInfo { round_id } => {
            to_json_binary(&query_validators_info(deps, round_id)?)
        }
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

fn query_admins(deps: Deps) -> StdResult<AdminsResponse> {
    Ok(AdminsResponse {
        admins: ADMINS
            .range(deps.storage, None, None, Order::Ascending)
            .filter_map(|l| match l {
                Ok((k, _)) => Some(k),
                Err(_) => {
                    deps.api.debug("Error parsing store when iterating admins!");
                    None
                }
            })
            .collect(),
    })
}

fn query_validators_info(deps: Deps, round_id: u64) -> StdResult<ValidatorsInfoResponse> {
    let round_id = get_nearest_store_initialized_round(deps.storage, round_id).unwrap_or_default();

    Ok(ValidatorsInfoResponse {
        round_id,
        validators: VALIDATORS_INFO
            .prefix(round_id)
            .range(deps.storage, None, None, Order::Ascending)
            .filter_map(|l| l.ok())
            .collect(),
    })
}
