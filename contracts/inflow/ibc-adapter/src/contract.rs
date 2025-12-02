use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Response,
    StdResult, Uint128,
};
use cw2::set_contract_version;
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::error::ContractError;
use crate::ibc::{calculate_timeout, create_ibc_transfer_msg};
use crate::msg::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AllChainsResponse, AllPositionsResponse,
    AllTokensResponse, AvailableAmountResponse, ChainConfigResponse, DepositorCapabilitiesResponse,
    DepositorPositionResponse, DepositorPositionsResponse, ExecuteMsg, ExecutorsResponse,
    IbcAdapterMsg, IbcAdapterQueryMsg, IbcConfigResponse, InstantiateMsg, QueryMsg,
    RegisteredDepositorInfo, RegisteredDepositorsResponse, TimeEstimateResponse,
    TokenConfigResponse,
};
use crate::state::{
    ChainConfig, Config, Depositor, TokenConfig, TransferFundsInstructions, ADMINS, CHAIN_REGISTRY,
    CONFIG, EXECUTORS, TOKEN_REGISTRY, WHITELISTED_DEPOSITORS,
};
use crate::validation::{
    get_depositor, validate_admin_caller, validate_admin_or_executor, validate_capabilities_binary,
    validate_config_admin, validate_depositor_caller, validate_recipient_for_chain,
};

const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// ========== INSTANTIATE ==========

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

    // Validate and store config admins
    let validated_admins = msg
        .admins
        .iter()
        .map(|a| deps.api.addr_validate(a))
        .collect::<StdResult<Vec<_>>>()?;
    ADMINS.save(deps.storage, &validated_admins)?;

    // Validate and store executors
    let validated_executors = if let Some(exec_list) = msg.executors {
        exec_list
            .iter()
            .map(|e| deps.api.addr_validate(e))
            .collect::<StdResult<Vec<_>>>()?
    } else {
        vec![]
    };
    EXECUTORS.save(deps.storage, &validated_executors)?;

    // Store config
    let config = Config {
        default_timeout_seconds: msg.default_timeout_seconds,
    };
    CONFIG.save(deps.storage, &config)?;

    // Initialize chain registry with initial chains if provided
    let mut chains_count = 0;
    if let Some(initial_chains) = msg.initial_chains {
        for (chain_id, chain_config) in initial_chains {
            CHAIN_REGISTRY.save(deps.storage, chain_id, &chain_config)?;
            chains_count += 1;
        }
    }

    // Initialize token registry with initial tokens if provided
    let mut tokens_count = 0;
    if let Some(initial_tokens) = msg.initial_tokens {
        for (denom, source_chain_id) in initial_tokens {
            let token_config = TokenConfig {
                denom: denom.clone(),
                source_chain_id,
            };
            TOKEN_REGISTRY.save(deps.storage, denom, &token_config)?;
            tokens_count += 1;
        }
    }

    // Optionally register initial depositor
    let mut depositor_registered = false;
    if let Some(depositor_addr) = msg.depositor_address {
        let addr = deps.api.addr_validate(&depositor_addr)?;

        // Parse capabilities or use default (simplified: only can_withdraw field)
        let capabilities = if let Some(cap_binary) = msg.depositor_capabilities {
            validate_capabilities_binary(&cap_binary)?
        } else {
            // Default capabilities: can withdraw
            crate::state::DepositorCapabilities { can_withdraw: true }
        };

        let depositor = Depositor {
            enabled: true,
            capabilities,
        };

        WHITELISTED_DEPOSITORS.save(deps.storage, addr, &depositor)?;
        depositor_registered = true;
    }

    let mut response = Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("contract_name", CONTRACT_NAME)
        .add_attribute("contract_version", CONTRACT_VERSION)
        .add_attribute("admin_count", validated_admins.len().to_string())
        .add_attribute("executor_count", validated_executors.len().to_string())
        .add_attribute(
            "default_timeout_seconds",
            config.default_timeout_seconds.to_string(),
        );

    // Add initial chains count
    if chains_count > 0 {
        response = response.add_attribute("initial_chains_count", chains_count.to_string());
    }

    // Add initial tokens count
    if tokens_count > 0 {
        response = response.add_attribute("initial_tokens_count", tokens_count.to_string());
    }

    // Add depositor if registered
    if depositor_registered {
        response = response.add_attribute("depositor_registered", "true");
    }

    Ok(response)
}

// ========== EXECUTE ==========

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        ExecuteMsg::Interface(interface_msg) => {
            dispatch_execute_interface(deps, env, info, interface_msg)
        }
        ExecuteMsg::Custom(custom_msg) => dispatch_execute_custom(deps, env, info, custom_msg),
    }
}

/// Dispatch standard adapter interface messages
fn dispatch_execute_interface(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: AdapterInterfaceMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        AdapterInterfaceMsg::Deposit {} => execute_deposit(deps, env, info),
        AdapterInterfaceMsg::Withdraw { coin } => execute_withdraw(deps, env, info, coin),
        AdapterInterfaceMsg::RegisterDepositor {
            depositor_address,
            metadata,
        } => execute_register_depositor(deps, info, depositor_address, metadata),
        AdapterInterfaceMsg::UnregisterDepositor { depositor_address } => {
            execute_unregister_depositor(deps, info, depositor_address)
        }
        AdapterInterfaceMsg::ToggleDepositorEnabled {
            depositor_address,
            enabled,
        } => execute_toggle_depositor_enabled(deps, info, depositor_address, enabled),
    }
}

/// Dispatch IBC adapter-specific custom messages
fn dispatch_execute_custom(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: IbcAdapterMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        // EXECUTOR OR CONFIG ADMIN - TransferFunds
        IbcAdapterMsg::TransferFunds { coin, instructions } => {
            execute_transfer_funds(deps, env, info, coin, instructions)
        }

        // CONFIG ADMIN ONLY - Executor Management
        IbcAdapterMsg::AddExecutor { executor_address } => {
            validate_config_admin(&deps, &info)?;
            execute_add_executor(deps, info, executor_address)
        }
        IbcAdapterMsg::RemoveExecutor { executor_address } => {
            validate_config_admin(&deps, &info)?;
            execute_remove_executor(deps, info, executor_address)
        }

        // CONFIG ADMIN ONLY - Chain/Token Management
        IbcAdapterMsg::RegisterChain {
            chain_id,
            channel_from_neutron,
            allowed_recipients,
        } => {
            validate_config_admin(&deps, &info)?;
            execute_register_chain(
                deps,
                info,
                chain_id,
                channel_from_neutron,
                allowed_recipients,
            )
        }
        IbcAdapterMsg::UnregisterChain { chain_id } => {
            validate_config_admin(&deps, &info)?;
            execute_unregister_chain(deps, info, chain_id)
        }
        IbcAdapterMsg::RegisterToken {
            denom,
            source_chain_id,
        } => {
            validate_config_admin(&deps, &info)?;
            execute_register_token(deps, info, denom, source_chain_id)
        }
        IbcAdapterMsg::UnregisterToken { denom } => {
            validate_config_admin(&deps, &info)?;
            execute_unregister_token(deps, info, denom)
        }
        IbcAdapterMsg::UpdateConfig {
            default_timeout_seconds,
        } => {
            validate_config_admin(&deps, &info)?;
            execute_update_config(deps, info, default_timeout_seconds)
        }
    }
}

// ========== EXECUTE HANDLERS ==========

/// Handle deposit - just holds the funds in the adapter
fn execute_deposit(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
) -> Result<Response<NeutronMsg>, ContractError> {
    // 1. Validate depositor
    validate_depositor_caller(&deps, &info)?;

    // 2. Validate exactly one coin sent
    if info.funds.len() != 1 {
        return Err(ContractError::InvalidFunds {
            count: info.funds.len(),
        });
    }
    let coin = &info.funds[0];

    // 3. Validate non-zero amount
    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // 4. Validate token is registered
    TOKEN_REGISTRY
        .may_load(deps.storage, coin.denom.clone())?
        .ok_or(ContractError::TokenNotRegistered {
            denom: coin.denom.clone(),
        })?;

    // 5. Funds are now held in the adapter contract
    // Admin will route them via TransferFunds

    Ok(Response::new()
        .add_attribute("action", "deposit")
        .add_attribute("depositor", info.sender)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", &coin.denom))
}

/// Handle IBC transfer of funds (admin or executor)
fn execute_transfer_funds(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    coin: Coin,
    instructions: TransferFundsInstructions,
) -> Result<Response<NeutronMsg>, ContractError> {
    // 1. Validate caller is admin or executor
    validate_admin_or_executor(&deps, &info)?;

    // 2. Validate non-zero amount
    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // 3. Validate token is registered
    TOKEN_REGISTRY
        .may_load(deps.storage, coin.denom.clone())?
        .ok_or(ContractError::TokenNotRegistered {
            denom: coin.denom.clone(),
        })?;

    // 4. Load chain configuration
    let chain_config = CHAIN_REGISTRY
        .may_load(deps.storage, instructions.destination_chain.clone())?
        .ok_or(ContractError::ChainNotRegistered {
            chain_id: instructions.destination_chain.clone(),
        })?;

    // 5. Validate recipient against chain-level restrictions
    validate_recipient_for_chain(&chain_config, &instructions.recipient)?;

    // 6. Query adapter balance
    let balance = deps
        .querier
        .query_balance(env.contract.address.clone(), coin.denom.clone())?;

    // 7. Verify sufficient balance
    if balance.amount < coin.amount {
        return Err(ContractError::InsufficientBalance {});
    }

    // 8. Load config for timeout
    let config = CONFIG.load(deps.storage)?;

    // 9. Calculate timeout
    let timeout = calculate_timeout(&env, &config, &instructions);

    // 10. Create IBC transfer message
    let ibc_msg = create_ibc_transfer_msg(
        deps.as_ref(),
        &env,
        &chain_config,
        coin.clone(),
        instructions.recipient.clone(),
        timeout,
    )?;

    // 11. Return response with IBC message
    Ok(Response::new()
        .add_message(ibc_msg)
        .add_attribute("action", "transfer_funds")
        .add_attribute("caller", info.sender)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", &coin.denom)
        .add_attribute("destination_chain", instructions.destination_chain)
        .add_attribute("recipient", instructions.recipient)
        .add_attribute("channel", chain_config.channel_from_neutron))
}

/// Handle withdraw from adapter balance
fn execute_withdraw(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    coin: Coin,
) -> Result<Response<NeutronMsg>, ContractError> {
    // 1. Validate depositor
    let depositor = validate_depositor_caller(&deps, &info)?;

    // 2. Check can_withdraw capability
    if !depositor.capabilities.can_withdraw {
        return Err(ContractError::WithdrawalNotAllowed {});
    }

    // 3. Validate non-zero amount
    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // 4. Validate token is registered
    TOKEN_REGISTRY
        .may_load(deps.storage, coin.denom.clone())?
        .ok_or(ContractError::TokenNotRegistered {
            denom: coin.denom.clone(),
        })?;

    // 5. Query adapter balance
    let balance = deps
        .querier
        .query_balance(env.contract.address, coin.denom.clone())?;

    // 6. Verify sufficient balance
    if balance.amount < coin.amount {
        return Err(ContractError::InsufficientBalance {});
    }

    // 7. Send funds back to depositor
    let bank_msg = BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![coin.clone()],
    };

    Ok(Response::new()
        .add_message(bank_msg)
        .add_attribute("action", "withdraw")
        .add_attribute("depositor", info.sender)
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

    let addr = deps.api.addr_validate(&depositor_address)?;

    // Check if depositor already registered
    if WHITELISTED_DEPOSITORS.has(deps.storage, addr.clone()) {
        return Err(ContractError::DepositorAlreadyRegistered {
            depositor_address: depositor_address.clone(),
        });
    }

    // Parse capabilities or use default (simplified: only can_withdraw field)
    let capabilities = if let Some(cap_binary) = metadata {
        validate_capabilities_binary(&cap_binary)?
    } else {
        // Default capabilities: can withdraw
        crate::state::DepositorCapabilities { can_withdraw: true }
    };

    let depositor = Depositor {
        enabled: true,
        capabilities,
    };

    WHITELISTED_DEPOSITORS.save(deps.storage, addr, &depositor)?;

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

    let addr = deps.api.addr_validate(&depositor_address)?;

    // Remove depositor
    WHITELISTED_DEPOSITORS.remove(deps.storage, addr);

    Ok(Response::new()
        .add_attribute("action", "unregister_depositor")
        .add_attribute("depositor_address", depositor_address))
}

/// Toggle depositor enabled status
fn execute_toggle_depositor_enabled(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    depositor_address: String,
    enabled: bool,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate caller is admin
    validate_admin_caller(&deps.as_ref(), &info)?;

    let addr = deps.api.addr_validate(&depositor_address)?;

    // Load and update depositor
    let mut depositor = WHITELISTED_DEPOSITORS
        .may_load(deps.storage, addr.clone())?
        .ok_or(ContractError::DepositorNotRegistered {
            depositor_address: depositor_address.clone(),
        })?;

    depositor.enabled = enabled;
    WHITELISTED_DEPOSITORS.save(deps.storage, addr, &depositor)?;

    Ok(Response::new()
        .add_attribute("action", "toggle_depositor_enabled")
        .add_attribute("depositor_address", depositor_address)
        .add_attribute("enabled", enabled.to_string()))
}

// ========== QUERY ==========

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<NeutronQuery>, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Interface(interface_msg) => dispatch_query_interface(deps, env, interface_msg),
        QueryMsg::Custom(custom_msg) => dispatch_query_custom(deps, custom_msg),
    }
}

/// Dispatch standard adapter interface queries
fn dispatch_query_interface(
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

/// Dispatch IBC adapter-specific custom queries
fn dispatch_query_custom(deps: Deps<NeutronQuery>, msg: IbcAdapterQueryMsg) -> StdResult<Binary> {
    match msg {
        IbcAdapterQueryMsg::ChainConfig { chain_id } => {
            to_json_binary(&query_chain_config(deps, chain_id)?)
        }
        IbcAdapterQueryMsg::AllChains {} => to_json_binary(&query_all_chains(deps)?),
        IbcAdapterQueryMsg::TokenConfig { denom } => {
            to_json_binary(&query_token_config(deps, denom)?)
        }
        IbcAdapterQueryMsg::AllTokens {} => to_json_binary(&query_all_tokens(deps)?),
        IbcAdapterQueryMsg::Executors {} => to_json_binary(&query_executors(deps)?),
        IbcAdapterQueryMsg::DepositorCapabilities { depositor_address } => {
            to_json_binary(&query_depositor_capabilities(deps, depositor_address)?)
        }
    }
}

// ========== QUERY HANDLERS ==========

/// Query adapter config
fn query_config(deps: Deps<NeutronQuery>) -> StdResult<IbcConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    let admins = ADMINS.load(deps.storage)?;

    Ok(IbcConfigResponse {
        admins: admins.iter().map(|a| a.to_string()).collect(),
        default_timeout_seconds: config.default_timeout_seconds,
    })
}

/// Query available amount for deposit (no cap for IBC)
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
    let balance = deps.querier.query_balance(env.contract.address, denom)?;
    Ok(AvailableAmountResponse {
        amount: balance.amount,
    })
}

/// Query time to withdraw (instant for local transfers)
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

// ========== IBC ADAPTER CUSTOM EXECUTE HANDLERS ==========

fn execute_add_executor(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    executor_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Already validated config admin in dispatch

    let executor_addr = deps.api.addr_validate(&executor_address)?;

    let mut executors = EXECUTORS.load(deps.storage)?;

    // Check not already in list
    if executors.contains(&executor_addr) {
        return Err(ContractError::ExecutorAlreadyExists {
            executor: executor_address,
        });
    }

    executors.push(executor_addr.clone());
    EXECUTORS.save(deps.storage, &executors)?;

    Ok(Response::new()
        .add_attribute("action", "add_executor")
        .add_attribute("executor", executor_addr)
        .add_attribute("added_by", info.sender))
}

fn execute_remove_executor(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    executor_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Already validated config admin in dispatch

    let executor_addr = deps.api.addr_validate(&executor_address)?;

    let mut executors = EXECUTORS.load(deps.storage)?;

    // Find and remove
    let original_len = executors.len();
    executors.retain(|e| e != executor_addr);

    if executors.len() == original_len {
        return Err(ContractError::ExecutorNotFound {
            executor: executor_address,
        });
    }

    EXECUTORS.save(deps.storage, &executors)?;

    Ok(Response::new()
        .add_attribute("action", "remove_executor")
        .add_attribute("executor", executor_addr)
        .add_attribute("removed_by", info.sender))
}

fn execute_register_chain(
    deps: DepsMut<NeutronQuery>,
    _info: MessageInfo,
    chain_id: String,
    channel_from_neutron: String,
    allowed_recipients: Vec<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let chain_config = ChainConfig {
        chain_id: chain_id.clone(),
        channel_from_neutron,
        allowed_recipients,
    };

    CHAIN_REGISTRY.save(deps.storage, chain_id.clone(), &chain_config)?;

    Ok(Response::new()
        .add_attribute("action", "register_chain")
        .add_attribute("chain_id", chain_id))
}

fn execute_unregister_chain(
    deps: DepsMut<NeutronQuery>,
    _info: MessageInfo,
    chain_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    CHAIN_REGISTRY.remove(deps.storage, chain_id.clone());

    Ok(Response::new()
        .add_attribute("action", "unregister_chain")
        .add_attribute("chain_id", chain_id))
}

fn execute_register_token(
    deps: DepsMut<NeutronQuery>,
    _info: MessageInfo,
    denom: String,
    source_chain_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let token_config = TokenConfig {
        denom: denom.clone(),
        source_chain_id,
    };

    TOKEN_REGISTRY.save(deps.storage, denom.clone(), &token_config)?;

    Ok(Response::new()
        .add_attribute("action", "register_token")
        .add_attribute("denom", denom))
}

fn execute_unregister_token(
    deps: DepsMut<NeutronQuery>,
    _info: MessageInfo,
    denom: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    TOKEN_REGISTRY.remove(deps.storage, denom.clone());

    Ok(Response::new()
        .add_attribute("action", "unregister_token")
        .add_attribute("denom", denom))
}

fn execute_update_config(
    deps: DepsMut<NeutronQuery>,
    _info: MessageInfo,
    default_timeout_seconds: u64,
) -> Result<Response<NeutronMsg>, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    config.default_timeout_seconds = default_timeout_seconds;
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("action", "update_config")
        .add_attribute(
            "default_timeout_seconds",
            default_timeout_seconds.to_string(),
        ))
}

// ========== IBC ADAPTER CUSTOM QUERY HANDLERS ==========

fn query_chain_config(
    deps: Deps<NeutronQuery>,
    chain_id: String,
) -> StdResult<ChainConfigResponse> {
    let chain_config = CHAIN_REGISTRY.load(deps.storage, chain_id)?;
    Ok(ChainConfigResponse { chain_config })
}

fn query_all_chains(deps: Deps<NeutronQuery>) -> StdResult<AllChainsResponse> {
    let chains: Vec<ChainConfig> = CHAIN_REGISTRY
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| item.map(|(_, config)| config))
        .collect::<StdResult<Vec<_>>>()?;

    Ok(AllChainsResponse { chains })
}

fn query_token_config(deps: Deps<NeutronQuery>, denom: String) -> StdResult<TokenConfigResponse> {
    let token_config = TOKEN_REGISTRY.load(deps.storage, denom)?;
    Ok(TokenConfigResponse { token_config })
}

fn query_all_tokens(deps: Deps<NeutronQuery>) -> StdResult<AllTokensResponse> {
    let tokens: Vec<TokenConfig> = TOKEN_REGISTRY
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| item.map(|(_, config)| config))
        .collect::<StdResult<Vec<_>>>()?;

    Ok(AllTokensResponse { tokens })
}

fn query_executors(deps: Deps<NeutronQuery>) -> StdResult<ExecutorsResponse> {
    let executors = EXECUTORS.load(deps.storage)?;
    let executors_strings: Vec<String> = executors.iter().map(|e| e.to_string()).collect();

    Ok(ExecutorsResponse {
        executors: executors_strings,
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
