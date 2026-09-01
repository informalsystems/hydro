use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Order,
    Response, StdError, StdResult, Uint128,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;
use interface::utils::{DEFAULT_PAGINATION_LIMIT, MAX_PAGINATION_LIMIT};

use crate::error::ContractError;
use crate::eureka::{build_eureka_transfer_msg, EurekaTransferParams};
use crate::msg::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AdminsResponse, AllPositionsResponse,
    AllowedDenomsResponse, AllowedDestinationAddressesResponse, AvailableAmountResponse,
    ConfigResponse, DepositorCapabilitiesResponse, DepositorPositionResponse,
    DepositorPositionsResponse, ExecuteMsg, ExecutorInfo, ExecutorsResponse, IbcEurekaAdapterMsg,
    IbcEurekaAdapterQueryMsg, InstantiateMsg, QueryMsg, RegisteredDepositorInfo,
    RegisteredDepositorsResponse, TimeEstimateResponse, UpdateConfigData,
};
use crate::state::{
    Config, Depositor, DepositorCapabilities, ADMINS, ALLOWED_DENOMS,
    ALLOWED_DESTINATION_ADDRESSES, CONFIG, EXECUTORS, WHITELISTED_DEPOSITORS,
};
use crate::validation::{
    get_depositor, get_destination_address, normalize_evm_address, validate_admin_caller,
    validate_depositor_caller, validate_executor_caller,
};

pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const SOLIDITY_ENCODING: &str = "application/x-solidity-abi";

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // Validate at least one admin
    if msg.admins.is_empty() {
        return Err(ContractError::AtLeastOneAdmin {});
    }

    // Validate admins
    let validated_admins = msg
        .admins
        .iter()
        .map(|a| deps.api.addr_validate(a))
        .collect::<StdResult<Vec<_>>>()?;

    // Deduplicate admins
    let mut unique_admins = validated_admins;
    unique_admins.sort();
    unique_admins.dedup();

    ADMINS.save(deps.storage, &unique_admins)?;

    // Validate executors
    let validated_executors = msg
        .initial_executors
        .iter()
        .map(|e| deps.api.addr_validate(&e.address))
        .collect::<StdResult<Vec<_>>>()?;

    // Deduplicate executors
    let mut unique_executors = validated_executors;
    unique_executors.sort();
    unique_executors.dedup();

    for executor in unique_executors {
        EXECUTORS.save(deps.storage, executor, &())?;
    }

    // Register initial depositors
    for initial_depositor in msg.initial_depositors {
        let addr = deps.api.addr_validate(&initial_depositor.address)?;

        // Check for duplicates
        if WHITELISTED_DEPOSITORS.has(deps.storage, addr.clone()) {
            return Err(ContractError::DepositorAlreadyRegistered {
                depositor_address: addr.to_string(),
            });
        }

        // Use provided capabilities or default
        let capabilities = initial_depositor
            .capabilities
            .unwrap_or(DepositorCapabilities { can_withdraw: true });

        let depositor = Depositor {
            enabled: true,
            capabilities,
        };

        WHITELISTED_DEPOSITORS.save(deps.storage, addr.clone(), &depositor)?;
    }

    // Register initial allowed denoms
    for denom in msg.initial_denoms {
        let denom = denom.trim().to_string();
        if denom.is_empty() {
            return Err(ContractError::InvalidConfig {
                reason: "denom cannot be empty".to_string(),
            });
        }

        if ALLOWED_DENOMS.has(deps.storage, denom.clone()) {
            return Err(ContractError::DenomAlreadyAllowed { denom });
        }

        ALLOWED_DENOMS.save(deps.storage, denom, &())?;
    }

    // Register initial allowed destination addresses
    for dest_addr in msg.initial_allowed_destination_addresses {
        let normalized_address = normalize_evm_address(&dest_addr)?;

        if ALLOWED_DESTINATION_ADDRESSES.has(deps.storage, normalized_address.clone()) {
            return Err(ContractError::DestinationAddressAlreadyExists {
                address: normalized_address,
            });
        }

        ALLOWED_DESTINATION_ADDRESSES.save(deps.storage, normalized_address, &())?;
    }

    // Validate config fields
    let skip_swap_entry_point_contract = deps
        .api
        .addr_validate(&msg.skip_swap_entry_point_contract)?;
    let eureka_fee_receiver = deps.api.addr_validate(&msg.eureka_fee_receiver)?;

    if msg.source_channel.trim().is_empty() {
        return Err(ContractError::InvalidConfig {
            reason: "source_channel cannot be empty".to_string(),
        });
    }

    let config = Config {
        skip_swap_entry_point_contract: skip_swap_entry_point_contract.to_string(),
        source_channel: msg.source_channel.trim().to_owned(),
        eureka_fee_receiver: eureka_fee_receiver.to_string(),
        encoding: SOLIDITY_ENCODING.to_string(),
        ibc_transfer_timeout_seconds: msg.ibc_transfer_timeout_seconds,
        eureka_fee_timeout_seconds: msg.eureka_fee_timeout_seconds,
    };
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("contract_name", CONTRACT_NAME)
        .add_attribute("contract_version", CONTRACT_VERSION)
        .add_attribute(
            "skip_swap_entry_point_contract",
            config.skip_swap_entry_point_contract,
        )
        .add_attribute("source_channel", config.source_channel)
        .add_attribute("eureka_fee_receiver", config.eureka_fee_receiver))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::StandardAction(interface_msg) => {
            dispatch_execute_standard(deps, env, info, interface_msg)
        }
        ExecuteMsg::CustomAction(custom_msg) => {
            dispatch_execute_custom(deps, env, info, custom_msg)
        }
    }
}

/// Dispatch standard adapter interface messages
fn dispatch_execute_standard(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: AdapterInterfaceMsg,
) -> Result<Response, ContractError> {
    match msg {
        AdapterInterfaceMsg::Deposit {} => execute_deposit(deps, info),
        AdapterInterfaceMsg::Withdraw { coin } => execute_withdraw(deps, env, info, coin),
        AdapterInterfaceMsg::RegisterDepositor {
            depositor_address,
            metadata,
        } => execute_register_depositor(deps, info, depositor_address, metadata),
        AdapterInterfaceMsg::UnregisterDepositor { depositor_address } => {
            execute_unregister_depositor(deps, info, depositor_address)
        }
        AdapterInterfaceMsg::SetDepositorEnabled {
            depositor_address,
            enabled,
        } => execute_set_depositor_enabled(deps, info, depositor_address, enabled),
        AdapterInterfaceMsg::AddAdmin { admin_address } => {
            execute_add_admin(deps, info, admin_address)
        }
        AdapterInterfaceMsg::RemoveAdmin { admin_address } => {
            execute_remove_admin(deps, info, admin_address)
        }
    }
}

/// Dispatch IBC Eureka adapter-specific custom messages
fn dispatch_execute_custom(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: IbcEurekaAdapterMsg,
) -> Result<Response, ContractError> {
    match msg {
        IbcEurekaAdapterMsg::UpdateConfig { update } => execute_update_config(deps, info, update),
        IbcEurekaAdapterMsg::TransferFunds {
            denom,
            amount,
            recipient,
        } => execute_transfer_funds(deps, env, info, denom, amount, recipient),
        IbcEurekaAdapterMsg::AddExecutor { executor_address } => {
            execute_add_executor(deps, info, executor_address)
        }
        IbcEurekaAdapterMsg::RemoveExecutor { executor_address } => {
            execute_remove_executor(deps, info, executor_address)
        }
        IbcEurekaAdapterMsg::AddAllowedDenom { denom } => {
            execute_add_allowed_denom(deps, info, denom)
        }
        IbcEurekaAdapterMsg::RemoveAllowedDenom { denom } => {
            execute_remove_allowed_denom(deps, info, denom)
        }
        IbcEurekaAdapterMsg::AddAllowedDestinationAddress { address } => {
            execute_add_allowed_destination_address(deps, info, address)
        }
        IbcEurekaAdapterMsg::RemoveAllowedDestinationAddress { address } => {
            execute_remove_allowed_destination_address(deps, info, address)
        }
    }
}

/// Handle deposit - just holds the funds in the adapter
fn execute_deposit(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    // Validate depositor
    validate_depositor_caller(&deps, &info)?;

    // Exactly one coin must be attached, and its denom must be a registered allowed denom
    let coin = cw_utils::one_coin(&info)?;

    if !ALLOWED_DENOMS.has(deps.storage, coin.denom.clone()) {
        return Err(ContractError::DenomNotAllowed { denom: coin.denom });
    }

    Ok(Response::new()
        .add_attribute("action", "deposit")
        .add_attribute("sender", info.sender)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", coin.denom))
}

/// Handle withdraw from adapter balance to the caller's address.
fn execute_withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    coin: Coin,
) -> Result<Response, ContractError> {
    // Validate depositor
    let depositor = validate_depositor_caller(&deps, &info)?;
    if !depositor.capabilities.can_withdraw {
        return Err(ContractError::WithdrawalNotAllowed {});
    }

    // Validate non-zero amount
    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // Validate token denom is a registered allowed denom
    if !ALLOWED_DENOMS.has(deps.storage, coin.denom.clone()) {
        return Err(ContractError::DenomNotAllowed {
            denom: coin.denom.clone(),
        });
    }

    // Verify that the adapter has sufficient balance
    let balance = deps
        .querier
        .query_balance(env.contract.address, coin.denom.clone())?;

    if balance.amount < coin.amount {
        return Err(ContractError::InsufficientBalance {
            has: balance.amount,
            needs: coin.amount,
        });
    }

    // Prepare msg to send funds to depositor
    let bank_msg = BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![coin.clone()],
    };

    Ok(Response::new()
        .add_message(bank_msg)
        .add_attribute("action", "withdraw")
        .add_attribute("sender", info.sender)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", coin.denom))
}

/// Register a new depositor
fn execute_register_depositor(
    deps: DepsMut,
    info: MessageInfo,
    depositor_address: String,
    metadata: Option<Binary>,
) -> Result<Response, ContractError> {
    // Validate caller is admin
    validate_admin_caller(&deps.as_ref(), &info)?;

    let depositor_address = deps.api.addr_validate(&depositor_address)?;

    // Check if depositor already registered
    if WHITELISTED_DEPOSITORS.has(deps.storage, depositor_address.clone()) {
        return Err(ContractError::DepositorAlreadyRegistered {
            depositor_address: depositor_address.to_string(),
        });
    }

    // Parse capabilities from metadata or use default
    let capabilities = if let Some(cap_binary) = metadata {
        cosmwasm_std::from_json(&cap_binary)?
    } else {
        // Default capabilities: can withdraw
        DepositorCapabilities { can_withdraw: true }
    };

    let depositor = Depositor {
        enabled: true,
        capabilities,
    };

    WHITELISTED_DEPOSITORS.save(deps.storage, depositor_address.clone(), &depositor)?;

    Ok(Response::new()
        .add_attribute("action", "register_depositor")
        .add_attribute("sender", info.sender)
        .add_attribute("depositor_address", depositor_address))
}

/// Unregister a depositor
fn execute_unregister_depositor(
    deps: DepsMut,
    info: MessageInfo,
    depositor_address: String,
) -> Result<Response, ContractError> {
    // Validate caller is admin
    validate_admin_caller(&deps.as_ref(), &info)?;

    let depositor_address = deps.api.addr_validate(&depositor_address)?;
    WHITELISTED_DEPOSITORS.remove(deps.storage, depositor_address.clone());

    Ok(Response::new()
        .add_attribute("action", "unregister_depositor")
        .add_attribute("sender", info.sender)
        .add_attribute("depositor_address", depositor_address))
}

/// Toggle depositor enabled status
fn execute_set_depositor_enabled(
    deps: DepsMut,
    info: MessageInfo,
    depositor_address: String,
    enabled: bool,
) -> Result<Response, ContractError> {
    // Validate caller is admin
    validate_admin_caller(&deps.as_ref(), &info)?;

    let depositor_address = deps.api.addr_validate(&depositor_address)?;

    // Load and update depositor
    let mut depositor = WHITELISTED_DEPOSITORS
        .may_load(deps.storage, depositor_address.clone())?
        .ok_or(ContractError::DepositorNotRegistered {
            depositor_address: depositor_address.to_string(),
        })?;

    depositor.enabled = enabled;
    WHITELISTED_DEPOSITORS.save(deps.storage, depositor_address.clone(), &depositor)?;

    Ok(Response::new()
        .add_attribute("action", "toggle_depositor_enabled")
        .add_attribute("sender", info.sender)
        .add_attribute("depositor_address", depositor_address.to_string())
        .add_attribute("enabled", enabled.to_string()))
}

/// Handle TransferFunds - initiate an IBC Eureka bridge to the destination EVM chain
fn execute_transfer_funds(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    denom: String,
    amount: Uint128,
    recipient: String,
) -> Result<Response, ContractError> {
    // Validate executor role
    validate_executor_caller(&deps, &info)?;

    if amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // Validate denom is registered as allowed for this adapter instance
    if !ALLOWED_DENOMS.has(deps.storage, denom.clone()) {
        return Err(ContractError::DenomNotAllowed { denom });
    }

    let config = CONFIG.load(deps.storage)?;

    // Executor must attach the fee amount in the same denom as the token that is being transfered
    let fee_amount = cw_utils::must_pay(&info, &denom)?;

    let total_transfer_amount = amount + fee_amount;

    // Verify contract has sufficient balance
    let balance = deps
        .querier
        .query_balance(env.contract.address.clone(), denom.clone())?;

    if balance.amount < total_transfer_amount {
        return Err(ContractError::InsufficientBalance {
            has: balance.amount,
            needs: total_transfer_amount,
        });
    }

    // Look up destination address from allowlist
    let destination_address = get_destination_address(&deps.as_ref(), &recipient)?;

    let action_timeout_timestamp = env
        .block
        .time
        .plus_seconds(config.ibc_transfer_timeout_seconds)
        .seconds(); // Skip:Go specifies this value in seconds

    let fee_timeout_timestamp = env
        .block
        .time
        .plus_seconds(config.eureka_fee_timeout_seconds)
        .nanos(); // Skip:Go specifies this value in nano seconds

    let wasm_msg = build_eureka_transfer_msg(EurekaTransferParams {
        skip_swap_entry_point_contract: config.skip_swap_entry_point_contract.clone(),
        source_channel: config.source_channel.clone(),
        receiver: format!("0x{destination_address}"),
        recover_address: env.contract.address.to_string(),
        encoding: config.encoding.clone(),
        fee_receiver: config.eureka_fee_receiver.clone(),
        denom: denom.clone(),
        amount,
        fee_amount,
        action_timeout_timestamp,
        fee_timeout_timestamp,
    })?;

    // Return response with the IBC Eureka transfer message
    Ok(Response::new()
        .add_message(wasm_msg)
        .add_attribute("action", "transfer_funds")
        .add_attribute("sender", info.sender)
        .add_attribute("denom", denom)
        .add_attribute("amount", amount)
        .add_attribute("fee_amount", fee_amount)
        .add_attribute("destination_address", destination_address))
}

fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    update: UpdateConfigData,
) -> Result<Response, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let mut config = CONFIG.load(deps.storage)?;

    let mut response = Response::new()
        .add_attribute("action", "update_config")
        .add_attribute("sender", info.sender);

    if let Some(skip_swap_entry_point_contract) = update.skip_swap_entry_point_contract {
        let validated = deps.api.addr_validate(&skip_swap_entry_point_contract)?;

        response = response
            .add_attribute(
                "old_skip_swap_entry_point_contract",
                config.skip_swap_entry_point_contract.clone(),
            )
            .add_attribute("new_skip_swap_entry_point_contract", validated.to_string());

        config.skip_swap_entry_point_contract = validated.to_string();
    }

    if let Some(source_channel) = update.source_channel {
        if source_channel.trim().is_empty() {
            return Err(ContractError::InvalidConfig {
                reason: "source_channel cannot be empty".to_string(),
            });
        }

        response = response
            .add_attribute("old_source_channel", config.source_channel.clone())
            .add_attribute("new_source_channel", source_channel.clone());

        config.source_channel = source_channel;
    }

    if let Some(eureka_fee_receiver) = update.eureka_fee_receiver {
        let validated = deps.api.addr_validate(&eureka_fee_receiver)?;

        response = response
            .add_attribute(
                "old_eureka_fee_receiver",
                config.eureka_fee_receiver.clone(),
            )
            .add_attribute("new_eureka_fee_receiver", validated.to_string());

        config.eureka_fee_receiver = validated.to_string();
    }

    if let Some(encoding) = update.encoding {
        if encoding.trim().is_empty() {
            return Err(ContractError::InvalidConfig {
                reason: "encoding cannot be empty".to_string(),
            });
        }

        response = response
            .add_attribute("old_encoding", config.encoding.clone())
            .add_attribute("new_encoding", encoding.clone());

        config.encoding = encoding;
    }

    if let Some(ibc_transfer_timeout_seconds) = update.ibc_transfer_timeout_seconds {
        response = response
            .add_attribute(
                "old_ibc_transfer_timeout_seconds",
                config.ibc_transfer_timeout_seconds.to_string(),
            )
            .add_attribute(
                "new_ibc_transfer_timeout_seconds",
                ibc_transfer_timeout_seconds.to_string(),
            );

        config.ibc_transfer_timeout_seconds = ibc_transfer_timeout_seconds;
    }

    if let Some(eureka_fee_timeout_seconds) = update.eureka_fee_timeout_seconds {
        response = response
            .add_attribute(
                "old_eureka_fee_timeout_seconds",
                config.eureka_fee_timeout_seconds.to_string(),
            )
            .add_attribute(
                "new_eureka_fee_timeout_seconds",
                eureka_fee_timeout_seconds.to_string(),
            );

        config.eureka_fee_timeout_seconds = eureka_fee_timeout_seconds;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(response)
}

fn execute_add_executor(
    deps: DepsMut,
    info: MessageInfo,
    executor_address: String,
) -> Result<Response, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let executor_addr = deps.api.addr_validate(&executor_address)?;

    // Check if already exists
    if EXECUTORS.has(deps.storage, executor_addr.clone()) {
        return Err(ContractError::ExecutorAlreadyExists {
            executor: executor_address,
        });
    }

    EXECUTORS.save(deps.storage, executor_addr.clone(), &())?;

    Ok(Response::new()
        .add_attribute("action", "add_executor")
        .add_attribute("sender", info.sender)
        .add_attribute("executor", executor_addr))
}

fn execute_remove_executor(
    deps: DepsMut,
    info: MessageInfo,
    executor_address: String,
) -> Result<Response, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let executor_addr = deps.api.addr_validate(&executor_address)?;

    // Check if exists
    if !EXECUTORS.has(deps.storage, executor_addr.clone()) {
        return Err(ContractError::ExecutorNotFound {
            executor: executor_address,
        });
    }

    EXECUTORS.remove(deps.storage, executor_addr.clone());

    Ok(Response::new()
        .add_attribute("action", "remove_executor")
        .add_attribute("sender", info.sender)
        .add_attribute("executor", executor_addr))
}

fn execute_add_admin(
    deps: DepsMut,
    info: MessageInfo,
    admin_address: String,
) -> Result<Response, ContractError> {
    // Validate caller is admin
    validate_admin_caller(&deps.as_ref(), &info)?;

    let admin_addr = deps.api.addr_validate(&admin_address)?;
    let mut admins = ADMINS.load(deps.storage)?;

    // Check if already exists
    if admins.contains(&admin_addr) {
        return Err(ContractError::AdminAlreadyExists {
            admin: admin_address,
        });
    }

    // Add new admin
    admins.push(admin_addr.clone());
    ADMINS.save(deps.storage, &admins)?;

    Ok(Response::new()
        .add_attribute("action", "add_admin")
        .add_attribute("sender", info.sender)
        .add_attribute("admin", admin_addr))
}

fn execute_remove_admin(
    deps: DepsMut,
    info: MessageInfo,
    admin_address: String,
) -> Result<Response, ContractError> {
    // Validate caller is admin
    validate_admin_caller(&deps.as_ref(), &info)?;

    let admin_addr = deps.api.addr_validate(&admin_address)?;
    let mut admins = ADMINS.load(deps.storage)?;

    // Check if admin exists
    if !admins.contains(&admin_addr) {
        return Err(ContractError::AdminNotFound {
            admin: admin_address,
        });
    }

    // Prevent removing the last admin
    if admins.len() <= 1 {
        return Err(ContractError::CannotRemoveLastAdmin {});
    }

    // Remove the admin
    admins.retain(|a| a != admin_addr);
    ADMINS.save(deps.storage, &admins)?;

    Ok(Response::new()
        .add_attribute("action", "remove_admin")
        .add_attribute("sender", info.sender)
        .add_attribute("admin", admin_addr))
}

fn execute_add_allowed_denom(
    deps: DepsMut,
    info: MessageInfo,
    denom: String,
) -> Result<Response, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let denom = denom.trim().to_string();
    if denom.is_empty() {
        return Err(ContractError::InvalidConfig {
            reason: "denom cannot be empty".to_string(),
        });
    }

    if ALLOWED_DENOMS.has(deps.storage, denom.clone()) {
        return Err(ContractError::DenomAlreadyAllowed { denom });
    }

    ALLOWED_DENOMS.save(deps.storage, denom.clone(), &())?;

    Ok(Response::new()
        .add_attribute("action", "add_allowed_denom")
        .add_attribute("sender", info.sender)
        .add_attribute("denom", denom))
}

fn execute_remove_allowed_denom(
    deps: DepsMut,
    info: MessageInfo,
    denom: String,
) -> Result<Response, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    if !ALLOWED_DENOMS.has(deps.storage, denom.clone()) {
        return Err(ContractError::DenomNotAllowed { denom });
    }

    ALLOWED_DENOMS.remove(deps.storage, denom.clone());

    Ok(Response::new()
        .add_attribute("action", "remove_allowed_denom")
        .add_attribute("sender", info.sender)
        .add_attribute("denom", denom))
}

fn execute_add_allowed_destination_address(
    deps: DepsMut,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    // Validate admin caller
    validate_admin_caller(&deps.as_ref(), &info)?;

    // Normalize and validate address
    let normalized_address = normalize_evm_address(&address)?;

    // Check for duplicate
    if ALLOWED_DESTINATION_ADDRESSES.has(deps.storage, normalized_address.clone()) {
        return Err(ContractError::DestinationAddressAlreadyExists {
            address: normalized_address,
        });
    }

    ALLOWED_DESTINATION_ADDRESSES.save(deps.storage, normalized_address.clone(), &())?;

    Ok(Response::new()
        .add_attribute("action", "add_allowed_destination_address")
        .add_attribute("sender", info.sender)
        .add_attribute("address", normalized_address))
}

fn execute_remove_allowed_destination_address(
    deps: DepsMut,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    // Validate admin caller
    validate_admin_caller(&deps.as_ref(), &info)?;

    // Normalize address
    let normalized_address = normalize_evm_address(&address)?;

    if !ALLOWED_DESTINATION_ADDRESSES.has(deps.storage, normalized_address.clone()) {
        return Err(ContractError::DestinationAddressDoesNotExist {
            address: normalized_address,
        });
    }

    ALLOWED_DESTINATION_ADDRESSES.remove(deps.storage, normalized_address.clone());

    Ok(Response::new()
        .add_attribute("action", "remove_allowed_destination_address")
        .add_attribute("sender", info.sender)
        .add_attribute("address", normalized_address))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::StandardQuery(interface_msg) => dispatch_query_standard(deps, env, interface_msg),
        QueryMsg::CustomQuery(custom_msg) => dispatch_query_custom(deps, custom_msg),
    }
}

/// Dispatch standard adapter interface queries
fn dispatch_query_standard(
    deps: Deps,
    env: Env,
    msg: AdapterInterfaceQueryMsg,
) -> StdResult<Binary> {
    match msg {
        AdapterInterfaceQueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        AdapterInterfaceQueryMsg::AvailableForDeposit {
            depositor_address,
            denom,
        } => to_json_binary(&query_available_for_deposit(
            deps,
            depositor_address,
            denom,
        )?),
        AdapterInterfaceQueryMsg::AvailableForWithdraw {
            depositor_address,
            denom,
        } => to_json_binary(&query_available_for_withdraw(
            deps,
            env,
            depositor_address,
            denom,
        )?),
        AdapterInterfaceQueryMsg::TimeToWithdraw {
            depositor_address: _,
            coin: _,
        } => to_json_binary(&query_time_to_withdraw()?),
        AdapterInterfaceQueryMsg::AllPositions {} => to_json_binary(&query_all_positions()?),
        AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address: _,
            denom: _,
        } => to_json_binary(&query_depositor_position()?),
        AdapterInterfaceQueryMsg::DepositorPositions {
            depositor_address: _,
        } => to_json_binary(&query_depositor_positions()?),
        AdapterInterfaceQueryMsg::RegisteredDepositors { enabled } => {
            to_json_binary(&query_registered_depositors(deps, enabled)?)
        }
        AdapterInterfaceQueryMsg::Admins {} => to_json_binary(&query_admins(deps)?),
    }
}

/// Dispatch IBC Eureka adapter-specific custom queries
fn dispatch_query_custom(deps: Deps, msg: IbcEurekaAdapterQueryMsg) -> StdResult<Binary> {
    match msg {
        IbcEurekaAdapterQueryMsg::AllowedDenoms {} => to_json_binary(&query_allowed_denoms(deps)?),
        IbcEurekaAdapterQueryMsg::Executors {} => to_json_binary(&query_executors(deps)?),
        IbcEurekaAdapterQueryMsg::DepositorCapabilities { depositor_address } => {
            to_json_binary(&query_depositor_capabilities(deps, depositor_address)?)
        }
        IbcEurekaAdapterQueryMsg::AllowedDestinationAddresses { start_after, limit } => {
            to_json_binary(&query_allowed_destination_addresses(
                deps,
                start_after,
                limit,
            )?)
        }
    }
}

/// Query adapter config
fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

fn query_available_for_deposit(
    deps: Deps,
    depositor_address: String,
    denom: String,
) -> StdResult<AvailableAmountResponse> {
    let depositor_addr = deps.api.addr_validate(&depositor_address)?;
    let depositor = WHITELISTED_DEPOSITORS.may_load(deps.storage, depositor_addr)?;
    let denom_allowed = ALLOWED_DENOMS.has(deps.storage, denom);

    let amount = match depositor {
        Some(d) if d.enabled && denom_allowed => Uint128::MAX,
        _ => Uint128::zero(),
    };

    Ok(AvailableAmountResponse { amount })
}

/// Query available amount for withdrawal (adapter balance)
fn query_available_for_withdraw(
    deps: Deps,
    env: Env,
    depositor_address: String,
    denom: String,
) -> StdResult<AvailableAmountResponse> {
    let depositor_address = deps.api.addr_validate(&depositor_address)?;
    let depositor = WHITELISTED_DEPOSITORS.may_load(deps.storage, depositor_address)?;
    let denom_allowed = ALLOWED_DENOMS.has(deps.storage, denom.clone());

    let amount = match depositor {
        Some(d) if d.enabled && d.capabilities.can_withdraw && denom_allowed => {
            deps.querier
                .query_balance(env.contract.address, denom)?
                .amount
        }
        _ => Uint128::zero(),
    };

    Ok(AvailableAmountResponse { amount })
}

/// Query time to withdraw
fn query_time_to_withdraw() -> StdResult<TimeEstimateResponse> {
    Ok(TimeEstimateResponse {
        blocks: 0,
        seconds: 0,
    })
}

/// Query all positions (returns empty for balance-based tracking)
fn query_all_positions() -> StdResult<AllPositionsResponse> {
    Ok(AllPositionsResponse { positions: vec![] })
}

/// Query depositor position (returns zero for balance-based tracking)
fn query_depositor_position() -> StdResult<DepositorPositionResponse> {
    Ok(DepositorPositionResponse {
        amount: Uint128::zero(),
    })
}

/// Query depositor positions (returns empty for balance-based tracking)
fn query_depositor_positions() -> StdResult<DepositorPositionsResponse> {
    Ok(DepositorPositionsResponse { positions: vec![] })
}

/// Query registered depositors
fn query_registered_depositors(
    deps: Deps,
    enabled: Option<bool>,
) -> StdResult<RegisteredDepositorsResponse> {
    let depositors: Vec<RegisteredDepositorInfo> = WHITELISTED_DEPOSITORS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .filter_map(|item| {
            item.ok().and_then(|(addr, depositor)| {
                // Filter by enabled status if specified
                if let Some(filter_enabled) = enabled {
                    if depositor.enabled != filter_enabled {
                        return None;
                    }
                }

                Some(RegisteredDepositorInfo {
                    depositor_address: addr.to_string(),
                    enabled: depositor.enabled,
                })
            })
        })
        .collect();

    Ok(RegisteredDepositorsResponse { depositors })
}

fn query_allowed_denoms(deps: Deps) -> StdResult<AllowedDenomsResponse> {
    let denoms: Vec<String> = ALLOWED_DENOMS
        .keys(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    Ok(AllowedDenomsResponse { denoms })
}

fn query_executors(deps: Deps) -> StdResult<ExecutorsResponse> {
    let executors: StdResult<Vec<ExecutorInfo>> = EXECUTORS
        .keys(deps.storage, None, None, Order::Ascending)
        .map(|address_res| {
            Ok(ExecutorInfo {
                executor_address: address_res?.to_string(),
            })
        })
        .collect();

    Ok(ExecutorsResponse {
        executors: executors?,
    })
}

fn query_admins(deps: Deps) -> StdResult<AdminsResponse> {
    let admins = ADMINS.load(deps.storage)?;
    Ok(AdminsResponse {
        admins: admins.into_iter().map(|a| a.to_string()).collect(),
    })
}

fn query_depositor_capabilities(
    deps: Deps,
    depositor_address: String,
) -> StdResult<DepositorCapabilitiesResponse> {
    let depositor = get_depositor(deps, depositor_address)?;
    Ok(DepositorCapabilitiesResponse {
        capabilities: depositor.capabilities,
    })
}

fn query_allowed_destination_addresses(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<AllowedDestinationAddressesResponse> {
    let limit = limit
        .unwrap_or(DEFAULT_PAGINATION_LIMIT)
        .min(MAX_PAGINATION_LIMIT) as usize;

    let start_bound = start_after
        .map(|addr| -> StdResult<String> {
            normalize_evm_address(&addr).map_err(|e| StdError::generic_err(e.to_string()))
        })
        .transpose()?
        .map(Bound::exclusive);

    let addresses: Vec<String> = ALLOWED_DESTINATION_ADDRESSES
        .keys(deps.storage, start_bound, None, Order::Ascending)
        .take(limit)
        .collect::<StdResult<Vec<_>>>()?;

    Ok(AllowedDestinationAddressesResponse { addresses })
}
