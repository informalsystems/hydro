use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdResult, Uint128,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;

use crate::{
    error::ContractError,
    msg::{ExecuteMsg, InstantiateMsg},
    query::{AdminsResponse, ConfigResponse, QueryMsg, RootHashResponse},
    state::{Config, ADMINS, CONFIG, ROOT_HASHES},
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

    // Save the config with the hydro contract address
    let config = Config {
        hydro_contract: msg.hydro_contract,
    };
    CONFIG.save(deps.storage, &config)?;

    // Add the provided admins
    if msg.admins.is_empty() {
        // If no admins provided, add the sender as an admin
        ADMINS.save(
            deps.storage,
            deps.api.addr_validate(info.sender.as_str())?,
            &true,
        )?;
    } else {
        // Add all provided admin addresses
        for admin in msg.admins {
            let admin_addr = deps.api.addr_validate(&admin)?;
            ADMINS.save(deps.storage, admin_addr, &true)?;
        }
    }

    Ok(Response::new().add_attribute("action", "initialisation"))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::AddRootHash {
            timestamp,
            root_hash,
        } => execute_add_root_hash(deps, info, timestamp, root_hash),
        ExecuteMsg::AddAdmin { address } => execute_add_admin(deps, info, address),
        ExecuteMsg::RemoveAdmin { address } => execute_remove_admin(deps, info, address),
        ExecuteMsg::Lock {
            amount,
            merkle_proof,
            root_id,
        } => execute_lock(deps, info, amount, merkle_proof, root_id),
    }
}

pub fn execute_add_root_hash(
    deps: DepsMut,
    info: MessageInfo,
    timestamp: u64,
    root_hash: String,
) -> Result<Response, ContractError> {
    // Check that the sender is an admin
    if !ADMINS
        .may_load(deps.storage, info.sender.clone())?
        .unwrap_or(false)
    {
        return Err(ContractError::Unauthorized {});
    }

    // check merkle root length
    let mut root_buf: [u8; 32] = [0; 32];
    hex::decode_to_slice(&root_hash, &mut root_buf)?;

    // Save the root hash with the timestamp as the key
    ROOT_HASHES.save(deps.storage, timestamp, &root_hash)?;

    Ok(Response::new()
        .add_attribute("action", "add_root_hash")
        .add_attribute("timestamp", timestamp.to_string())
        .add_attribute("root_hash", root_hash))
}

pub fn execute_add_admin(
    deps: DepsMut,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    // Check that the sender is an admin
    if !ADMINS
        .may_load(deps.storage, info.sender.clone())?
        .unwrap_or(false)
    {
        return Err(ContractError::Unauthorized {});
    }

    // Validate and save the new admin
    let admin_addr = deps.api.addr_validate(&address)?;
    ADMINS.save(deps.storage, admin_addr, &true)?;

    Ok(Response::new()
        .add_attribute("action", "add_admin")
        .add_attribute("admin", address))
}

pub fn execute_remove_admin(
    deps: DepsMut,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    // Check that the sender is an admin
    if !ADMINS
        .may_load(deps.storage, info.sender.clone())?
        .unwrap_or(false)
    {
        return Err(ContractError::Unauthorized {});
    }

    // Validate the admin to remove
    let admin_addr = deps.api.addr_validate(&address)?;

    // Count the number of admins
    let admin_count = ADMINS
        .keys(deps.storage, None, None, Order::Ascending)
        .count();

    // Prevent removing the last admin
    if admin_count <= 1 {
        return Err(ContractError::CannotRemoveLastAdmin {});
    }

    // Remove the admin
    ADMINS.remove(deps.storage, admin_addr);

    Ok(Response::new()
        .add_attribute("action", "remove_admin")
        .add_attribute("admin", address))
}

fn execute_lock(
    _deps: DepsMut,
    _info: MessageInfo,
    _amount: Uint128,
    _merkle_proof: Vec<String>,
    _root_id: u64,
) -> Result<Response, ContractError> {
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::GetRootHash { timestamp } => {
            to_json_binary(&query_get_root_hash(deps, timestamp)?)
        }
        QueryMsg::GetAdmins {} => to_json_binary(&query_get_admins(deps)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

fn query_get_root_hash(deps: Deps, timestamp: u64) -> StdResult<RootHashResponse> {
    // Get all root hashes with timestamp less than or equal to the provided timestamp
    let root_hashes: Vec<(u64, String)> = ROOT_HASHES
        .range(
            deps.storage,
            None,
            Some(Bound::exclusive(timestamp + 1)),
            Order::Ascending,
        )
        .collect::<StdResult<Vec<(u64, String)>>>()?;

    // Get the root hash with the largest timestamp that is less than or equal to the provided timestamp
    if let Some((ts, hash)) = root_hashes.into_iter().rev().next() {
        Ok(RootHashResponse {
            timestamp: ts,
            root_hash: hash,
        })
    } else {
        Err(cosmwasm_std::StdError::not_found(
            "No root hash found for the given timestamp",
        ))
    }
}

fn query_get_admins(deps: Deps) -> StdResult<AdminsResponse> {
    let admins: StdResult<Vec<String>> = ADMINS
        .keys(deps.storage, None, None, Order::Ascending)
        .map(|addr_result| addr_result.map(|addr| addr.to_string()))
        .collect();

    Ok(AdminsResponse { admins: admins? })
}
