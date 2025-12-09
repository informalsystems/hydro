use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdError,
    StdResult,
};
use cw2::set_contract_version;

use crate::{
    error::{new_generic_error, ContractError},
    msg::{ExecuteMsg, InstantiateMsg, ProxyAddressResponse, QueryMsg},
    state::{ADMINS, USER_PROXIES},
};

const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let admins = msg
        .admins
        .iter()
        .map(|addr| deps.api.addr_validate(addr))
        .collect::<StdResult<Vec<Addr>>>()?;

    if admins.is_empty() {
        return Err(new_generic_error("no admins provided"));
    }

    for admin in admins {
        ADMINS.save(deps.storage, admin, &())?;
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new().add_attribute("action", "initialization"))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::RegisterUser {
            user_id,
            proxy_address,
        } => register_user(deps, info, user_id, proxy_address),
    }
}

fn register_user(
    deps: DepsMut,
    info: MessageInfo,
    user_id: String,
    proxy_address: String,
) -> Result<Response, ContractError> {
    ensure_admin(&deps.as_ref(), &info.sender)?;

    let proxy_address = deps.api.addr_validate(&proxy_address)?;

    if USER_PROXIES
        .may_load(deps.storage, user_id.clone())?
        .is_some()
    {
        return Err(new_generic_error(format!(
            "proxy contract for user {} is already registered",
            user_id.clone()
        )));
    }

    USER_PROXIES.save(deps.storage, user_id.clone(), &proxy_address)?;

    Ok(Response::new()
        .add_attribute("action", "register_user")
        .add_attribute("user_id", user_id)
        .add_attribute("proxy_address", proxy_address))
}

fn ensure_admin(deps: &Deps, sender: &Addr) -> Result<(), ContractError> {
    match ADMINS.may_load(deps.storage, sender.clone())? {
        None => Err(ContractError::Unauthorized {}),
        Some(_) => Ok(()),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::ProxyAddress { user_id } => to_json_binary(&query_proxy_address(&deps, user_id)?),
    }
}

fn query_proxy_address(deps: &Deps, user_id: String) -> StdResult<ProxyAddressResponse> {
    let address = USER_PROXIES
        .may_load(deps.storage, user_id.clone())?
        .ok_or_else(|| {
            StdError::generic_err(format!("proxy contract for user {user_id} not found"))
        })?;

    Ok(ProxyAddressResponse { address })
}
