use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo,
    Order, Response, Uint128,
};
use cw2::set_contract_version;
use cw_utils::one_coin;
use interface::inflow_adapter::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AllPositionsResponse, AvailableAmountResponse,
    DepositorPositionResponse, DepositorPositionsResponse, RegisteredDepositorInfo,
    RegisteredDepositorsResponse, TimeEstimateResponse,
};

use crate::{
    drop,
    error::ContractError,
    msg::{
        ConfigResponse, DAssetAdapterMsg, DAssetAdapterQueryMsg, ExecuteMsg, ExecutorsResponse,
        InstantiateMsg, QueryMsg, TokenConfigResponse, TokensResponse,
    },
    state::{DAssetConfig, Depositor, ADMINS, EXECUTORS, TOKEN_REGISTRY, WHITELISTED_DEPOSITORS},
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
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    if msg.initial_admins.is_empty() {
        return Err(ContractError::AtLeastOneAdmin {});
    }

    if msg.initial_executors.is_empty() {
        return Err(ContractError::AtLeastOneExecutor {});
    }

    let admin_count = msg.initial_admins.len();
    let executor_count = msg.initial_executors.len();
    let depositor_count = msg.initial_depositors.len();
    let token_count = msg.initial_tokens.len();

    let admins: Vec<Addr> = msg
        .initial_admins
        .into_iter()
        .map(|a| deps.api.addr_validate(&a))
        .collect::<Result<_, _>>()?;

    let executors: Vec<Addr> = msg
        .initial_executors
        .into_iter()
        .map(|a| deps.api.addr_validate(&a))
        .collect::<Result<_, _>>()?;

    ADMINS.save(deps.storage, &admins)?;
    EXECUTORS.save(deps.storage, &executors)?;

    // Register initial depositors
    for depositor_addr in msg.initial_depositors {
        let addr = deps.api.addr_validate(&depositor_addr)?;
        WHITELISTED_DEPOSITORS.save(deps.storage, &addr, &Depositor { enabled: true })?;
    }

    // Register initial tokens
    for token in msg.initial_tokens {
        let config = DAssetConfig {
            enabled: true,
            denom: token.denom,
            drop_staking_core: deps.api.addr_validate(&token.drop_staking_core)?,
            drop_voucher: deps.api.addr_validate(&token.drop_voucher)?,
            drop_withdrawal_manager: deps.api.addr_validate(&token.drop_withdrawal_manager)?,
            base_asset_denom: token.base_asset_denom,
        };
        TOKEN_REGISTRY.save(deps.storage, &token.symbol, &config)?;
    }

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("admin_count", admin_count.to_string())
        .add_attribute("executor_count", executor_count.to_string())
        .add_attribute("depositor_count", depositor_count.to_string())
        .add_attribute("token_count", token_count.to_string()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::StandardAction(action_msg) => {
            dispatch_standard_action(deps, env, info, action_msg)
        }
        ExecuteMsg::CustomAction(custom_msg) => {
            dispatch_custom_execute(deps, env, info, custom_msg)
        }
    }
}

fn dispatch_standard_action(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: AdapterInterfaceMsg,
) -> Result<Response, ContractError> {
    match msg {
        AdapterInterfaceMsg::Deposit {} => execute_deposit(deps, info),
        AdapterInterfaceMsg::Withdraw { coin } => execute_standard_withdraw(deps, env, info, coin),
        AdapterInterfaceMsg::RegisterDepositor {
            depositor_address,
            metadata: _,
        } => {
            validate_admin(&deps, &info)?;
            execute_register_depositor(deps, depositor_address)
        }
        AdapterInterfaceMsg::UnregisterDepositor { depositor_address } => {
            validate_admin(&deps, &info)?;
            execute_unregister_depositor(deps, depositor_address)
        }
        AdapterInterfaceMsg::SetDepositorEnabled {
            depositor_address,
            enabled,
        } => {
            validate_admin(&deps, &info)?;
            execute_set_depositor_enabled(deps, depositor_address, enabled)
        }
    }
}

fn dispatch_custom_execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: DAssetAdapterMsg,
) -> Result<Response, ContractError> {
    match msg {
        DAssetAdapterMsg::UnbondInDrop { symbol, amount } => {
            validate_executor(&deps, &info)?;
            execute_unbond_in_drop(deps, env, symbol, amount)
        }

        DAssetAdapterMsg::WithdrawFromDrop { symbol, token_id } => {
            validate_executor(&deps, &info)?;
            execute_withdraw_from_drop(deps, symbol, token_id)
        }

        DAssetAdapterMsg::RegisterToken {
            symbol,
            denom,
            drop_staking_core,
            drop_voucher,
            drop_withdrawal_manager,
            base_asset_denom,
        } => {
            validate_admin(&deps, &info)?;
            execute_register_token(
                deps,
                symbol,
                denom,
                drop_staking_core,
                drop_voucher,
                drop_withdrawal_manager,
                base_asset_denom,
            )
        }

        DAssetAdapterMsg::UnregisterToken { symbol } => {
            validate_admin(&deps, &info)?;
            execute_unregister_token(deps, symbol)
        }

        DAssetAdapterMsg::SetTokenEnabled { symbol, enabled } => {
            validate_admin(&deps, &info)?;
            execute_set_token_enabled(deps, symbol, enabled)
        }

        DAssetAdapterMsg::AddExecutor { executor_address } => {
            validate_admin(&deps, &info)?;
            execute_add_executor(deps, executor_address)
        }

        DAssetAdapterMsg::RemoveExecutor { executor_address } => {
            validate_admin(&deps, &info)?;
            execute_remove_executor(deps, executor_address)
        }
    }
}

// ============================================================================
// StandardAction Handlers
// ============================================================================

fn execute_deposit(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    validate_depositor_caller(&deps, &info)?;

    let coin = one_coin(&info)?;

    // Find token config by iterating registry (simple since few tokens expected)
    let token_entry = TOKEN_REGISTRY
        .range(deps.storage, None, None, Order::Ascending)
        .find(|r| {
            r.as_ref()
                .map(|(_, config)| config.denom == coin.denom)
                .unwrap_or(false)
        })
        .transpose()?
        .ok_or(ContractError::TokenNotRegistered {
            denom: coin.denom.clone(),
        })?;

    let (symbol, token_config) = token_entry;

    if !token_config.enabled {
        return Err(ContractError::TokenDisabled {
            symbol: symbol.clone(),
        });
    }

    // Funds are now in adapter, emit detailed events
    Ok(Response::new()
        .add_attribute("action", "deposit")
        .add_attribute("depositor", info.sender)
        .add_attribute("symbol", symbol)
        .add_attribute("denom", &coin.denom)
        .add_attribute("amount", coin.amount))
}

fn execute_standard_withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    coin: Coin,
) -> Result<Response, ContractError> {
    validate_depositor_caller(&deps, &info)?;

    // Query adapter balance
    let balance = deps
        .querier
        .query_balance(env.contract.address, coin.denom.clone())?;

    if balance.amount < coin.amount {
        return Err(ContractError::InsufficientBalance {});
    }

    // Send funds back to depositor (vault)
    Ok(Response::new()
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![coin.clone()],
        })
        .add_attribute("action", "withdraw")
        .add_attribute("depositor", info.sender)
        .add_attribute("denom", coin.denom)
        .add_attribute("amount", coin.amount))
}

fn execute_register_depositor(
    deps: DepsMut,
    depositor_address: String,
) -> Result<Response, ContractError> {
    let addr = deps.api.addr_validate(&depositor_address)?;

    if WHITELISTED_DEPOSITORS.has(deps.storage, &addr) {
        return Err(ContractError::DepositorAlreadyRegistered {
            address: depositor_address,
        });
    }

    WHITELISTED_DEPOSITORS.save(deps.storage, &addr, &Depositor { enabled: true })?;

    Ok(Response::new()
        .add_attribute("action", "register_depositor")
        .add_attribute("depositor", depositor_address))
}

fn execute_unregister_depositor(
    deps: DepsMut,
    depositor_address: String,
) -> Result<Response, ContractError> {
    let addr = deps.api.addr_validate(&depositor_address)?;

    if !WHITELISTED_DEPOSITORS.has(deps.storage, &addr) {
        return Err(ContractError::DepositorNotWhitelisted {
            address: depositor_address,
        });
    }

    WHITELISTED_DEPOSITORS.remove(deps.storage, &addr);

    Ok(Response::new()
        .add_attribute("action", "unregister_depositor")
        .add_attribute("depositor", depositor_address))
}

fn execute_set_depositor_enabled(
    deps: DepsMut,
    depositor_address: String,
    enabled: bool,
) -> Result<Response, ContractError> {
    let addr = deps.api.addr_validate(&depositor_address)?;

    let mut depositor = WHITELISTED_DEPOSITORS
        .may_load(deps.storage, &addr)?
        .ok_or(ContractError::DepositorNotWhitelisted {
            address: depositor_address.clone(),
        })?;

    depositor.enabled = enabled;
    WHITELISTED_DEPOSITORS.save(deps.storage, &addr, &depositor)?;

    Ok(Response::new()
        .add_attribute("action", "set_depositor_enabled")
        .add_attribute("depositor", depositor_address)
        .add_attribute("enabled", enabled.to_string()))
}

// ============================================================================
// Executor Operations
// ============================================================================

fn execute_unbond_in_drop(
    deps: DepsMut,
    env: Env,
    symbol: String,
    amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let token_config = TOKEN_REGISTRY.may_load(deps.storage, &symbol)?.ok_or(
        ContractError::TokenNotRegisteredBySymbol {
            symbol: symbol.clone(),
        },
    )?;

    if !token_config.enabled {
        return Err(ContractError::TokenDisabled {
            symbol: symbol.clone(),
        });
    }

    // Use the actual denom from token config to query balance
    let balance = deps
        .querier
        .query_balance(env.contract.address, token_config.denom.clone())?;

    if balance.amount.is_zero() {
        return Err(ContractError::NoFundsToUnbond {});
    }

    // Use specified amount or full balance
    let unbond_amount = amount.unwrap_or(balance.amount);

    if unbond_amount > balance.amount {
        return Err(ContractError::InsufficientBalance {});
    }

    let unbond_coin = Coin {
        denom: token_config.denom.clone(),
        amount: unbond_amount,
    };

    let msg = drop::unbond_msg(token_config.drop_staking_core, vec![unbond_coin])?;

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "unbond")
        .add_attribute("symbol", symbol)
        .add_attribute("denom", token_config.denom)
        .add_attribute("amount", unbond_amount))
}

fn execute_withdraw_from_drop(
    deps: DepsMut,
    symbol: String,
    token_id: String,
) -> Result<Response, ContractError> {
    let token_config = TOKEN_REGISTRY.may_load(deps.storage, &symbol)?.ok_or(
        ContractError::TokenNotRegisteredBySymbol {
            symbol: symbol.clone(),
        },
    )?;

    if !token_config.enabled {
        return Err(ContractError::TokenDisabled {
            symbol: symbol.clone(),
        });
    }

    let msg = drop::withdraw_voucher_msg(
        token_config.drop_voucher,
        token_config.drop_withdrawal_manager,
        token_id.clone(),
    )?;

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "withdraw_from_drop")
        .add_attribute("symbol", symbol)
        .add_attribute("token_id", token_id)
        .add_attribute("base_asset_denom", token_config.base_asset_denom))
}

// ============================================================================
// Admin Operations
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn execute_register_token(
    deps: DepsMut,
    symbol: String,
    denom: String,
    drop_staking_core: String,
    drop_voucher: String,
    drop_withdrawal_manager: String,
    base_asset_denom: String,
) -> Result<Response, ContractError> {
    if TOKEN_REGISTRY.has(deps.storage, &symbol) {
        return Err(ContractError::TokenAlreadyRegistered { symbol });
    }

    let config = DAssetConfig {
        enabled: true,
        denom: denom.clone(),
        drop_staking_core: deps.api.addr_validate(&drop_staking_core)?,
        drop_voucher: deps.api.addr_validate(&drop_voucher)?,
        drop_withdrawal_manager: deps.api.addr_validate(&drop_withdrawal_manager)?,
        base_asset_denom: base_asset_denom.clone(),
    };

    TOKEN_REGISTRY.save(deps.storage, &symbol, &config)?;

    Ok(Response::new()
        .add_attribute("action", "register_token")
        .add_attribute("symbol", symbol)
        .add_attribute("denom", denom)
        .add_attribute("base_asset_denom", base_asset_denom))
}

fn execute_unregister_token(deps: DepsMut, symbol: String) -> Result<Response, ContractError> {
    let config = TOKEN_REGISTRY.may_load(deps.storage, &symbol)?.ok_or(
        ContractError::TokenNotRegisteredBySymbol {
            symbol: symbol.clone(),
        },
    )?;

    let denom = config.denom;
    TOKEN_REGISTRY.remove(deps.storage, &symbol);

    Ok(Response::new()
        .add_attribute("action", "unregister_token")
        .add_attribute("symbol", symbol)
        .add_attribute("denom", denom))
}

fn execute_set_token_enabled(
    deps: DepsMut,
    symbol: String,
    enabled: bool,
) -> Result<Response, ContractError> {
    let mut config = TOKEN_REGISTRY.may_load(deps.storage, &symbol)?.ok_or(
        ContractError::TokenNotRegisteredBySymbol {
            symbol: symbol.clone(),
        },
    )?;

    config.enabled = enabled;
    TOKEN_REGISTRY.save(deps.storage, &symbol, &config)?;

    Ok(Response::new()
        .add_attribute("action", "set_token_enabled")
        .add_attribute("symbol", symbol)
        .add_attribute("enabled", enabled.to_string()))
}

fn execute_add_executor(
    deps: DepsMut,
    executor_address: String,
) -> Result<Response, ContractError> {
    let addr = deps.api.addr_validate(&executor_address)?;
    let mut executors = EXECUTORS.load(deps.storage)?;

    if executors.contains(&addr) {
        return Err(ContractError::ExecutorAlreadyExists {
            address: executor_address,
        });
    }

    executors.push(addr);
    EXECUTORS.save(deps.storage, &executors)?;

    Ok(Response::new()
        .add_attribute("action", "add_executor")
        .add_attribute("executor", executor_address)
        .add_attribute("executor_count", executors.len().to_string()))
}

fn execute_remove_executor(
    deps: DepsMut,
    executor_address: String,
) -> Result<Response, ContractError> {
    let addr = deps.api.addr_validate(&executor_address)?;
    let mut executors = EXECUTORS.load(deps.storage)?;

    let pos = executors
        .iter()
        .position(|e| *e == addr)
        .ok_or(ContractError::ExecutorNotFound {
            address: executor_address.clone(),
        })?;

    executors.remove(pos);

    if executors.is_empty() {
        return Err(ContractError::AtLeastOneExecutor {});
    }

    EXECUTORS.save(deps.storage, &executors)?;

    Ok(Response::new()
        .add_attribute("action", "remove_executor")
        .add_attribute("executor", executor_address)
        .add_attribute("executor_count", executors.len().to_string()))
}

// ============================================================================
// Queries
// ============================================================================

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> Result<Binary, ContractError> {
    match msg {
        QueryMsg::StandardQuery(standard_query) => {
            dispatch_standard_query(deps, env, standard_query)
        }
        QueryMsg::CustomQuery(custom_query) => dispatch_custom_query(deps, custom_query),
    }
}

fn dispatch_standard_query(
    deps: Deps,
    env: Env,
    query: AdapterInterfaceQueryMsg,
) -> Result<Binary, ContractError> {
    match query {
        AdapterInterfaceQueryMsg::Config {} => {
            to_json_binary(&query_config(deps)?).map_err(ContractError::Std)
        }
        AdapterInterfaceQueryMsg::RegisteredDepositors { enabled } => {
            to_json_binary(&query_registered_depositors(deps, enabled)?).map_err(ContractError::Std)
        }
        AdapterInterfaceQueryMsg::AvailableForDeposit { .. } => {
            to_json_binary(&query_available_for_deposit()).map_err(ContractError::Std)
        }
        AdapterInterfaceQueryMsg::AvailableForWithdraw { denom, .. } => {
            to_json_binary(&query_available_for_withdraw(deps, env, denom)?)
                .map_err(ContractError::Std)
        }
        AdapterInterfaceQueryMsg::TimeToWithdraw { .. } => {
            to_json_binary(&query_time_to_withdraw()).map_err(ContractError::Std)
        }
        AdapterInterfaceQueryMsg::AllPositions {} => {
            to_json_binary(&query_all_positions()).map_err(ContractError::Std)
        }
        AdapterInterfaceQueryMsg::DepositorPosition { .. } => {
            to_json_binary(&query_depositor_position()).map_err(ContractError::Std)
        }
        AdapterInterfaceQueryMsg::DepositorPositions { .. } => {
            to_json_binary(&query_depositor_positions()).map_err(ContractError::Std)
        }
    }
}

fn dispatch_custom_query(
    deps: Deps,
    query: DAssetAdapterQueryMsg,
) -> Result<Binary, ContractError> {
    match query {
        DAssetAdapterQueryMsg::TokenConfig { symbol } => {
            to_json_binary(&query_token_config(deps, symbol)?).map_err(ContractError::Std)
        }
        DAssetAdapterQueryMsg::AllTokens {} => {
            to_json_binary(&query_tokens(deps)?).map_err(ContractError::Std)
        }
        DAssetAdapterQueryMsg::Executors {} => {
            to_json_binary(&query_executors(deps)?).map_err(ContractError::Std)
        }
    }
}

fn query_config(deps: Deps) -> Result<ConfigResponse, ContractError> {
    let admins = ADMINS.load(deps.storage)?;

    Ok(ConfigResponse {
        admins: admins.iter().map(|a| a.to_string()).collect(),
    })
}

fn query_executors(deps: Deps) -> Result<ExecutorsResponse, ContractError> {
    let executors = EXECUTORS.load(deps.storage)?;

    Ok(ExecutorsResponse {
        executors: executors.iter().map(|a| a.to_string()).collect(),
    })
}

fn query_token_config(deps: Deps, symbol: String) -> Result<TokenConfigResponse, ContractError> {
    let config = TOKEN_REGISTRY.may_load(deps.storage, &symbol)?.ok_or(
        ContractError::TokenNotRegisteredBySymbol {
            symbol: symbol.clone(),
        },
    )?;

    Ok(TokenConfigResponse {
        symbol,
        enabled: config.enabled,
        denom: config.denom,
        drop_staking_core: config.drop_staking_core.to_string(),
        drop_voucher: config.drop_voucher.to_string(),
        drop_withdrawal_manager: config.drop_withdrawal_manager.to_string(),
        base_asset_denom: config.base_asset_denom,
    })
}

fn query_tokens(deps: Deps) -> Result<TokensResponse, ContractError> {
    let tokens: Vec<TokenConfigResponse> = TOKEN_REGISTRY
        .range(deps.storage, None, None, Order::Ascending)
        .map(|res| {
            let (symbol, config) = res?;
            Ok(TokenConfigResponse {
                symbol,
                enabled: config.enabled,
                denom: config.denom,
                drop_staking_core: config.drop_staking_core.to_string(),
                drop_voucher: config.drop_voucher.to_string(),
                drop_withdrawal_manager: config.drop_withdrawal_manager.to_string(),
                base_asset_denom: config.base_asset_denom,
            })
        })
        .collect::<Result<_, ContractError>>()?;

    Ok(TokensResponse { tokens })
}

fn query_registered_depositors(
    deps: Deps,
    enabled_filter: Option<bool>,
) -> Result<RegisteredDepositorsResponse, ContractError> {
    let depositors: Vec<RegisteredDepositorInfo> = WHITELISTED_DEPOSITORS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|res| {
            res.ok().and_then(|(addr, depositor)| match enabled_filter {
                Some(filter) if depositor.enabled != filter => None,
                _ => Some(RegisteredDepositorInfo {
                    depositor_address: addr.to_string(),
                    enabled: depositor.enabled,
                }),
            })
        })
        .collect();

    Ok(RegisteredDepositorsResponse { depositors })
}

/// Query available amount for deposit (no cap for dAsset adapter)
fn query_available_for_deposit() -> AvailableAmountResponse {
    AvailableAmountResponse {
        amount: Uint128::MAX,
    }
}

/// Query available amount for withdrawal (adapter balance)
fn query_available_for_withdraw(
    deps: Deps,
    env: Env,
    denom: String,
) -> Result<AvailableAmountResponse, ContractError> {
    let balance = deps.querier.query_balance(env.contract.address, denom)?;
    Ok(AvailableAmountResponse {
        amount: balance.amount,
    })
}

/// Query time to withdraw (returns 0 - actual unbonding time depends on Drop protocol)
fn query_time_to_withdraw() -> TimeEstimateResponse {
    TimeEstimateResponse {
        blocks: 0,
        seconds: 0,
    }
}

/// Query all positions (returns empty for balance-based tracking)
fn query_all_positions() -> AllPositionsResponse {
    AllPositionsResponse { positions: vec![] }
}

/// Query depositor position (returns zero for balance-based tracking)
fn query_depositor_position() -> DepositorPositionResponse {
    DepositorPositionResponse {
        amount: Uint128::zero(),
    }
}

/// Query depositor positions (returns empty for balance-based tracking)
fn query_depositor_positions() -> DepositorPositionsResponse {
    DepositorPositionsResponse { positions: vec![] }
}

// ============================================================================
// Validation Helpers
// ============================================================================

fn validate_executor(deps: &DepsMut, info: &MessageInfo) -> Result<(), ContractError> {
    let executors = EXECUTORS.load(deps.storage)?;
    if !executors.contains(&info.sender) {
        return Err(ContractError::UnauthorizedExecutor {});
    }
    Ok(())
}

fn validate_admin(deps: &DepsMut, info: &MessageInfo) -> Result<(), ContractError> {
    let admins = ADMINS.load(deps.storage)?;
    if !admins.contains(&info.sender) {
        return Err(ContractError::UnauthorizedAdmin {});
    }
    Ok(())
}

fn validate_depositor_caller(deps: &DepsMut, info: &MessageInfo) -> Result<(), ContractError> {
    let depositor = WHITELISTED_DEPOSITORS
        .may_load(deps.storage, &info.sender)?
        .ok_or(ContractError::DepositorNotWhitelisted {
            address: info.sender.to_string(),
        })?;

    if !depositor.enabled {
        return Err(ContractError::DepositorDisabled {
            address: info.sender.to_string(),
        });
    }

    Ok(())
}
