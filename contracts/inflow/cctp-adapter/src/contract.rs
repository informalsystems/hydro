use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Order,
    Response, StdError, StdResult, Uint128,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;
use interface::utils::{DEFAULT_PAGINATION_LIMIT, MAX_PAGINATION_LIMIT};
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::error::ContractError;
use crate::ibc::create_ibc_transfer_msg;
use crate::msg::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AdminsResponse, AllChainsResponse,
    AllPositionsResponse, AllowedDestinationAddressesResponse, AvailableAmountResponse,
    CctpAdapterMsg, CctpAdapterQueryMsg, ChainConfigResponse, ConfigResponse,
    DepositorCapabilitiesResponse, DepositorPositionResponse, DepositorPositionsResponse,
    ExecuteMsg, ExecutorInfo, ExecutorsResponse, InstantiateMsg, QueryMsg, RegisteredDepositorInfo,
    RegisteredDepositorsResponse, TimeEstimateResponse,
};
use crate::noble::construct_noble_cctp_memo;
use crate::state::{
    ChainConfig, Config, Depositor, DepositorCapabilities, DestinationAddress,
    TransferFundsInstructions, ADMINS, ALLOWED_DESTINATION_ADDRESSES, CHAIN_REGISTRY, CONFIG,
    EXECUTORS, WHITELISTED_DEPOSITORS,
};
use crate::validation::{
    get_depositor, get_destination_address, normalize_evm_address, validate_admin_caller,
    validate_depositor_caller, validate_executor_caller,
};

const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // Validate at least one admin
    if msg.admins.is_empty() {
        return Err(ContractError::AtLeastOneAdmin {});
    }

    // Validate and deduplicate admins
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

    // Validate and store executors
    for initial_executor in msg.initial_executors {
        let addr = deps.api.addr_validate(&initial_executor.address)?;

        // Check for duplicate
        if EXECUTORS.has(deps.storage, addr.clone()) {
            return Err(ContractError::ExecutorAlreadyExists {
                executor: addr.to_string(),
            });
        }

        EXECUTORS.save(deps.storage, addr, &())?;
    }

    // Register initial depositors
    for initial_depositor in msg.initial_depositors {
        let addr = deps.api.addr_validate(&initial_depositor.address)?;

        // Check for duplicate
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

    // Initialize chain registry and destination addresses from initial chains
    for mut initial_chain in msg.initial_chains {
        // Validate and normalize chain config data first
        initial_chain.chain_config = initial_chain.chain_config.validate_and_normalize()?;

        let chain_id = initial_chain.chain_config.chain_id.clone();
        if CHAIN_REGISTRY.has(deps.storage, chain_id.clone()) {
            return Err(ContractError::ChainAlreadyRegistered {
                chain_id: chain_id.clone(),
            });
        }

        // Save chain config to registry
        CHAIN_REGISTRY.save(deps.storage, chain_id.clone(), &initial_chain.chain_config)?;

        // Register destination addresses for this chain
        for dest_addr in initial_chain.initial_allowed_destination_addresses {
            // Normalize and validate address
            let normalized_address = normalize_evm_address(&dest_addr.address)?;

            // Check for duplicate
            let key = (chain_id.clone(), normalized_address.clone());
            if ALLOWED_DESTINATION_ADDRESSES.has(deps.storage, key.clone()) {
                return Err(ContractError::DestinationAddressAlreadyExists {
                    chain_id: chain_id.clone(),
                    address: normalized_address,
                });
            }

            // Save allowed destination address in normalized form
            let normalized_dest_addr = DestinationAddress {
                address: normalized_address,
                protocol: dest_addr.protocol,
            };
            ALLOWED_DESTINATION_ADDRESSES.save(deps.storage, key, &normalized_dest_addr)?;
        }
    }

    if msg.denom.trim().is_empty() {
        return Err(StdError::generic_err("Denom cannot be empty").into());
    }

    // Store config with the token denom and Noble IBC settings
    let config = Config {
        denom: msg.denom.clone(),
        noble_transfer_channel_id: msg.noble_transfer_channel_id.clone(),
        ibc_default_timeout_seconds: msg.ibc_default_timeout_seconds,
    };
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("contract_name", CONTRACT_NAME)
        .add_attribute("contract_version", CONTRACT_VERSION)
        .add_attribute("denom", msg.denom)
        .add_attribute("noble_transfer_channel_id", msg.noble_transfer_channel_id)
        .add_attribute(
            "ibc_default_timeout_seconds",
            msg.ibc_default_timeout_seconds.to_string(),
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
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: AdapterInterfaceMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
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
    }
}

/// Dispatch CCTP adapter-specific custom messages
fn dispatch_execute_custom(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: CctpAdapterMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        CctpAdapterMsg::TransferFunds {
            amount,
            instructions,
        } => execute_transfer_funds(deps, env, info, amount, instructions),
        CctpAdapterMsg::AddExecutor { executor_address } => {
            execute_add_executor(deps, info, executor_address)
        }
        CctpAdapterMsg::RemoveExecutor { executor_address } => {
            execute_remove_executor(deps, info, executor_address)
        }
        CctpAdapterMsg::AddAdmin { admin_address } => execute_add_admin(deps, info, admin_address),
        CctpAdapterMsg::RemoveAdmin { admin_address } => {
            execute_remove_admin(deps, info, admin_address)
        }
        CctpAdapterMsg::RegisterChain { chain_config } => {
            execute_register_chain(deps, info, chain_config)
        }
        CctpAdapterMsg::UpdateRegisteredChain { chain_config } => {
            execute_update_registered_chain(deps, info, chain_config)
        }
        CctpAdapterMsg::UnregisterChain { chain_id } => {
            execute_unregister_chain(deps, info, chain_id)
        }
        CctpAdapterMsg::AddAllowedDestinationAddress {
            chain_id,
            address,
            protocol,
        } => execute_add_allowed_destination_address(deps, info, chain_id, address, protocol),
        CctpAdapterMsg::RemoveAllowedDestinationAddress { chain_id, address } => {
            execute_remove_allowed_destination_address(deps, info, chain_id, address)
        }
    }
}

/// Handle deposit - just holds the funds in the adapter
fn execute_deposit(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate depositor
    validate_depositor_caller(&deps, &info)?;

    // Load config and validate payment
    let config = CONFIG.load(deps.storage)?;
    let amount = cw_utils::must_pay(&info, &config.denom)?;

    Ok(Response::new()
        .add_attribute("action", "deposit")
        .add_attribute("sender", info.sender)
        .add_attribute("amount", amount)
        .add_attribute("denom", &config.denom))
}

/// Handle withdraw from adapter balance
fn execute_withdraw(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    coin: Coin,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate depositor
    let depositor = validate_depositor_caller(&deps, &info)?;
    if !depositor.capabilities.can_withdraw {
        return Err(ContractError::WithdrawalNotAllowed {});
    }

    // Validate non-zero amount
    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // Validate token denom matches config
    let config = CONFIG.load(deps.storage)?;
    if coin.denom != config.denom {
        return Err(ContractError::WrongTokenDenom {
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
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    depositor_address: String,
    metadata: Option<Binary>,
) -> Result<Response<NeutronMsg>, ContractError> {
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
        .add_attribute("depositor_address", depositor_address))
}

/// Unregister a depositor
fn execute_unregister_depositor(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    depositor_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
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
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    depositor_address: String,
    enabled: bool,
) -> Result<Response<NeutronMsg>, ContractError> {
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

/// Handle TransferFunds - initiate CCTP bridge to EVM chain via Noble
fn execute_transfer_funds(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
    instructions: TransferFundsInstructions,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate executor role
    validate_executor_caller(&deps, &info)?;

    if amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    let config = CONFIG.load(deps.storage)?;

    // Validate and extract fee info from info.funds
    // Executor must send exactly one coin (for the bridging fee)
    // Fee must be in the same denom as the adapter's USDC
    let bridging_fee_amount = cw_utils::must_pay(&info, &config.denom)?;

    let total_transfer_amount = amount + bridging_fee_amount;

    // Verify contract has sufficient USDC balance
    let balance = deps
        .querier
        .query_balance(env.contract.address.clone(), config.denom.clone())?;

    if balance.amount < total_transfer_amount {
        return Err(ContractError::InsufficientBalance {
            has: balance.amount,
            needs: total_transfer_amount,
        });
    }

    // Load chain configuration for destination EVM chain
    let chain_config = CHAIN_REGISTRY
        .may_load(deps.storage, instructions.chain_id.clone())?
        .ok_or(ContractError::ChainNotRegistered {
            chain_id: instructions.chain_id.clone(),
        })?;

    // Look up destination address from allowlist
    let destination_address = get_destination_address(
        &deps.as_ref(),
        &instructions.chain_id,
        &instructions.recipient,
    )?;

    // Construct Noble CCTP memo with fee and forwarding info
    let memo = construct_noble_cctp_memo(
        &chain_config.bridging_config,
        &destination_address.address,
        bridging_fee_amount,
    )?;

    // Create IBC transfer message to Noble chain
    // total token amount = amount (USDC to bridge) + fee_coin.amount (bridging fee for Skip)
    let total_token = Coin {
        denom: config.denom.clone(),
        amount: total_transfer_amount,
    };

    let ibc_msg = create_ibc_transfer_msg(
        deps.as_ref(),
        &env,
        &config,
        total_token,
        chain_config.bridging_config.noble_receiver.clone(),
        memo,
        config.ibc_default_timeout_seconds,
    )?;

    // Return response with IBC transfer message
    Ok(Response::new()
        .add_message(ibc_msg)
        .add_attribute("action", "transfer_funds")
        .add_attribute("sender", info.sender)
        .add_attribute("amount", amount)
        .add_attribute("bridging_fee", bridging_fee_amount)
        .add_attribute("total_transfer", total_transfer_amount.to_string())
        .add_attribute("chain_id", instructions.chain_id)
        .add_attribute("destination_address", destination_address.address)
        .add_attribute("protocol", destination_address.protocol)
        .add_attribute(
            "noble_receiver",
            chain_config.bridging_config.noble_receiver,
        ))
}

fn execute_add_executor(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    executor_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
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
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    executor_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
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
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    admin_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
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
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    admin_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
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

fn execute_register_chain(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    mut chain_config: ChainConfig,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    // Validate and normalize chain config data first
    chain_config = chain_config.validate_and_normalize()?;
    let chain_id = chain_config.chain_id.clone();

    // Check if chain already exists
    if CHAIN_REGISTRY.has(deps.storage, chain_id.clone()) {
        return Err(ContractError::ChainAlreadyRegistered {
            chain_id: chain_id.clone(),
        });
    }

    CHAIN_REGISTRY.save(deps.storage, chain_id.clone(), &chain_config)?;

    Ok(Response::new()
        .add_attribute("action", "register_chain")
        .add_attribute("sender", info.sender)
        .add_attribute("chain_id", chain_id))
}

fn execute_update_registered_chain(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    mut chain_config: ChainConfig,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    // Validate and normalize chain config data first
    chain_config = chain_config.validate_and_normalize()?;
    let chain_id = chain_config.chain_id.clone();

    // Check if chain with the given chain_id exists
    if !CHAIN_REGISTRY.has(deps.storage, chain_id.clone()) {
        return Err(ContractError::ChainNotRegistered {
            chain_id: chain_id.clone(),
        });
    }

    CHAIN_REGISTRY.save(deps.storage, chain_id.clone(), &chain_config)?;

    Ok(Response::new()
        .add_attribute("action", "update_registered_chain")
        .add_attribute("sender", info.sender)
        .add_attribute("chain_id", chain_id))
}

fn execute_unregister_chain(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    chain_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let chain_id = chain_id.trim().to_string();

    // Verify that the chain exists, error out if it doesn't
    if !CHAIN_REGISTRY.has(deps.storage, chain_id.clone()) {
        return Err(ContractError::ChainNotRegistered {
            chain_id: chain_id.clone(),
        });
    }

    // Remove all ALLOWED_DESTINATION_ADDRESSES related to the given chain_id
    let addresses_to_remove: Vec<String> = ALLOWED_DESTINATION_ADDRESSES
        .prefix(chain_id.clone())
        .keys(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    for address in addresses_to_remove {
        ALLOWED_DESTINATION_ADDRESSES.remove(deps.storage, (chain_id.clone(), address));
    }

    // Remove the chain from registry
    CHAIN_REGISTRY.remove(deps.storage, chain_id.clone());

    Ok(Response::new()
        .add_attribute("action", "unregister_chain")
        .add_attribute("sender", info.sender)
        .add_attribute("chain_id", chain_id))
}

fn execute_add_allowed_destination_address(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    chain_id: String,
    address: String,
    protocol: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate admin caller
    validate_admin_caller(&deps.as_ref(), &info)?;

    // Verify chain exists
    if !CHAIN_REGISTRY.has(deps.storage, chain_id.clone()) {
        return Err(ContractError::ChainNotRegistered {
            chain_id: chain_id.clone(),
        });
    }

    // Normalize and validate address
    let normalized_address = normalize_evm_address(&address)?;

    // Check for duplicate
    let key = (chain_id.clone(), normalized_address.clone());
    if ALLOWED_DESTINATION_ADDRESSES.has(deps.storage, key.clone()) {
        return Err(ContractError::DestinationAddressAlreadyExists {
            chain_id: chain_id.clone(),
            address: normalized_address,
        });
    }

    // Save destination address
    let dest_addr = DestinationAddress {
        address: normalized_address.clone(),
        protocol: protocol.clone(),
    };
    ALLOWED_DESTINATION_ADDRESSES.save(deps.storage, key, &dest_addr)?;

    Ok(Response::new()
        .add_attribute("action", "add_allowed_destination_address")
        .add_attribute("sender", info.sender)
        .add_attribute("chain_id", chain_id)
        .add_attribute("address", normalized_address)
        .add_attribute("protocol", protocol))
}

fn execute_remove_allowed_destination_address(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    chain_id: String,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate admin caller
    validate_admin_caller(&deps.as_ref(), &info)?;

    // Normalize address
    let normalized_address = normalize_evm_address(&address)?;

    // Remove from storage
    let key = (chain_id.clone(), normalized_address.clone());

    if !ALLOWED_DESTINATION_ADDRESSES.has(deps.storage, key.clone()) {
        return Err(ContractError::DestinationAddressDoesNotExist {
            chain_id: chain_id.clone(),
            address: normalized_address,
        });
    }

    ALLOWED_DESTINATION_ADDRESSES.remove(deps.storage, key);

    Ok(Response::new()
        .add_attribute("action", "remove_allowed_destination_address")
        .add_attribute("sender", info.sender)
        .add_attribute("chain_id", chain_id)
        .add_attribute("address", normalized_address))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<NeutronQuery>, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::StandardQuery(interface_msg) => dispatch_query_standard(deps, env, interface_msg),
        QueryMsg::CustomQuery(custom_msg) => dispatch_query_custom(deps, custom_msg),
    }
}

/// Dispatch standard adapter interface queries
fn dispatch_query_standard(
    deps: Deps<NeutronQuery>,
    env: Env,
    msg: AdapterInterfaceQueryMsg,
) -> StdResult<Binary> {
    match msg {
        AdapterInterfaceQueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        AdapterInterfaceQueryMsg::AvailableForDeposit {
            depositor_address: _,
            denom: _,
        } => to_json_binary(&query_available_for_deposit()?),
        AdapterInterfaceQueryMsg::AvailableForWithdraw {
            depositor_address: _,
            denom,
        } => to_json_binary(&query_available_for_withdraw(deps, env, denom)?),
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
    }
}

/// Dispatch CCTP adapter-specific custom queries
fn dispatch_query_custom(deps: Deps<NeutronQuery>, msg: CctpAdapterQueryMsg) -> StdResult<Binary> {
    match msg {
        CctpAdapterQueryMsg::ChainConfig { chain_id } => {
            to_json_binary(&query_chain_config(deps, chain_id)?)
        }
        CctpAdapterQueryMsg::AllChains {} => to_json_binary(&query_all_chains(deps)?),
        CctpAdapterQueryMsg::Executors {} => to_json_binary(&query_executors(deps)?),
        CctpAdapterQueryMsg::Admins {} => to_json_binary(&query_admins(deps)?),
        CctpAdapterQueryMsg::DepositorCapabilities { depositor_address } => {
            to_json_binary(&query_depositor_capabilities(deps, depositor_address)?)
        }
        CctpAdapterQueryMsg::AllowedDestinationAddresses {
            chain_id,
            start_after,
            limit,
        } => to_json_binary(&query_allowed_destination_addresses(
            deps,
            chain_id,
            start_after,
            limit,
        )?),
    }
}

/// Query adapter config
fn query_config(deps: Deps<NeutronQuery>) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

/// Query available amount for deposit (no cap for CCTP adapter)
fn query_available_for_deposit() -> StdResult<AvailableAmountResponse> {
    Ok(AvailableAmountResponse {
        amount: Uint128::MAX,
    })
}

/// Query available amount for withdrawal (adapter balance)
fn query_available_for_withdraw(
    deps: Deps<NeutronQuery>,
    env: Env,
    denom: String,
) -> StdResult<AvailableAmountResponse> {
    let config = CONFIG.load(deps.storage)?;
    if denom != config.denom {
        return Err(StdError::generic_err(format!(
            "Unsupported denom: {}. Expected: {}",
            denom, config.denom
        )));
    }

    let balance = deps.querier.query_balance(env.contract.address, denom)?;
    Ok(AvailableAmountResponse {
        amount: balance.amount,
    })
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
    deps: Deps<NeutronQuery>,
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

fn query_chain_config(
    deps: Deps<NeutronQuery>,
    chain_id: String,
) -> StdResult<ChainConfigResponse> {
    let chain_config = CHAIN_REGISTRY.load(deps.storage, chain_id)?;
    Ok(ChainConfigResponse { chain_config })
}

fn query_all_chains(deps: Deps<NeutronQuery>) -> StdResult<AllChainsResponse> {
    let chains: Vec<ChainConfig> = CHAIN_REGISTRY
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| item.map(|(_, config)| config))
        .collect::<StdResult<Vec<_>>>()?;

    Ok(AllChainsResponse { chains })
}

fn query_executors(deps: Deps<NeutronQuery>) -> StdResult<ExecutorsResponse> {
    let executors: StdResult<Vec<ExecutorInfo>> = EXECUTORS
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| {
            let (addr, _) = item?;
            Ok(ExecutorInfo {
                executor_address: addr.to_string(),
            })
        })
        .collect();

    Ok(ExecutorsResponse {
        executors: executors?,
    })
}

fn query_admins(deps: Deps<NeutronQuery>) -> StdResult<AdminsResponse> {
    let admins = ADMINS.load(deps.storage)?;
    Ok(AdminsResponse {
        admins: admins.into_iter().map(|a| a.to_string()).collect(),
    })
}

fn query_depositor_capabilities(
    deps: Deps<NeutronQuery>,
    depositor_address: String,
) -> StdResult<DepositorCapabilitiesResponse> {
    let depositor = get_depositor(deps, depositor_address)?;
    Ok(DepositorCapabilitiesResponse {
        capabilities: depositor.capabilities,
    })
}

fn query_allowed_destination_addresses(
    deps: Deps<NeutronQuery>,
    chain_id: String,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<AllowedDestinationAddressesResponse> {
    let limit = limit
        .unwrap_or(DEFAULT_PAGINATION_LIMIT)
        .min(MAX_PAGINATION_LIMIT) as usize;

    let start = start_after.map(|addr| -> Result<String, StdError> {
        let normalized =
            normalize_evm_address(&addr).map_err(|e| StdError::generic_err(e.to_string()))?;
        Ok(normalized)
    });

    let start_bound = match start {
        Some(Ok(s)) => Some(Bound::exclusive(s)),
        Some(Err(e)) => return Err(e),
        None => None,
    };

    let addresses: Vec<DestinationAddress> = ALLOWED_DESTINATION_ADDRESSES
        .prefix(chain_id)
        .range(deps.storage, start_bound, None, Order::Ascending)
        .take(limit)
        .map(|item| item.map(|(_, dest_addr)| dest_addr))
        .collect::<StdResult<Vec<_>>>()?;

    Ok(AllowedDestinationAddressesResponse { addresses })
}
