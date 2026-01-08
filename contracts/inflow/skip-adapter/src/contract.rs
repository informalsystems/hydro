use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Order,
    Response, StdResult, Uint128,
};
use cw2::set_contract_version;
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::cross_chain::{
    build_cross_chain_swap_ibc_adapter_msg, construct_cross_chain_wasm_hook_memo,
};
use crate::error::ContractError;
use crate::msg::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AllPositionsResponse, AllRoutesResponse,
    AvailableAmountResponse, DepositorPositionResponse, DepositorPositionsResponse, ExecuteMsg,
    ExecutorsResponse, InstantiateMsg, QueryMsg, RegisteredDepositorInfo,
    RegisteredDepositorsResponse, RouteResponse, SkipAdapterMsg, SkipAdapterQueryMsg,
    SkipConfigResponse, SwapParams, TimeEstimateResponse,
};
use crate::skip::create_local_swap_and_action_msg;
use crate::state::{
    Config, Depositor, SwapVenue, UnifiedRoute, ADMINS, CONFIG, EXECUTORS, ROUTES,
    WHITELISTED_DEPOSITORS,
};
use crate::validation::{
    validate_admin_or_executor, validate_config_admin, validate_depositor_caller,
    validate_route_config,
};

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

    // Validate addresses (only for contracts on this chain)
    let ibc_adapter = deps.api.addr_validate(&msg.ibc_adapter)?;

    // Validate max_slippage_bps
    if msg.max_slippage_bps > MAX_SLIPPAGE_BPS {
        return Err(ContractError::InvalidSlippage {
            bps: msg.max_slippage_bps,
            max_bps: MAX_SLIPPAGE_BPS,
        });
    }

    // Store unified config
    // Skip contracts are not validated since they may be on different chains
    let config = Config {
        skip_contracts: msg.skip_contracts,
        ibc_adapter,
        default_timeout_nanos: msg.default_timeout_nanos,
        max_slippage_bps: msg.max_slippage_bps,
    };
    CONFIG.save(deps.storage, &config)?;

    // Register initial routes
    for (route_id, route) in msg.initial_routes {
        validate_route_config(&route)?;
        ROUTES.save(deps.storage, route_id, &route)?;
    }

    // Register initial depositors
    for depositor_addr_str in msg.initial_depositors.clone() {
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
        .add_attribute("version", CONTRACT_VERSION)
        .add_attribute("num_admins", admins.len().to_string())
        .add_attribute("num_executors", executors.len().to_string())
        .add_attribute("num_depositors", msg.initial_depositors.len().to_string()))
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
        } => {
            validate_config_admin(&deps, &info)?;
            execute_register_depositor(deps, depositor_address)
        }
        AdapterInterfaceMsg::UnregisterDepositor { depositor_address } => {
            validate_config_admin(&deps, &info)?;
            execute_unregister_depositor(deps, depositor_address)
        }
        AdapterInterfaceMsg::SetDepositorEnabled {
            depositor_address,
            enabled,
        } => {
            validate_config_admin(&deps, &info)?;
            execute_set_depositor_enabled(deps, depositor_address, enabled)
        }
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
            validate_admin_or_executor(&deps, &info)?;
            execute_swap(deps, env, params)
        }
        SkipAdapterMsg::AddExecutor { executor_address } => {
            validate_config_admin(&deps, &info)?;
            execute_add_executor(deps, executor_address)
        }
        SkipAdapterMsg::RemoveExecutor { executor_address } => {
            validate_config_admin(&deps, &info)?;
            execute_remove_executor(deps, executor_address)
        }
        SkipAdapterMsg::RegisterRoute { route_id, route } => {
            validate_config_admin(&deps, &info)?;
            execute_register_route(deps, route_id, route)
        }
        SkipAdapterMsg::UnregisterRoute { route_id } => {
            validate_config_admin(&deps, &info)?;
            execute_unregister_route(deps, route_id)
        }
        SkipAdapterMsg::SetRouteEnabled { route_id, enabled } => {
            validate_config_admin(&deps, &info)?;
            execute_set_route_enabled(deps, route_id, enabled)
        }
        SkipAdapterMsg::UpdateConfig {
            skip_contracts,
            ibc_adapter,
            default_timeout_nanos,
            max_slippage_bps,
        } => {
            validate_config_admin(&deps, &info)?;
            execute_update_config(
                deps,
                skip_contracts,
                ibc_adapter,
                default_timeout_nanos,
                max_slippage_bps,
            )
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
    validate_depositor_caller(&deps, &info)?;

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
    validate_depositor_caller(&deps, &info)?;

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
        .add_attribute("withdrawer", info.sender)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", coin.denom))
}

fn execute_swap(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    params: SwapParams,
) -> Result<Response<NeutronMsg>, ContractError> {
    // 1. Validate non-zero input amount
    if params.amount_in.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // 2. Load unified route
    let route = ROUTES
        .may_load(deps.storage, params.route_id.clone())?
        .ok_or(ContractError::RouteNotRegistered {
            route_id: params.route_id.clone(),
        })?;

    // 3. Validate route is enabled
    if !route.enabled {
        return Err(ContractError::RouteDisabled {
            route_id: params.route_id.clone(),
        });
    }

    // 4. Load config
    let config = CONFIG.load(deps.storage)?;

    // 5. Build coin from input parameters
    let coin_in = Coin {
        denom: route.denom_in.clone(),
        amount: params.amount_in,
    };

    // 6. Verify skip-adapter has sufficient balance
    let balance = deps
        .querier
        .query_balance(env.contract.address.clone(), coin_in.denom.clone())?;

    if balance.amount < coin_in.amount {
        return Err(ContractError::InsufficientBalance {});
    }

    // 7. Calculate timeout
    let timeout_nanos = env.block.time.nanos() + config.default_timeout_nanos;

    // 6. Dispatch based on venue type
    if route.venue.is_local() {
        execute_local_swap(deps, env, &config, &route, &coin_in, &params, timeout_nanos)
    } else {
        execute_cross_chain_swap(env, &config, &route, &coin_in, &params, timeout_nanos)
    }
}

fn execute_local_swap(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    config: &Config,
    route: &UnifiedRoute,
    coin_in: &Coin,
    params: &SwapParams,
    timeout_nanos: u64,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Get Skip contract for this venue
    let skip_contract_str = config.get_skip_contract(&route.swap_venue_name)?;
    let skip_contract = deps.api.addr_validate(skip_contract_str)?;

    // Build Skip message using stored operations from route
    // Transfer output back to this adapter contract
    let min_coin_out = Coin {
        denom: route.denom_out.clone(),
        amount: params.min_amount_out,
    };
    let swap_msg = create_local_swap_and_action_msg(
        skip_contract,
        coin_in.clone(),
        min_coin_out,
        route.operations.clone(),
        route.swap_venue_name.clone(),
        env.contract.address.to_string(), // Return funds to adapter
        timeout_nanos,
    )?;

    Ok(Response::new()
        .add_message(swap_msg)
        .add_attribute("action", "swap")
        .add_attribute("venue", "neutron")
        .add_attribute("route_id", &params.route_id)
        .add_attribute("amount_in", coin_in.amount))
}

fn execute_cross_chain_swap(
    env: Env,
    config: &Config,
    route: &UnifiedRoute,
    coin_in: &Coin,
    params: &SwapParams,
    timeout_nanos: u64,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Build wasm hook memo (Skip swap message)
    let wasm_hook_memo =
        construct_cross_chain_wasm_hook_memo(config, route, &params.min_amount_out, &env)?;

    // Build typed IBC adapter message using route's forward_path
    let ibc_adapter_msg = build_cross_chain_swap_ibc_adapter_msg(
        config.ibc_adapter.to_string(),
        coin_in.clone(),
        &route.forward_path,
        wasm_hook_memo,
        timeout_nanos,
    )?;

    // Send funds from Skip adapter to IBC adapter via bank send
    let bank_send_msg = BankMsg::Send {
        to_address: config.ibc_adapter.to_string(),
        amount: vec![coin_in.clone()],
    };

    Ok(Response::new()
        .add_message(bank_send_msg) // First: send funds to IBC adapter
        .add_message(ibc_adapter_msg) // Second: call TransferFunds
        .add_attribute("action", "swap")
        .add_attribute("venue", "cross_chain")
        .add_attribute("swap_venue_name", &route.swap_venue_name)
        .add_attribute("route_id", &params.route_id)
        .add_attribute("amount_in", coin_in.amount))
}

fn execute_register_depositor(
    deps: DepsMut<NeutronQuery>,
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
    route: UnifiedRoute,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate route configuration
    validate_route_config(&route)?;

    // Check if already registered
    if ROUTES.has(deps.storage, route_id.clone()) {
        return Err(ContractError::RouteAlreadyRegistered {
            route_id: route_id.clone(),
        });
    }

    // Save unified route
    ROUTES.save(deps.storage, route_id.clone(), &route)?;

    Ok(Response::new()
        .add_attribute("action", "register_route")
        .add_attribute("route_id", route_id)
        .add_attribute("venue", format!("{:?}", route.venue)))
}

fn execute_unregister_route(
    deps: DepsMut<NeutronQuery>,
    route_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Check if registered
    if !ROUTES.has(deps.storage, route_id.clone()) {
        return Err(ContractError::RouteNotRegistered {
            route_id: route_id.clone(),
        });
    }

    ROUTES.remove(deps.storage, route_id.clone());

    Ok(Response::new()
        .add_attribute("action", "unregister_route")
        .add_attribute("route_id", route_id))
}

fn execute_set_route_enabled(
    deps: DepsMut<NeutronQuery>,
    route_id: String,
    enabled: bool,
) -> Result<Response<NeutronMsg>, ContractError> {
    let mut route = ROUTES.may_load(deps.storage, route_id.clone())?.ok_or(
        ContractError::RouteNotRegistered {
            route_id: route_id.clone(),
        },
    )?;

    route.enabled = enabled;
    ROUTES.save(deps.storage, route_id.clone(), &route)?;

    Ok(Response::new()
        .add_attribute("action", "set_route_enabled")
        .add_attribute("route_id", route_id)
        .add_attribute("enabled", enabled.to_string()))
}

fn execute_update_config(
    deps: DepsMut<NeutronQuery>,
    skip_contracts: Option<std::collections::BTreeMap<String, String>>,
    ibc_adapter: Option<String>,
    default_timeout_nanos: Option<u64>,
    max_slippage_bps: Option<u64>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    // Merge skip_contracts (add/update entries, don't remove existing)
    if let Some(contracts) = skip_contracts {
        for (chain, contract_addr) in contracts {
            config.skip_contracts.insert(chain, contract_addr);
        }
    }

    if let Some(addr) = ibc_adapter {
        config.ibc_adapter = deps.api.addr_validate(&addr)?;
    }

    if let Some(timeout) = default_timeout_nanos {
        config.default_timeout_nanos = timeout;
    }

    if let Some(slippage) = max_slippage_bps {
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
        AdapterInterfaceQueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        AdapterInterfaceQueryMsg::AvailableForDeposit {
            depositor_address,
            denom: _,
        } => to_json_binary(&query_available_for_deposit(deps, depositor_address)?),
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
        } => to_json_binary(&TimeEstimateResponse {
            blocks: 0, // Instant withdrawal
            seconds: 0,
        }),
        AdapterInterfaceQueryMsg::AllPositions {} => {
            to_json_binary(&AllPositionsResponse { positions: vec![] })
        }
        AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address: _,
            denom: _,
        } => to_json_binary(&DepositorPositionResponse {
            amount: Uint128::zero(),
        }),
        AdapterInterfaceQueryMsg::DepositorPositions {
            depositor_address: _,
        } => to_json_binary(&DepositorPositionsResponse { positions: vec![] }),
        AdapterInterfaceQueryMsg::RegisteredDepositors { enabled } => {
            to_json_binary(&query_registered_depositors(deps, enabled)?)
        }
    }
}

fn dispatch_query_custom(deps: Deps<NeutronQuery>, msg: SkipAdapterQueryMsg) -> StdResult<Binary> {
    match msg {
        SkipAdapterQueryMsg::Route { route_id } => {
            to_json_binary(&query_route_config(deps, route_id)?)
        }
        SkipAdapterQueryMsg::AllRoutes { venue } => {
            to_json_binary(&query_all_routes_filtered(deps, venue)?)
        }
        SkipAdapterQueryMsg::Executors {} => to_json_binary(&query_executors(deps)?),
    }
}

// ========== QUERY HANDLERS ==========

fn query_config(deps: Deps<NeutronQuery>) -> StdResult<SkipConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    let admins = ADMINS.load(deps.storage)?;

    Ok(SkipConfigResponse {
        admins: admins.iter().map(|a| a.to_string()).collect(),
        skip_contracts: config.skip_contracts,
        ibc_adapter: config.ibc_adapter.to_string(),
        default_timeout_nanos: config.default_timeout_nanos,
        max_slippage_bps: config.max_slippage_bps,
    })
}

fn query_route_config(deps: Deps<NeutronQuery>, route_id: String) -> StdResult<RouteResponse> {
    let route = ROUTES.load(deps.storage, route_id.clone())?;
    Ok(RouteResponse { route_id, route })
}

fn query_all_routes_filtered(
    deps: Deps<NeutronQuery>,
    venue: Option<SwapVenue>,
) -> StdResult<AllRoutesResponse> {
    let all_routes: StdResult<Vec<_>> = ROUTES
        .range(deps.storage, None, None, Order::Ascending)
        .collect();

    let routes = if let Some(venue_filter) = venue {
        all_routes?
            .into_iter()
            .filter(|(_, route)| route.venue == venue_filter)
            .collect()
    } else {
        all_routes?
    };

    Ok(AllRoutesResponse { routes })
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

fn query_available_for_deposit(
    deps: Deps<NeutronQuery>,
    depositor_address: String,
) -> StdResult<AvailableAmountResponse> {
    let depositor_addr = deps.api.addr_validate(&depositor_address)?;

    // Check if depositor is registered and enabled
    let depositor = WHITELISTED_DEPOSITORS.may_load(deps.storage, depositor_addr)?;

    let amount = match depositor {
        Some(d) if d.enabled => Uint128::MAX, // No deposit cap if enabled
        _ => Uint128::zero(),                 // Not registered or not enabled
    };

    Ok(AvailableAmountResponse { amount })
}

fn query_available_for_withdraw(
    deps: Deps<NeutronQuery>,
    env: Env,
    depositor_address: String,
    denom: String,
) -> StdResult<AvailableAmountResponse> {
    let depositor_addr = deps.api.addr_validate(&depositor_address)?;

    // Check if depositor is registered and enabled
    let depositor = WHITELISTED_DEPOSITORS.may_load(deps.storage, depositor_addr)?;

    let amount = match depositor {
        Some(d) if d.enabled => {
            // Query contract balance if depositor is enabled
            let balance = deps.querier.query_balance(env.contract.address, denom)?;
            balance.amount
        }
        _ => Uint128::zero(), // Not registered or not enabled
    };

    Ok(AvailableAmountResponse { amount })
}
