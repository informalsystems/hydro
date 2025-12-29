use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Order,
    Response, StdResult, Uint128,
};
use cw2::set_contract_version;
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::error::ContractError;
use crate::msg::*;
use crate::oracle;
use crate::skip;
use crate::state::*;
use crate::validation;

const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_SLIPPAGE_BPS: u64 = 1000; // 10%

// ========== INSTANTIATE ==========

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // Validate at least one config admin
    if msg.admins.is_empty() {
        return Err(ContractError::AtLeastOneAdmin {});
    }

    // Validate and store config admins
    let mut admins: Vec<_> = msg
        .admins
        .iter()
        .map(|addr| deps.api.addr_validate(addr))
        .collect::<StdResult<_>>()?;
    admins.sort();
    admins.dedup();
    ADMINS.save(deps.storage, &admins)?;

    // Validate and store executors
    let mut executors: Vec<_> = msg
        .executors
        .iter()
        .map(|addr| deps.api.addr_validate(addr))
        .collect::<StdResult<_>>()?;
    executors.sort();
    executors.dedup();
    EXECUTORS.save(deps.storage, &executors)?;

    // Validate and store skip_contract
    let skip_contract = deps.api.addr_validate(&msg.skip_contract)?;

    // Validate max_slippage_bps
    if msg.max_slippage_bps > MAX_SLIPPAGE_BPS {
        return Err(ContractError::InvalidSlippage {
            bps: msg.max_slippage_bps,
            max_bps: MAX_SLIPPAGE_BPS,
        });
    }

    // Store config
    let config = Config {
        skip_contract,
        default_timeout_nanos: msg.default_timeout_nanos,
        max_slippage_bps: msg.max_slippage_bps,
    };
    CONFIG.save(deps.storage, &config)?;

    // Register initial routes
    for (route_id, route_config) in msg.initial_routes {
        validation::validate_route_config(&route_config)?;
        ROUTE_REGISTRY.save(deps.storage, route_id, &route_config)?;
    }

    // Register initial recipients
    for (recipient_addr_str, recipient_config) in msg.initial_recipients {
        let recipient_addr = deps.api.addr_validate(&recipient_addr_str)?;
        RECIPIENT_REGISTRY.save(deps.storage, recipient_addr, &recipient_config)?;
    }

    // Register initial depositors
    for depositor_addr_str in msg.initial_depositors {
        let depositor_addr = deps.api.addr_validate(&depositor_addr_str)?;

        // Check for duplicate
        if WHITELISTED_DEPOSITORS.has(deps.storage, depositor_addr.clone()) {
            return Err(ContractError::DepositorAlreadyRegistered {
                depositor_address: depositor_addr.to_string(),
            });
        }

        let depositor = Depositor { enabled: true };
        WHITELISTED_DEPOSITORS.save(deps.storage, depositor_addr, &depositor)?;
    }

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("contract", CONTRACT_NAME)
        .add_attribute("version", CONTRACT_VERSION))
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
        ExecuteMsg::StandardAction(interface_msg) => {
            dispatch_execute_standard(deps, env, info, interface_msg)
        }
        ExecuteMsg::CustomAction(custom_msg) => {
            dispatch_execute_custom(deps, env, info, custom_msg)
        }
    }
}

fn dispatch_execute_standard(
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
            metadata: _,
        } => execute_register_depositor(deps, info, depositor_address),
        AdapterInterfaceMsg::UnregisterDepositor { depositor_address } => {
            execute_unregister_depositor(deps, info, depositor_address)
        }
        AdapterInterfaceMsg::SetDepositorEnabled {
            depositor_address,
            enabled,
        } => execute_set_depositor_enabled(deps, info, depositor_address, enabled),
    }
}

fn dispatch_execute_custom(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: SkipAdapterMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        SkipAdapterMsg::ExecuteSwap { params } => {
            validation::validate_admin_or_executor(&deps, &info)?;
            execute_swap(deps, env, info, params)
        }
        SkipAdapterMsg::AddExecutor { executor_address } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_add_executor(deps, executor_address)
        }
        SkipAdapterMsg::RemoveExecutor { executor_address } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_remove_executor(deps, executor_address)
        }
        SkipAdapterMsg::RegisterRoute {
            route_id,
            route_config,
        } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_register_route(deps, route_id, route_config)
        }
        SkipAdapterMsg::UnregisterRoute { route_id } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_unregister_route(deps, route_id)
        }
        SkipAdapterMsg::SetRouteEnabled { route_id, enabled } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_set_route_enabled(deps, route_id, enabled)
        }
        SkipAdapterMsg::RegisterRecipient {
            recipient_address,
            description,
        } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_register_recipient(deps, recipient_address, description)
        }
        SkipAdapterMsg::UnregisterRecipient { recipient_address } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_unregister_recipient(deps, recipient_address)
        }
        SkipAdapterMsg::SetRecipientEnabled {
            recipient_address,
            enabled,
        } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_set_recipient_enabled(deps, recipient_address, enabled)
        }
        SkipAdapterMsg::UpdateConfig {
            skip_contract,
            default_timeout_nanos,
            max_slippage_bps,
        } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_update_config(deps, skip_contract, default_timeout_nanos, max_slippage_bps)
        }
        SkipAdapterMsg::RegisterDenomSymbol {
            denom,
            symbol,
            description,
        } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_register_denom_symbol(deps, denom, symbol, description)
        }
        SkipAdapterMsg::UnregisterDenomSymbol { denom } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_unregister_denom_symbol(deps, denom)
        }
        SkipAdapterMsg::BulkRegisterDenomSymbols { mappings } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_bulk_register_denom_symbols(deps, mappings)
        }
        SkipAdapterMsg::ExecuteCrossChainSwap { params } => {
            validation::validate_admin_or_executor(&deps, &info)?;
            execute_cross_chain_swap(deps, env, info, params)
        }
        SkipAdapterMsg::RegisterToken {
            symbol,
            native_chain,
            native_denom,
            decimals,
        } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_register_token(deps, symbol, native_chain, native_denom, decimals)
        }
        SkipAdapterMsg::RegisterChain {
            chain_id,
            allowed_address,
        } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_register_chain(deps, chain_id, allowed_address)
        }
        SkipAdapterMsg::RegisterChannel {
            source_chain,
            dest_chain,
            channel_id,
        } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_register_channel(deps, source_chain, dest_chain, channel_id)
        }
        SkipAdapterMsg::UpdateOsmosisConfig {
            chain_id,
            skip_contract,
            swap_venue,
            ibc_adapter,
        } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_update_osmosis_config(deps, chain_id, skip_contract, swap_venue, ibc_adapter)
        }
        SkipAdapterMsg::RegisterCrossChainRoute {
            route_id,
            token_in,
            token_out,
            swap_chain,
            pool_id,
        } => {
            validation::validate_config_admin(&deps, &info)?;
            execute_register_cross_chain_route(deps, route_id, token_in, token_out, swap_chain, pool_id)
        }
    }
}

// ========== EXECUTE HANDLERS ==========

fn execute_deposit(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate depositor
    validation::validate_depositor_caller(&deps, &info)?;

    // Validate exactly one coin sent
    if info.funds.len() != 1 {
        return Err(ContractError::InvalidFunds {
            count: info.funds.len(),
        });
    }

    let coin = &info.funds[0];

    // Validate non-zero amount
    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // Funds held in adapter contract
    Ok(Response::new()
        .add_attribute("action", "deposit")
        .add_attribute("depositor", info.sender)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", &coin.denom))
}

fn execute_withdraw(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    coin: Coin,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate depositor
    validation::validate_depositor_caller(&deps, &info)?;

    // Validate non-zero amount
    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // Query adapter balance
    let balance = deps
        .querier
        .query_balance(env.contract.address, coin.denom.clone())?;

    // Verify sufficient balance
    if balance.amount < coin.amount {
        return Err(ContractError::InsufficientBalance {});
    }

    // Send funds back to depositor
    let send_msg = BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![coin.clone()],
    };

    Ok(Response::new()
        .add_message(send_msg)
        .add_attribute("action", "withdraw")
        .add_attribute("depositor", info.sender)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", coin.denom))
}

fn execute_swap(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    params: SwapExecutionParams,
) -> Result<Response<NeutronMsg>, ContractError> {
    // === 11-STEP VALIDATION FLOW ===

    // 1. Caller is admin or executor (already validated in dispatch)

    // 2. Validate non-zero input amount
    if params.coin_in.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // 3. Load route configuration
    let route_config = ROUTE_REGISTRY
        .may_load(deps.storage, params.route_id.clone())?
        .ok_or(ContractError::RouteNotRegistered {
            route_id: params.route_id.clone(),
        })?;

    // 4. Validate route is enabled
    if !route_config.enabled {
        return Err(ContractError::RouteDisabled {
            route_id: params.route_id.clone(),
        });
    }

    // 5. Validate coin_in denom matches route's denom_in
    skip::validate_coin_in_matches_route(&params.coin_in, &route_config)
        .map_err(|e| ContractError::InvalidRoute { reason: e })?;

    // 6. Validate operations match route's denom path
    validation::validate_operations_match_route(&route_config, &params.operations)?;

    // 7. If post_swap_action provided, validate recipient
    if let Some(ref action) = params.post_swap_action {
        match action {
            PostSwapAction::Transfer { to_address } => {
                validation::validate_recipient(&deps, to_address)?;
            }
        }
    }

    // 8. Query adapter balance
    let balance = deps
        .querier
        .query_balance(env.contract.address.clone(), params.coin_in.denom.clone())?;

    // 9. Verify sufficient balance
    if balance.amount < params.coin_in.amount {
        return Err(ContractError::InsufficientBalance {});
    }

    // 10. Load config and calculate timeout
    let config = CONFIG.load(deps.storage)?;
    let timeout_timestamp = params
        .timeout_nanos
        .unwrap_or(env.block.time.nanos() + config.default_timeout_nanos);

    // 10.5. Calculate oracle-enhanced min_asset
    let enhanced_min_asset = oracle::calculate_min_asset_with_oracle(
        &deps.as_ref(),
        &params.coin_in,
        &route_config,
        &config,
        &params.min_asset,
    );

    // 11. Create Skip swap message
    let swap_msg = skip::create_swap_and_action_msg(
        config.skip_contract,
        params.coin_in.clone(),
        params.operations,
        params.swap_venue_name,
        enhanced_min_asset,
        params.post_swap_action,
        timeout_timestamp,
    )?;

    Ok(Response::new()
        .add_message(swap_msg)
        .add_attribute("action", "execute_swap")
        .add_attribute("caller", info.sender)
        .add_attribute("route_id", params.route_id)
        .add_attribute("amount_in", params.coin_in.amount)
        .add_attribute("denom_in", params.coin_in.denom))
}

fn execute_register_depositor(
    deps: DepsMut<NeutronQuery>,
    _info: MessageInfo,
    depositor_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let depositor_addr = deps.api.addr_validate(&depositor_address)?;

    // Check if already registered
    if WHITELISTED_DEPOSITORS.has(deps.storage, depositor_addr.clone()) {
        return Err(ContractError::DepositorAlreadyRegistered { depositor_address });
    }

    let depositor = Depositor { enabled: true };
    WHITELISTED_DEPOSITORS.save(deps.storage, depositor_addr, &depositor)?;

    Ok(Response::new()
        .add_attribute("action", "register_depositor")
        .add_attribute("depositor", depositor_address))
}

fn execute_unregister_depositor(
    deps: DepsMut<NeutronQuery>,
    _info: MessageInfo,
    depositor_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let depositor_addr = deps.api.addr_validate(&depositor_address)?;

    // Check if registered
    if !WHITELISTED_DEPOSITORS.has(deps.storage, depositor_addr.clone()) {
        return Err(ContractError::DepositorNotRegistered { depositor_address });
    }

    WHITELISTED_DEPOSITORS.remove(deps.storage, depositor_addr);

    Ok(Response::new()
        .add_attribute("action", "unregister_depositor")
        .add_attribute("depositor", depositor_address))
}

fn execute_set_depositor_enabled(
    deps: DepsMut<NeutronQuery>,
    _info: MessageInfo,
    depositor_address: String,
    enabled: bool,
) -> Result<Response<NeutronMsg>, ContractError> {
    let depositor_addr = deps.api.addr_validate(&depositor_address)?;

    let mut depositor = WHITELISTED_DEPOSITORS
        .may_load(deps.storage, depositor_addr.clone())?
        .ok_or(ContractError::DepositorNotRegistered {
            depositor_address: depositor_address.clone(),
        })?;

    depositor.enabled = enabled;
    WHITELISTED_DEPOSITORS.save(deps.storage, depositor_addr, &depositor)?;

    Ok(Response::new()
        .add_attribute("action", "set_depositor_enabled")
        .add_attribute("depositor", depositor_address)
        .add_attribute("enabled", enabled.to_string()))
}

fn execute_add_executor(
    deps: DepsMut<NeutronQuery>,
    executor_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let executor_addr = deps.api.addr_validate(&executor_address)?;

    let mut executors = EXECUTORS.load(deps.storage)?;

    if executors.contains(&executor_addr) {
        return Err(ContractError::ExecutorAlreadyExists {
            executor: executor_address,
        });
    }

    executors.push(executor_addr);
    EXECUTORS.save(deps.storage, &executors)?;

    Ok(Response::new()
        .add_attribute("action", "add_executor")
        .add_attribute("executor", executor_address))
}

fn execute_remove_executor(
    deps: DepsMut<NeutronQuery>,
    executor_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let executor_addr = deps.api.addr_validate(&executor_address)?;

    let mut executors = EXECUTORS.load(deps.storage)?;

    let initial_len = executors.len();
    executors.retain(|addr| addr != executor_addr);

    if executors.len() == initial_len {
        return Err(ContractError::ExecutorNotFound {
            executor: executor_address,
        });
    }

    EXECUTORS.save(deps.storage, &executors)?;

    Ok(Response::new()
        .add_attribute("action", "remove_executor")
        .add_attribute("executor", executor_address))
}

fn execute_register_route(
    deps: DepsMut<NeutronQuery>,
    route_id: String,
    route_config: RouteConfig,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Check if already registered
    if ROUTE_REGISTRY.has(deps.storage, route_id.clone()) {
        return Err(ContractError::RouteAlreadyRegistered {
            route_id: route_id.clone(),
        });
    }

    // Validate route config
    validation::validate_route_config(&route_config)?;

    ROUTE_REGISTRY.save(deps.storage, route_id.clone(), &route_config)?;

    Ok(Response::new()
        .add_attribute("action", "register_route")
        .add_attribute("route_id", route_id))
}

fn execute_unregister_route(
    deps: DepsMut<NeutronQuery>,
    route_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Check if registered
    if !ROUTE_REGISTRY.has(deps.storage, route_id.clone()) {
        return Err(ContractError::RouteNotRegistered {
            route_id: route_id.clone(),
        });
    }

    ROUTE_REGISTRY.remove(deps.storage, route_id.clone());

    Ok(Response::new()
        .add_attribute("action", "unregister_route")
        .add_attribute("route_id", route_id))
}

fn execute_set_route_enabled(
    deps: DepsMut<NeutronQuery>,
    route_id: String,
    enabled: bool,
) -> Result<Response<NeutronMsg>, ContractError> {
    let mut route_config = ROUTE_REGISTRY
        .may_load(deps.storage, route_id.clone())?
        .ok_or(ContractError::RouteNotRegistered {
            route_id: route_id.clone(),
        })?;

    route_config.enabled = enabled;
    ROUTE_REGISTRY.save(deps.storage, route_id.clone(), &route_config)?;

    Ok(Response::new()
        .add_attribute("action", "set_route_enabled")
        .add_attribute("route_id", route_id)
        .add_attribute("enabled", enabled.to_string()))
}

fn execute_register_recipient(
    deps: DepsMut<NeutronQuery>,
    recipient_address: String,
    description: Option<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let recipient_addr = deps.api.addr_validate(&recipient_address)?;

    let recipient_config = RecipientConfig {
        address: recipient_address.clone(),
        description,
        enabled: true,
    };

    RECIPIENT_REGISTRY.save(deps.storage, recipient_addr, &recipient_config)?;

    Ok(Response::new()
        .add_attribute("action", "register_recipient")
        .add_attribute("recipient", recipient_address))
}

fn execute_unregister_recipient(
    deps: DepsMut<NeutronQuery>,
    recipient_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let recipient_addr = deps.api.addr_validate(&recipient_address)?;

    // Check if registered
    if !RECIPIENT_REGISTRY.has(deps.storage, recipient_addr.clone()) {
        return Err(ContractError::RecipientNotRegistered {
            recipient: recipient_address.clone(),
        });
    }

    RECIPIENT_REGISTRY.remove(deps.storage, recipient_addr);

    Ok(Response::new()
        .add_attribute("action", "unregister_recipient")
        .add_attribute("recipient", recipient_address))
}

fn execute_set_recipient_enabled(
    deps: DepsMut<NeutronQuery>,
    recipient_address: String,
    enabled: bool,
) -> Result<Response<NeutronMsg>, ContractError> {
    let recipient_addr = deps.api.addr_validate(&recipient_address)?;

    let mut recipient_config = RECIPIENT_REGISTRY
        .may_load(deps.storage, recipient_addr.clone())?
        .ok_or(ContractError::RecipientNotRegistered {
            recipient: recipient_address.clone(),
        })?;

    recipient_config.enabled = enabled;
    RECIPIENT_REGISTRY.save(deps.storage, recipient_addr, &recipient_config)?;

    Ok(Response::new()
        .add_attribute("action", "set_recipient_enabled")
        .add_attribute("recipient", recipient_address)
        .add_attribute("enabled", enabled.to_string()))
}

fn execute_update_config(
    deps: DepsMut<NeutronQuery>,
    skip_contract: Option<String>,
    default_timeout_nanos: Option<u64>,
    max_slippage_bps: Option<u64>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    if let Some(skip_contract_addr) = skip_contract {
        config.skip_contract = deps.api.addr_validate(&skip_contract_addr)?;
    }

    if let Some(timeout) = default_timeout_nanos {
        config.default_timeout_nanos = timeout;
    }

    if let Some(slippage) = max_slippage_bps {
        // Validate max_slippage_bps
        if slippage > MAX_SLIPPAGE_BPS {
            return Err(ContractError::InvalidSlippage {
                bps: slippage,
                max_bps: MAX_SLIPPAGE_BPS,
            });
        }
        config.max_slippage_bps = slippage;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

fn execute_register_denom_symbol(
    deps: DepsMut<NeutronQuery>,
    denom: String,
    symbol: String,
    description: Option<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Check if already registered
    if DENOM_SYMBOL_REGISTRY.has(deps.storage, denom.clone()) {
        return Err(ContractError::DenomSymbolAlreadyRegistered { denom });
    }

    let mapping = DenomSymbolMapping {
        symbol: symbol.clone(),
        description,
    };
    DENOM_SYMBOL_REGISTRY.save(deps.storage, denom.clone(), &mapping)?;

    Ok(Response::new()
        .add_attribute("action", "register_denom_symbol")
        .add_attribute("denom", denom)
        .add_attribute("symbol", symbol))
}

fn execute_unregister_denom_symbol(
    deps: DepsMut<NeutronQuery>,
    denom: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Check if registered
    if !DENOM_SYMBOL_REGISTRY.has(deps.storage, denom.clone()) {
        return Err(ContractError::DenomSymbolNotFound { denom });
    }

    DENOM_SYMBOL_REGISTRY.remove(deps.storage, denom.clone());

    Ok(Response::new()
        .add_attribute("action", "unregister_denom_symbol")
        .add_attribute("denom", denom))
}

fn execute_bulk_register_denom_symbols(
    deps: DepsMut<NeutronQuery>,
    mappings: Vec<DenomSymbolInput>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let mut count = 0;

    for input in mappings {
        let mapping = DenomSymbolMapping {
            symbol: input.symbol,
            description: input.description,
        };
        DENOM_SYMBOL_REGISTRY.save(deps.storage, input.denom, &mapping)?;
        count += 1;
    }

    Ok(Response::new()
        .add_attribute("action", "bulk_register_denom_symbols")
        .add_attribute("count", count.to_string()))
}

// ========== CROSS-CHAIN HANDLERS ==========

fn execute_register_token(
    deps: DepsMut<NeutronQuery>,
    symbol: String,
    native_chain: String,
    native_denom: String,
    decimals: Option<u8>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let token_info = TokenInfo {
        symbol: symbol.clone(),
        native_chain,
        native_denom,
        decimals,
    };

    TOKEN_REGISTRY.save(deps.storage, symbol.clone(), &token_info)?;

    Ok(Response::new()
        .add_attribute("action", "register_token")
        .add_attribute("symbol", symbol))
}

fn execute_register_chain(
    deps: DepsMut<NeutronQuery>,
    chain_id: String,
    allowed_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let chain_info = ChainInfo {
        chain_id: chain_id.clone(),
        allowed_address: allowed_address.clone(),
    };

    CHAIN_REGISTRY.save(deps.storage, chain_id.clone(), &chain_info)?;

    Ok(Response::new()
        .add_attribute("action", "register_chain")
        .add_attribute("chain_id", chain_id)
        .add_attribute("allowed_address", allowed_address))
}

fn execute_register_channel(
    deps: DepsMut<NeutronQuery>,
    source_chain: String,
    dest_chain: String,
    channel_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let channel_info = ChannelInfo {
        source_chain: source_chain.clone(),
        dest_chain: dest_chain.clone(),
        channel_id: channel_id.clone(),
    };

    CHANNEL_REGISTRY.save(
        deps.storage,
        (source_chain.clone(), dest_chain.clone()),
        &channel_info,
    )?;

    Ok(Response::new()
        .add_attribute("action", "register_channel")
        .add_attribute("source_chain", source_chain)
        .add_attribute("dest_chain", dest_chain)
        .add_attribute("channel_id", channel_id))
}

fn execute_update_osmosis_config(
    deps: DepsMut<NeutronQuery>,
    chain_id: Option<String>,
    skip_contract: Option<String>,
    swap_venue: Option<String>,
    ibc_adapter: Option<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let existing_config = OSMOSIS_CONFIG.may_load(deps.storage)?;
    let is_update = existing_config.is_some();

    let mut config = if let Some(existing) = existing_config {
        // Update existing config
        existing
    } else {
        // Create new config - all fields must be provided
        OsmosisConfig {
            chain_id: chain_id
                .clone()
                .ok_or(ContractError::OsmosisConfigNotSet {})?,
            skip_contract: skip_contract
                .clone()
                .ok_or(ContractError::OsmosisConfigNotSet {})?,
            swap_venue: swap_venue
                .clone()
                .ok_or(ContractError::OsmosisConfigNotSet {})?,
            ibc_adapter: deps.api.addr_validate(
                &ibc_adapter
                    .clone()
                    .ok_or(ContractError::OsmosisConfigNotSet {})?,
            )?,
        }
    };

    // Apply updates if config already existed
    if is_update {
        if let Some(chain_id) = chain_id {
            config.chain_id = chain_id;
        }
        if let Some(skip_contract) = skip_contract {
            config.skip_contract = skip_contract;
        }
        if let Some(swap_venue) = swap_venue {
            config.swap_venue = swap_venue;
        }
        if let Some(ibc_adapter) = ibc_adapter {
            config.ibc_adapter = deps.api.addr_validate(&ibc_adapter)?;
        }
    }

    OSMOSIS_CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("action", "update_osmosis_config")
        .add_attribute("chain_id", config.chain_id)
        .add_attribute("skip_contract", config.skip_contract)
        .add_attribute("swap_venue", config.swap_venue)
        .add_attribute("ibc_adapter", config.ibc_adapter))
}

fn execute_register_cross_chain_route(
    deps: DepsMut<NeutronQuery>,
    route_id: String,
    token_in: String,
    token_out: String,
    swap_chain: String,
    pool_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let route = CrossChainRoute {
        token_in: token_in.clone(),
        token_out: token_out.clone(),
        swap_chain: swap_chain.clone(),
        pool_id: pool_id.clone(),
        enabled: true,
    };

    CROSS_CHAIN_ROUTES.save(deps.storage, route_id.clone(), &route)?;

    Ok(Response::new()
        .add_attribute("action", "register_cross_chain_route")
        .add_attribute("route_id", route_id)
        .add_attribute("token_in", token_in)
        .add_attribute("token_out", token_out)
        .add_attribute("swap_chain", swap_chain)
        .add_attribute("pool_id", pool_id))
}

fn execute_cross_chain_swap(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    params: CrossChainSwapParams,
) -> Result<Response<NeutronMsg>, ContractError> {
    // 1. Load route config
    let route = CROSS_CHAIN_ROUTES
        .may_load(deps.storage, params.route_id.clone())?
        .ok_or(ContractError::CrossChainRouteNotRegistered {
            route_id: params.route_id.clone(),
        })?;

    // 2. Verify route is enabled
    if !route.enabled {
        return Err(ContractError::CrossChainRouteDisabled {
            route_id: params.route_id.clone(),
        });
    }

    // 3. Calculate denoms on Neutron
    let neutron_denom_in =
        crate::ibc::get_token_denom_on_chain(&deps.as_ref(), &route.token_in, "neutron-1")?;
    let neutron_denom_out =
        crate::ibc::get_token_denom_on_chain(&deps.as_ref(), &route.token_out, "neutron-1")?;

    // 4. Calculate denoms on Osmosis
    let osmosis_denom_in =
        crate::ibc::get_token_denom_on_chain(&deps.as_ref(), &route.token_in, &route.swap_chain)?;
    let osmosis_denom_out =
        crate::ibc::get_token_denom_on_chain(&deps.as_ref(), &route.token_out, &route.swap_chain)?;

    // 5. Validate user sent correct token and amount
    if info.funds.len() != 1 {
        return Err(ContractError::InvalidFunds {
            count: info.funds.len(),
        });
    }

    let sent_coin = &info.funds[0];
    if sent_coin.denom != neutron_denom_in {
        return Err(ContractError::InvalidDenom {
            expected: neutron_denom_in.clone(),
            actual: sent_coin.denom.clone(),
        });
    }

    if sent_coin.amount != params.amount_in {
        return Err(ContractError::AmountMismatch {
            expected: params.amount_in.to_string(),
            actual: sent_coin.amount.to_string(),
        });
    }

    // 6. Load Osmosis config
    let osmosis_config = OSMOSIS_CONFIG
        .may_load(deps.storage)?
        .ok_or(ContractError::OsmosisConfigNotSet {})?;

    // 7. Get recovery address from chain registry (for Neutron)
    let neutron_chain_info = CHAIN_REGISTRY
        .may_load(deps.storage, "neutron-1".to_string())?
        .ok_or(ContractError::ChainNotRegistered {
            chain_id: "neutron-1".to_string(),
        })?;

    // 8. Construct nested memo (PFM + wasm hook)
    let memo = crate::cross_chain::construct_osmosis_swap_memo(
        &deps.as_ref(),
        &osmosis_config,
        &route,
        &osmosis_denom_in,
        &osmosis_denom_out,
        &params.min_amount_out,
        &neutron_chain_info.allowed_address,
        &env,
    )?;

    // 9. Call IBC adapter with memo
    // Note: IBC adapter needs to be registered as a depositor with can_set_memo permission
    let ibc_transfer_msg = cosmwasm_std::WasmMsg::Execute {
        contract_addr: osmosis_config.ibc_adapter.to_string(),
        msg: to_json_binary(&serde_json::json!({
            "custom_action": {
                "transfer_funds": {
                    "coin": sent_coin,
                    "instructions": {
                        "destination_chain": osmosis_config.chain_id,
                        "recipient": osmosis_config.skip_contract,
                        "timeout_seconds": null,
                        "memo": memo,
                    }
                }
            }
        }))?,
        funds: info.funds,
    };

    Ok(Response::new()
        .add_message(ibc_transfer_msg)
        .add_attribute("action", "cross_chain_swap")
        .add_attribute("route_id", params.route_id)
        .add_attribute("token_in", route.token_in)
        .add_attribute("token_out", route.token_out)
        .add_attribute("amount_in", params.amount_in)
        .add_attribute("min_amount_out", params.min_amount_out)
        .add_attribute("neutron_denom_in", neutron_denom_in)
        .add_attribute("neutron_denom_out", neutron_denom_out)
        .add_attribute("osmosis_denom_in", osmosis_denom_in)
        .add_attribute("osmosis_denom_out", osmosis_denom_out))
}

// ========== QUERY ==========

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<NeutronQuery>, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::StandardQuery(interface_msg) => dispatch_query_standard(deps, env, interface_msg),
        QueryMsg::CustomQuery(custom_msg) => dispatch_query_custom(deps, custom_msg),
    }
}

fn dispatch_query_standard(
    deps: Deps<NeutronQuery>,
    env: Env,
    msg: AdapterInterfaceQueryMsg,
) -> StdResult<Binary> {
    match msg {
        AdapterInterfaceQueryMsg::Config {} => to_json_binary(&query_adapter_config(deps)?),
        AdapterInterfaceQueryMsg::AvailableForDeposit {
            depositor_address: _,
            denom: _,
        } => to_json_binary(&AvailableAmountResponse {
            amount: Uint128::MAX, // No deposit cap
        }),
        AdapterInterfaceQueryMsg::AvailableForWithdraw {
            depositor_address: _,
            denom,
        } => {
            let balance = deps.querier.query_balance(env.contract.address, denom)?;
            to_json_binary(&AvailableAmountResponse {
                amount: balance.amount,
            })
        }
        AdapterInterfaceQueryMsg::TimeToWithdraw {
            depositor_address: _,
            coin: _,
        } => to_json_binary(&TimeEstimateResponse {
            blocks: 0, // Instant withdrawal
            seconds: 0,
        }),
        AdapterInterfaceQueryMsg::AllPositions {} => {
            to_json_binary(&AllPositionsResponse { positions: vec![] }) // No position tracking
        }
        AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address: _,
            denom: _,
        } => to_json_binary(&DepositorPositionResponse {
            amount: Uint128::zero(), // No position tracking
        }),
        AdapterInterfaceQueryMsg::DepositorPositions {
            depositor_address: _,
        } => {
            to_json_binary(&DepositorPositionsResponse { positions: vec![] }) // No position tracking
        }
        AdapterInterfaceQueryMsg::RegisteredDepositors { enabled } => {
            to_json_binary(&query_registered_depositors(deps, enabled)?)
        }
    }
}

fn dispatch_query_custom(deps: Deps<NeutronQuery>, msg: SkipAdapterQueryMsg) -> StdResult<Binary> {
    match msg {
        SkipAdapterQueryMsg::Config {} => to_json_binary(&query_skip_config(deps)?),
        SkipAdapterQueryMsg::RouteConfig { route_id } => {
            to_json_binary(&query_route_config(deps, route_id)?)
        }
        SkipAdapterQueryMsg::AllRoutes {} => to_json_binary(&query_all_routes(deps)?),
        SkipAdapterQueryMsg::RecipientConfig { recipient_address } => {
            to_json_binary(&query_recipient_config(deps, recipient_address)?)
        }
        SkipAdapterQueryMsg::AllRecipients {} => to_json_binary(&query_all_recipients(deps)?),
        SkipAdapterQueryMsg::Executors {} => to_json_binary(&query_executors(deps)?),
        SkipAdapterQueryMsg::DenomSymbol { denom } => {
            to_json_binary(&query_denom_symbol(deps, denom)?)
        }
        SkipAdapterQueryMsg::AllDenomSymbols {} => {
            to_json_binary(&query_all_denom_symbols(deps)?)
        }
    }
}

// ========== QUERY HANDLERS ==========

fn query_adapter_config(deps: Deps<NeutronQuery>) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;
    let admins = ADMINS.load(deps.storage)?;

    // Return minimal config for standard query
    to_json_binary(&serde_json::json!({
        "skip_contract": config.skip_contract.to_string(),
        "admins": admins.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
    }))
}

fn query_skip_config(deps: Deps<NeutronQuery>) -> StdResult<SkipConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    let admins = ADMINS.load(deps.storage)?;

    Ok(SkipConfigResponse {
        admins: admins.iter().map(|a| a.to_string()).collect(),
        skip_contract: config.skip_contract.to_string(),
        default_timeout_nanos: config.default_timeout_nanos,
        max_slippage_bps: config.max_slippage_bps,
    })
}

fn query_route_config(
    deps: Deps<NeutronQuery>,
    route_id: String,
) -> StdResult<RouteConfigResponse> {
    let route_config = ROUTE_REGISTRY.load(deps.storage, route_id.clone())?;

    Ok(RouteConfigResponse {
        route_id,
        route_config,
    })
}

fn query_all_routes(deps: Deps<NeutronQuery>) -> StdResult<AllRoutesResponse> {
    let routes: StdResult<Vec<_>> = ROUTE_REGISTRY
        .range(deps.storage, None, None, Order::Ascending)
        .collect();

    Ok(AllRoutesResponse { routes: routes? })
}

fn query_recipient_config(
    deps: Deps<NeutronQuery>,
    recipient_address: String,
) -> StdResult<RecipientConfigResponse> {
    let recipient_addr = deps.api.addr_validate(&recipient_address)?;
    let recipient_config = RECIPIENT_REGISTRY.load(deps.storage, recipient_addr)?;

    Ok(RecipientConfigResponse { recipient_config })
}

fn query_all_recipients(deps: Deps<NeutronQuery>) -> StdResult<AllRecipientsResponse> {
    let recipients: StdResult<Vec<_>> = RECIPIENT_REGISTRY
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| {
            let (addr, config) = item?;
            Ok((addr.to_string(), config))
        })
        .collect();

    Ok(AllRecipientsResponse {
        recipients: recipients?,
    })
}

fn query_executors(deps: Deps<NeutronQuery>) -> StdResult<ExecutorsResponse> {
    let executors = EXECUTORS.load(deps.storage)?;

    Ok(ExecutorsResponse {
        executors: executors.iter().map(|a| a.to_string()).collect(),
    })
}

fn query_registered_depositors(
    deps: Deps<NeutronQuery>,
    enabled: Option<bool>,
) -> StdResult<RegisteredDepositorsResponse> {
    let depositors: StdResult<Vec<_>> = WHITELISTED_DEPOSITORS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|item| {
            let (addr, depositor) = item.ok()?;

            // Filter by enabled status if specified
            if let Some(enabled_filter) = enabled {
                if depositor.enabled != enabled_filter {
                    return None;
                }
            }

            Some(Ok(RegisteredDepositorInfo {
                depositor_address: addr.to_string(),
                enabled: depositor.enabled,
            }))
        })
        .collect();

    Ok(RegisteredDepositorsResponse {
        depositors: depositors?,
    })
}

fn query_denom_symbol(
    deps: Deps<NeutronQuery>,
    denom: String,
) -> StdResult<DenomSymbolResponse> {
    let mapping = DENOM_SYMBOL_REGISTRY.load(deps.storage, denom.clone())?;

    Ok(DenomSymbolResponse {
        denom,
        symbol: mapping.symbol,
        description: mapping.description,
    })
}

fn query_all_denom_symbols(deps: Deps<NeutronQuery>) -> StdResult<AllDenomSymbolsResponse> {
    let mappings: StdResult<Vec<_>> = DENOM_SYMBOL_REGISTRY
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| {
            let (denom, mapping) = item?;
            Ok(DenomSymbolInput {
                denom,
                symbol: mapping.symbol,
                description: mapping.description,
            })
        })
        .collect();

    Ok(AllDenomSymbolsResponse {
        mappings: mappings?,
    })
}


#[cfg(test)]
mod cross_chain_tests {
    use super::*;
    use crate::testing_mocks::mock_dependencies;
    use cosmwasm_std::testing::{message_info, mock_env};
    use cosmwasm_std::Addr;

    fn get_test_instantiate_msg(
        admin: &str,
        executor: &str,
        skip_contract: &str,
    ) -> InstantiateMsg {
        InstantiateMsg {
            admins: vec![admin.to_string()],
            skip_contract: skip_contract.to_string(),
            default_timeout_nanos: 1800000000000,
            max_slippage_bps: 100,
            executors: vec![executor.to_string()],
            initial_routes: vec![],
            initial_recipients: vec![],
            initial_depositors: vec![],
        }
    }

    #[test]
    fn test_register_token() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let admin = deps.api.addr_make("admin");
        let executor = deps.api.addr_make("executor");
        let skip_addr = deps.api.addr_make("skip");
        let info = message_info(&admin, &[]);

        // Instantiate
        instantiate(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            get_test_instantiate_msg(&admin.to_string(), &executor.to_string(), &skip_addr.to_string()),
        )
        .unwrap();

        // Register token
        let msg = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterToken {
            symbol: "ATOM".to_string(),
            native_chain: "cosmoshub-4".to_string(),
            native_denom: "uatom".to_string(),
            decimals: Some(6),
        });

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "register_token");
        assert_eq!(res.attributes[1].value, "ATOM");

        // Verify stored
        let token = TOKEN_REGISTRY.load(&deps.storage, "ATOM".to_string()).unwrap();
        assert_eq!(token.symbol, "ATOM");
        assert_eq!(token.native_chain, "cosmoshub-4");
        assert_eq!(token.native_denom, "uatom");
        assert_eq!(token.decimals, Some(6));
    }

    #[test]
    fn test_register_chain() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let admin = deps.api.addr_make("admin");
        let executor = deps.api.addr_make("executor");
        let skip_addr = deps.api.addr_make("skip");
        let info = message_info(&admin, &[]);

        instantiate(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            get_test_instantiate_msg(&admin.to_string(), &executor.to_string(), &skip_addr.to_string()),
        )
        .unwrap();

        // Register chain
        let msg = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterChain {
            chain_id: "neutron-1".to_string(),
            allowed_address: "neutron1recovery".to_string(),
        });

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "register_chain");

        // Verify stored
        let chain = CHAIN_REGISTRY.load(&deps.storage, "neutron-1".to_string()).unwrap();
        assert_eq!(chain.chain_id, "neutron-1");
        assert_eq!(chain.allowed_address, "neutron1recovery");
    }

    #[test]
    fn test_register_channel() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let admin = deps.api.addr_make("admin");
        let executor = deps.api.addr_make("executor");
        let skip_addr = deps.api.addr_make("skip");
        let info = message_info(&admin, &[]);

        instantiate(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            get_test_instantiate_msg(&admin.to_string(), &executor.to_string(), &skip_addr.to_string()),
        )
        .unwrap();

        // Register channel
        let msg = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterChannel {
            source_chain: "neutron-1".to_string(),
            dest_chain: "osmosis-1".to_string(),
            channel_id: "channel-0".to_string(),
        });

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "register_channel");

        // Verify stored
        let channel = CHANNEL_REGISTRY
            .load(&deps.storage, ("neutron-1".to_string(), "osmosis-1".to_string()))
            .unwrap();
        assert_eq!(channel.channel_id, "channel-0");
    }

    #[test]
    fn test_update_osmosis_config() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let admin = deps.api.addr_make("admin");
        let executor = deps.api.addr_make("executor");
        let ibc_adapter = deps.api.addr_make("ibcadapter");
        let skip_addr = deps.api.addr_make("skip");
        let osmo_skip = deps.api.addr_make("osmoskip");
        let info = message_info(&admin, &[]);

        instantiate(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            get_test_instantiate_msg(&admin.to_string(), &executor.to_string(), &skip_addr.to_string()),
        )
        .unwrap();

        // First initialize Osmosis config
        let init_msg = ExecuteMsg::CustomAction(SkipAdapterMsg::UpdateOsmosisConfig {
            chain_id: Some("osmosis-1".to_string()),
            skip_contract: Some(skip_addr.to_string()),
            swap_venue: Some("osmosis-poolmanager".to_string()),
            ibc_adapter: Some(ibc_adapter.to_string()),
        });
        execute(deps.as_mut(), env.clone(), info.clone(), init_msg).unwrap();

        // Now update with new values
        let msg = ExecuteMsg::CustomAction(SkipAdapterMsg::UpdateOsmosisConfig {
            chain_id: None,  // Keep existing
            skip_contract: Some(osmo_skip.to_string()),  // Update this
            swap_venue: None,  // Keep existing
            ibc_adapter: None,  // Keep existing
        });

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "update_osmosis_config");

        // Verify stored
        let config = OSMOSIS_CONFIG.load(&deps.storage).unwrap();
        assert_eq!(config.chain_id, "osmosis-1");
        assert_eq!(config.skip_contract, osmo_skip.to_string());
        assert_eq!(config.swap_venue, "osmosis-poolmanager");
        assert_eq!(config.ibc_adapter, ibc_adapter);
    }

    #[test]
    fn test_register_cross_chain_route() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let admin = deps.api.addr_make("admin");
        let executor = deps.api.addr_make("executor");
        let skip_addr = deps.api.addr_make("skip");
        let info = message_info(&admin, &[]);

        instantiate(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            get_test_instantiate_msg(&admin.to_string(), &executor.to_string(), &skip_addr.to_string()),
        )
        .unwrap();

        // Register route
        let msg = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterCrossChainRoute {
            route_id: "statom_to_atom_osmosis".to_string(),
            token_in: "stATOM".to_string(),
            token_out: "ATOM".to_string(),
            swap_chain: "osmosis-1".to_string(),
            pool_id: "1234".to_string(),
        });

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "register_cross_chain_route");

        // Verify stored
        let route = CROSS_CHAIN_ROUTES
            .load(&deps.storage, "statom_to_atom_osmosis".to_string())
            .unwrap();
        assert_eq!(route.token_in, "stATOM");
        assert_eq!(route.token_out, "ATOM");
        assert_eq!(route.pool_id, "1234");
        assert!(route.enabled);
    }

    #[test]
    fn test_cross_chain_swap_route_not_found() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let admin = deps.api.addr_make("admin");
        let executor = deps.api.addr_make("executor");
        let skip_addr = deps.api.addr_make("skip");
        let info = message_info(&admin, &[]);

        instantiate(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            get_test_instantiate_msg(&admin.to_string(), &executor.to_string(), &skip_addr.to_string()),
        )
        .unwrap();

        // Try to execute swap with non-existent route
        let msg = ExecuteMsg::CustomAction(SkipAdapterMsg::ExecuteCrossChainSwap {
            params: CrossChainSwapParams {
                route_id: "nonexistent".to_string(),
                amount_in: Uint128::new(1000000),
                min_amount_out: Uint128::new(950000),
            },
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        match err {
            ContractError::CrossChainRouteNotRegistered { route_id } => {
                assert_eq!(route_id, "nonexistent");
            }
            _ => panic!("Expected CrossChainRouteNotRegistered error"),
        }
    }

    #[test]
    fn test_unauthorized_registration() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let admin = deps.api.addr_make("admin");
        let executor = deps.api.addr_make("executor");
        let hacker = deps.api.addr_make("hacker");
        let skip_addr = deps.api.addr_make("skip");
        let admin_info = message_info(&admin, &[]);

        instantiate(
            deps.as_mut(),
            env.clone(),
            admin_info,
            get_test_instantiate_msg(&admin.to_string(), &executor.to_string(), &skip_addr.to_string()),
        )
        .unwrap();

        // Try to register token as non-admin
        let non_admin_info = message_info(&hacker, &[]);
        let msg = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterToken {
            symbol: "ATOM".to_string(),
            native_chain: "cosmoshub-4".to_string(),
            native_denom: "uatom".to_string(),
            decimals: Some(6),
        });

        let err = execute(deps.as_mut(), env, non_admin_info, msg).unwrap_err();
        match err {
            ContractError::UnauthorizedAdmin {} => {}
            _ => panic!("Expected UnauthorizedAdmin error"),
        }
    }
}
