use std::collections::BTreeSet;

use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdResult,
};
use cw2::set_contract_version;

use crate::{
    error::ContractError,
    msg::{ConfigResponse, ControlCentersResponse, ExecuteMsg, InstantiateMsg, QueryMsg},
    state::{ADMIN, CONTROL_CENTERS, PENDING_ADMIN},
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
    let admin = deps.api.addr_validate(&msg.admin)?;
    let control_centers = msg
        .control_centers
        .iter()
        .map(|address| deps.api.addr_validate(address))
        .collect::<StdResult<BTreeSet<Addr>>>()?;
    let control_center_count = control_centers.len();

    ADMIN.save(deps.storage, &admin)?;
    for address in control_centers {
        CONTROL_CENTERS.save(deps.storage, address, &())?;
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new()
        .add_attribute("action", "initialization")
        .add_attribute("admin", admin)
        .add_attribute("control_center_count", control_center_count.to_string()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::ProposeAdmin { new_admin } => propose_admin(deps, info, new_admin),
        ExecuteMsg::AcceptAdmin {} => accept_admin(deps, info),
        ExecuteMsg::AddControlCenter { address } => add_control_center(deps, info, address),
        ExecuteMsg::RemoveControlCenter { address } => remove_control_center(deps, info, address),
    }
}

fn ensure_admin(deps: Deps, sender: &Addr) -> Result<(), ContractError> {
    if sender != ADMIN.load(deps.storage)? {
        return Err(ContractError::Unauthorized {});
    }
    Ok(())
}

fn propose_admin(
    deps: DepsMut,
    info: MessageInfo,
    new_admin: String,
) -> Result<Response, ContractError> {
    ensure_admin(deps.as_ref(), &info.sender)?;
    let new_admin = deps.api.addr_validate(&new_admin)?;
    PENDING_ADMIN.save(deps.storage, &new_admin)?;

    Ok(Response::new()
        .add_attribute("action", "propose_admin")
        .add_attribute("admin", info.sender)
        .add_attribute("pending_admin", new_admin))
}

fn accept_admin(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    if PENDING_ADMIN.may_load(deps.storage)?.as_ref() != Some(&info.sender) {
        return Err(ContractError::Unauthorized {});
    }
    let old_admin = ADMIN.load(deps.storage)?;
    ADMIN.save(deps.storage, &info.sender)?;
    PENDING_ADMIN.remove(deps.storage);

    Ok(Response::new()
        .add_attribute("action", "accept_admin")
        .add_attribute("old_admin", old_admin)
        .add_attribute("new_admin", info.sender))
}

fn add_control_center(
    deps: DepsMut,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    ensure_admin(deps.as_ref(), &info.sender)?;
    let address = deps.api.addr_validate(&address)?;
    if CONTROL_CENTERS.has(deps.storage, address.clone()) {
        return Err(ContractError::AlreadyRegistered {
            address: address.to_string(),
        });
    }
    CONTROL_CENTERS.save(deps.storage, address.clone(), &())?;

    Ok(Response::new()
        .add_attribute("action", "add_control_center")
        .add_attribute("sender", info.sender)
        .add_attribute("control_center_address", address))
}

fn remove_control_center(
    deps: DepsMut,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    ensure_admin(deps.as_ref(), &info.sender)?;
    let address = deps.api.addr_validate(&address)?;
    if !CONTROL_CENTERS.has(deps.storage, address.clone()) {
        return Err(ContractError::NotRegistered {
            address: address.to_string(),
        });
    }
    CONTROL_CENTERS.remove(deps.storage, address.clone());

    Ok(Response::new()
        .add_attribute("action", "remove_control_center")
        .add_attribute("sender", info.sender)
        .add_attribute("control_center_address", address))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&ConfigResponse {
            admin: ADMIN.load(deps.storage)?,
            pending_admin: PENDING_ADMIN.may_load(deps.storage)?,
        }),
        QueryMsg::ControlCenters {} => to_json_binary(&ControlCentersResponse {
            control_centers: CONTROL_CENTERS
                .keys(deps.storage, None, None, Order::Ascending)
                .collect::<StdResult<Vec<Addr>>>()?,
        }),
    }
}
