use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdResult,
};
use cw2::set_contract_version;

use crate::{
    error::ContractError,
    msg::{ConfigResponse, ControlCentersResponse, ExecuteMsg, InstantiateMsg, QueryMsg},
    state::{ADMIN, CONTROL_CENTERS},
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
        .collect::<StdResult<Vec<Addr>>>()?;

    ADMIN.save(deps.storage, &admin)?;
    for address in control_centers {
        CONTROL_CENTERS.save(deps.storage, address, &())?;
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
    if info.sender != ADMIN.load(deps.storage)? {
        return Err(ContractError::Unauthorized {});
    }

    let (action, address) = match msg {
        ExecuteMsg::UpdateAdmin { new_admin } => {
            let new_admin = deps.api.addr_validate(&new_admin)?;
            ADMIN.save(deps.storage, &new_admin)?;
            return Ok(Response::new()
                .add_attribute("action", "update_admin")
                .add_attribute("old_admin", info.sender)
                .add_attribute("new_admin", new_admin));
        }
        ExecuteMsg::AddControlCenter { address } => {
            let address = deps.api.addr_validate(&address)?;
            if CONTROL_CENTERS.has(deps.storage, address.clone()) {
                return Err(ContractError::AlreadyRegistered {
                    address: address.to_string(),
                });
            }
            CONTROL_CENTERS.save(deps.storage, address.clone(), &())?;
            ("add_control_center", address)
        }
        ExecuteMsg::RemoveControlCenter { address } => {
            let address = deps.api.addr_validate(&address)?;
            if !CONTROL_CENTERS.has(deps.storage, address.clone()) {
                return Err(ContractError::NotRegistered {
                    address: address.to_string(),
                });
            }
            CONTROL_CENTERS.remove(deps.storage, address.clone());
            ("remove_control_center", address)
        }
    };

    Ok(Response::new()
        .add_attribute("action", action)
        .add_attribute("sender", info.sender)
        .add_attribute("control_center_address", address))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&ConfigResponse {
            admin: ADMIN.load(deps.storage)?,
        }),
        QueryMsg::ControlCenters {} => to_json_binary(&ControlCentersResponse {
            control_centers: CONTROL_CENTERS
                .keys(deps.storage, None, None, Order::Ascending)
                .collect::<StdResult<Vec<Addr>>>()?,
        }),
    }
}
