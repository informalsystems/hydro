use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Order,
    Response, StdResult,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;

use crate::{
    error::ContractError,
    msg::{AllPairsResponse, ConfigResponse, ExecuteMsg, InstantiateMsg, PairResponse, QueryMsg},
    state::{ADMIN, PAIRS},
};

const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const DEFAULT_LIMIT: u32 = 30;
const MAX_LIMIT: u32 = 100;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let admin = deps.api.addr_validate(&msg.admin)?;
    ADMIN.save(deps.storage, &admin)?;

    for pair in &msg.pairs {
        PAIRS.save(
            deps.storage,
            pair.neutron_shares_denom.as_str(),
            &pair.cosmos_hub_shares_denom,
        )?;
    }

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("admin", admin))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Convert {} => convert(deps, env, info),
        ExecuteMsg::AddPair {
            neutron_shares_denom,
            cosmos_hub_shares_denom,
        } => add_pair(deps, info, neutron_shares_denom, cosmos_hub_shares_denom),
        ExecuteMsg::RemovePair {
            neutron_shares_denom,
        } => remove_pair(deps, info, neutron_shares_denom),
        ExecuteMsg::UpdateAdmin { new_admin } => update_admin(deps, info, new_admin),
    }
}

fn convert(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    if info.funds.len() != 1 {
        return Err(ContractError::InvalidFunds);
    }
    let sent = &info.funds[0];

    let cosmos_hub_denom = PAIRS
        .may_load(deps.storage, sent.denom.as_str())?
        .ok_or_else(|| ContractError::PairNotFound {
            denom: sent.denom.clone(),
        })?;

    let contract_balance = deps
        .querier
        .query_balance(&env.contract.address, &cosmos_hub_denom)?;

    if contract_balance.amount < sent.amount {
        return Err(ContractError::InsufficientBalance {
            available: contract_balance.amount,
            required: sent.amount,
        });
    }

    Ok(Response::new()
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin {
                denom: cosmos_hub_denom.clone(),
                amount: sent.amount,
            }],
        })
        .add_attribute("action", "convert")
        .add_attribute("sender", &info.sender)
        .add_attribute("neutron_denom", &sent.denom)
        .add_attribute("cosmos_hub_denom", cosmos_hub_denom)
        .add_attribute("amount", sent.amount))
}

fn add_pair(
    deps: DepsMut,
    info: MessageInfo,
    neutron_shares_denom: String,
    cosmos_hub_shares_denom: String,
) -> Result<Response, ContractError> {
    let admin = ADMIN.load(deps.storage)?;
    if info.sender != admin {
        return Err(ContractError::Unauthorized);
    }

    if PAIRS
        .may_load(deps.storage, neutron_shares_denom.as_str())?
        .is_some()
    {
        return Err(ContractError::PairAlreadyExists {
            denom: neutron_shares_denom,
        });
    }

    PAIRS.save(
        deps.storage,
        neutron_shares_denom.as_str(),
        &cosmos_hub_shares_denom,
    )?;

    Ok(Response::new()
        .add_attribute("action", "add_pair")
        .add_attribute("neutron_denom", neutron_shares_denom)
        .add_attribute("cosmos_hub_denom", cosmos_hub_shares_denom))
}

fn remove_pair(
    deps: DepsMut,
    info: MessageInfo,
    neutron_shares_denom: String,
) -> Result<Response, ContractError> {
    let admin = ADMIN.load(deps.storage)?;
    if info.sender != admin {
        return Err(ContractError::Unauthorized);
    }

    if PAIRS
        .may_load(deps.storage, neutron_shares_denom.as_str())?
        .is_none()
    {
        return Err(ContractError::PairNotFound {
            denom: neutron_shares_denom,
        });
    }

    PAIRS.remove(deps.storage, neutron_shares_denom.as_str());

    Ok(Response::new()
        .add_attribute("action", "remove_pair")
        .add_attribute("neutron_denom", neutron_shares_denom))
}

fn update_admin(
    deps: DepsMut,
    info: MessageInfo,
    new_admin: String,
) -> Result<Response, ContractError> {
    let admin = ADMIN.load(deps.storage)?;
    if info.sender != admin {
        return Err(ContractError::Unauthorized);
    }

    let new_admin_addr = deps.api.addr_validate(&new_admin)?;
    ADMIN.save(deps.storage, &new_admin_addr)?;

    Ok(Response::new()
        .add_attribute("action", "update_admin")
        .add_attribute("old_admin", admin)
        .add_attribute("new_admin", new_admin_addr))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::Pair { neutron_denom } => to_json_binary(&query_pair(deps, neutron_denom)?),
        QueryMsg::AllPairs { start_after, limit } => {
            to_json_binary(&query_all_pairs(deps, start_after, limit)?)
        }
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let admin = ADMIN.load(deps.storage)?;
    Ok(ConfigResponse {
        admin: admin.to_string(),
    })
}

fn query_pair(deps: Deps, neutron_denom: String) -> StdResult<Option<PairResponse>> {
    let cosmos_hub_denom = PAIRS.may_load(deps.storage, neutron_denom.as_str())?;
    Ok(
        cosmos_hub_denom.map(|cosmos_hub_shares_denom| PairResponse {
            neutron_shares_denom: neutron_denom,
            cosmos_hub_shares_denom,
        }),
    )
}

fn query_all_pairs(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<AllPairsResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let start = start_after.as_deref().map(Bound::exclusive);

    let pairs = PAIRS
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|item| {
            item.map(
                |(neutron_shares_denom, cosmos_hub_shares_denom)| PairResponse {
                    neutron_shares_denom,
                    cosmos_hub_shares_denom,
                },
            )
        })
        .collect::<StdResult<Vec<_>>>()?;

    Ok(AllPairsResponse { pairs })
}
