use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdResult, Uint128,
};
use cw2::set_contract_version;
use interface::{
    inflow_control_center::{
        Config, ConfigResponse, ExecuteMsg, PoolInfoResponse, QueryMsg, SubvaultsResponse,
        UpdateConfigData, WhitelistResponse,
    },
    inflow_vault::{PoolInfoResponse as VaultPoolInfoResponse, QueryMsg as VaultQueryMsg},
};
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

use crate::{
    error::{new_generic_error, ContractError},
    msg::InstantiateMsg,
    state::{load_config, CONFIG, DEPLOYED_AMOUNT, SUBVAULTS, WHITELIST},
};

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    CONFIG.save(
        deps.storage,
        &Config {
            deposit_cap: msg.deposit_cap,
        },
    )?;

    DEPLOYED_AMOUNT.save(deps.storage, &Uint128::zero(), env.block.height)?;

    let whitelist_addresses = msg
        .whitelist
        .iter()
        .filter_map(|addr| deps.api.addr_validate(addr).ok())
        .collect::<Vec<_>>();

    if whitelist_addresses.is_empty() {
        return Err(new_generic_error(
            "at least one whitelist address must be provided",
        ));
    }

    for whitelist_address in &whitelist_addresses {
        WHITELIST.save(deps.storage, whitelist_address.clone(), &())?;
    }

    let subvaults = msg
        .subvaults
        .iter()
        .filter_map(|addr| deps.api.addr_validate(addr).ok())
        .collect::<Vec<_>>();

    for subvault in &subvaults {
        SUBVAULTS.save(deps.storage, subvault.clone(), &())?;
    }

    Ok(Response::new()
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender)
        .add_attribute(
            "whitelist",
            whitelist_addresses
                .iter()
                .map(|addr| addr.to_string())
                .collect::<Vec<String>>()
                .join(", "),
        )
        .add_attribute(
            "subvaults",
            subvaults
                .iter()
                .map(|addr| addr.to_string())
                .collect::<Vec<String>>()
                .join(", "),
        ))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        ExecuteMsg::SubmitDeployedAmount { amount } => {
            submit_deployed_amount(deps, env, info, amount)
        }
        ExecuteMsg::AddToDeployedAmount { amount_to_add } => {
            add_to_deployed_amount(deps, env, info, amount_to_add)
        }
        ExecuteMsg::SubFromDeployedAmount { amount_to_sub } => {
            sub_from_deployed_amount(deps, env, info, amount_to_sub)
        }
        ExecuteMsg::AddToWhitelist { address } => add_to_whitelist(deps, env, info, address),
        ExecuteMsg::RemoveFromWhitelist { address } => {
            remove_from_whitelist(deps, env, info, address)
        }
        ExecuteMsg::AddSubvault { address } => add_subvault(deps, info, address),
        ExecuteMsg::RemoveSubvault { address } => remove_subvault(deps, info, address),
        ExecuteMsg::UpdateConfig { config } => update_config(deps, info, config),
    }
}

fn submit_deployed_amount(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    DEPLOYED_AMOUNT.save(deps.storage, &amount, env.block.height)?;

    Ok(Response::new()
        .add_attribute("action", "submit_deployed_amount")
        .add_attribute("sender", info.sender)
        .add_attribute("amount", amount))
}

fn update_deployed_amount(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    sender: Addr,
    amount: Uint128,
    to_add: bool, // true = add, false = sub
) -> Result<(), ContractError> {
    // Only registered sub-vaults can execute this function.
    // This happens when a whitelisted address:
    // - withdraws funds for deployment
    // or
    // - deposits funds from deployment
    // from a sub-vault.
    // This can also happen during adapter deposit/withdraw flows.
    if !SUBVAULTS.has(deps.storage, sender.clone()) {
        return Err(ContractError::Unauthorized);
    }

    if to_add {
        DEPLOYED_AMOUNT.update(deps.storage, env.block.height, |current_value| {
            current_value
                .unwrap_or_default()
                .checked_add(amount)
                .map_err(|e| new_generic_error(format!("overflow error: {e}")))
        })?;
    } else {
        DEPLOYED_AMOUNT.update(deps.storage, env.block.height, |current_value| {
            current_value
                .unwrap_or_default()
                .checked_sub(amount)
                .map_err(|e| new_generic_error(format!("overflow error: {e}")))
        })?;
    }

    Ok(())
}

fn add_to_deployed_amount(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    amount_to_add: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    update_deployed_amount(deps, env, info.sender.clone(), amount_to_add, true)?;

    Ok(Response::new()
        .add_attribute("action", "add_to_deployed_amount")
        .add_attribute("sender", info.sender)
        .add_attribute("amount_to_add", amount_to_add))
}

fn sub_from_deployed_amount(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    amount_to_sub: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    update_deployed_amount(deps, env, info.sender.clone(), amount_to_sub, false)?;

    Ok(Response::new()
        .add_attribute("action", "sub_from_deployed_amount")
        .add_attribute("sender", info.sender)
        .add_attribute("amount_to_sub", amount_to_sub))
}

// Adds a new account address to the whitelist.
fn add_to_whitelist(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;
    let whitelist_address = deps.api.addr_validate(&address)?;

    // Return an error if the account address is already in the whitelist
    if WHITELIST
        .may_load(deps.storage, whitelist_address.clone())?
        .is_some()
    {
        return Err(new_generic_error(format!(
            "address {whitelist_address} is already in the whitelist"
        )));
    }

    // Add address to whitelist
    WHITELIST.save(deps.storage, whitelist_address.clone(), &())?;

    Ok(Response::new()
        .add_attribute("action", "add_to_whitelist")
        .add_attribute("sender", info.sender)
        .add_attribute("added_whitelist_address", whitelist_address))
}

// Removes an account address from the whitelist.
fn remove_from_whitelist(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;
    let whitelist_address = deps.api.addr_validate(&address)?;

    // Return an error if the account address is not in the whitelist
    if WHITELIST
        .may_load(deps.storage, whitelist_address.clone())?
        .is_none()
    {
        return Err(new_generic_error(format!(
            "address {whitelist_address} is not in the whitelist"
        )));
    }

    // Remove address from the whitelist
    WHITELIST.remove(deps.storage, whitelist_address.clone());

    if WHITELIST
        .keys(deps.storage, None, None, Order::Ascending)
        .count()
        == 0
    {
        return Err(new_generic_error(
            "cannot remove last outstanding whitelisted address",
        ));
    }

    Ok(Response::new()
        .add_attribute("action", "remove_from_whitelist")
        .add_attribute("sender", info.sender)
        .add_attribute("removed_whitelist_address", whitelist_address))
}

fn update_config(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    config_update: UpdateConfigData,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    let mut config = load_config(deps.storage)?;

    let mut response = Response::new()
        .add_attribute("action", "update_config")
        .add_attribute("sender", info.sender);

    if let Some(deposit_cap) = config_update.deposit_cap {
        config.deposit_cap = deposit_cap;

        response = response.add_attribute("deposit_cap", deposit_cap.to_string());
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(response)
}

fn add_subvault(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    let subvault_address = deps.api.addr_validate(&address)?;

    if SUBVAULTS.has(deps.storage, subvault_address.clone()) {
        return Err(new_generic_error(format!(
            "sub-vault address {subvault_address} is already registered with the control center"
        )));
    }

    SUBVAULTS.save(deps.storage, subvault_address.clone(), &())?;

    Ok(Response::new()
        .add_attribute("action", "add_subvault")
        .add_attribute("sender", info.sender)
        .add_attribute("subvault_address", subvault_address))
}

fn remove_subvault(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    let subvault_address = deps.api.addr_validate(&address)?;

    if !SUBVAULTS.has(deps.storage, subvault_address.clone()) {
        return Err(new_generic_error(format!(
            "sub-vault address {subvault_address} is not registered with the control center",
        )));
    }

    SUBVAULTS.remove(deps.storage, subvault_address.clone());

    Ok(Response::new()
        .add_attribute("action", "remove_subvault")
        .add_attribute("sender", info.sender)
        .add_attribute("subvault_address", subvault_address))
}

fn validate_address_is_whitelisted(
    deps: &DepsMut<NeutronQuery>,
    address: Addr,
) -> Result<(), ContractError> {
    match WHITELIST.may_load(deps.storage, address)? {
        Some(_) => Ok(()),
        None => Err(ContractError::Unauthorized),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<NeutronQuery>, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(&deps)?),
        QueryMsg::PoolInfo {} => to_json_binary(&query_pool_info(&deps, &env)?),
        QueryMsg::DeployedAmount {} => to_json_binary(&query_deployed_amount(&deps)?),
        QueryMsg::Whitelist {} => to_json_binary(&query_whitelist(&deps)?),
        QueryMsg::Subvaults {} => to_json_binary(&query_subvaults(&deps)?),
    }
}

fn query_config(deps: &Deps<NeutronQuery>) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: load_config(deps.storage)?,
    })
}

fn query_pool_info(deps: &Deps<NeutronQuery>, _env: &Env) -> StdResult<PoolInfoResponse> {
    let sub_vaults = SUBVAULTS
        .keys(deps.storage, None, None, Order::Ascending)
        .filter_map(|v| v.ok())
        .collect::<Vec<Addr>>();

    let mut total_balance = Uint128::zero();
    let mut total_adapter_deposits = Uint128::zero();
    let mut total_shares_issued = Uint128::zero();
    let mut total_withdrawal_amount = Uint128::zero();

    for sub_vault in sub_vaults {
        let vault_info: VaultPoolInfoResponse = deps
            .querier
            .query_wasm_smart(sub_vault.to_string(), &VaultQueryMsg::PoolInfo {})?;

        total_balance = total_balance.checked_add(vault_info.balance_base_tokens)?;
        total_adapter_deposits =
            total_adapter_deposits.checked_add(vault_info.adapter_deposits_base_tokens)?;
        total_shares_issued = total_shares_issued.checked_add(vault_info.shares_issued)?;
        total_withdrawal_amount =
            total_withdrawal_amount.checked_add(vault_info.withdrawal_queue_base_tokens)?;
    }

    let deployed_amount = query_deployed_amount(deps)?;

    // If the sum of total balance, deployed amount and adapter deposits is less than the total withdrawal amount, return zero.
    let total_pool_value = total_balance
        .checked_add(deployed_amount)?
        .checked_add(total_adapter_deposits)?
        .checked_sub(total_withdrawal_amount)
        .unwrap_or_default();

    Ok(PoolInfoResponse {
        total_pool_value,
        total_shares_issued,
    })
}

fn query_deployed_amount(deps: &Deps<NeutronQuery>) -> StdResult<Uint128> {
    DEPLOYED_AMOUNT.load(deps.storage)
}

fn query_whitelist(deps: &Deps<NeutronQuery>) -> StdResult<WhitelistResponse> {
    Ok(WhitelistResponse {
        whitelist: WHITELIST
            .keys(deps.storage, None, None, Order::Ascending)
            .filter_map(|w| w.ok())
            .collect(),
    })
}

fn query_subvaults(deps: &Deps<NeutronQuery>) -> StdResult<SubvaultsResponse> {
    Ok(SubvaultsResponse {
        subvaults: SUBVAULTS
            .keys(deps.storage, None, None, Order::Ascending)
            .filter_map(|w| w.ok())
            .collect(),
    })
}

/// Converts a slice of items into a comma-separated string of their string representations.
pub fn get_slice_as_attribute<T: ToString>(input: &[T]) -> String {
    input
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<String>>()
        .join(",")
}
