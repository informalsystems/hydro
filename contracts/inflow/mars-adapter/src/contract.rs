use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Order, Reply,
    Response, StdError, StdResult, SubMsg, SubMsgResult, Uint128,
};
use cw2::set_contract_version;
use std::collections::HashMap;

use crate::error::ContractError;
use crate::mars;
use crate::msg::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AllPositionsResponse, AvailableAmountResponse,
    DepositorPositionResponse, DepositorPositionsResponse, ExecuteMsg, InstantiateMsg,
    MarsAdapterMsg, MarsAdapterQueryMsg, MarsConfigResponse, QueryMsg, RegisteredDepositorInfo,
    RegisteredDepositorsResponse, TimeEstimateResponse,
};
use crate::state::{Config, Depositor, ADMINS, CONFIG, PENDING_DEPOSITORS, WHITELISTED_DEPOSITORS};

/// Contract name that is used for migration
const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Reply ID for CreateCreditAccount submessage
const REPLY_CREATE_CREDIT_ACCOUNT_ID: u64 = 1;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    mut deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let mut admins: Vec<Addr> = vec![];

    for admin in msg.admins {
        let admin_addr = deps.api.addr_validate(&admin)?;
        if !admins.contains(&admin_addr) {
            admins.push(admin_addr.clone());
        }
    }

    // Ensure at least one admin is provided
    if admins.is_empty() {
        return Err(ContractError::AtLeastOneAdmin {});
    }

    ADMINS.save(deps.storage, &admins)?;

    // Ensure at least one supported denom is provided
    if msg.supported_denoms.is_empty() {
        return Err(ContractError::AtLeastOneDenom {});
    }

    // Validate Mars contract addresses
    let mars_credit_manager = deps.api.addr_validate(&msg.mars_credit_manager)?;
    let mars_params = deps.api.addr_validate(&msg.mars_params)?;
    let mars_red_bank = deps.api.addr_validate(&msg.mars_red_bank)?;

    // Save configuration
    let config = Config {
        mars_credit_manager: mars_credit_manager.clone(),
        mars_params,
        mars_red_bank,
        supported_denoms: msg.supported_denoms.clone(),
    };
    CONFIG.save(deps.storage, &config)?;

    let mut response = Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("mars_credit_manager", mars_credit_manager.to_string())
        .add_attribute("supported_denoms", msg.supported_denoms.join(", "));

    // Create Mars accounts for initial depositors
    for depositor_address in msg.initial_depositors {
        let sub_msg = create_depositor_account(deps.branch(), depositor_address.clone())?;

        response = response
            .add_submessage(sub_msg)
            .add_attribute("initial_depositor", depositor_address);
    }

    Ok(response)
}

/// Creates a Mars account for a depositor address and returns the SubMsg for account creation
fn create_depositor_account(
    deps: DepsMut,
    depositor_address: String,
) -> Result<SubMsg, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let depositor_addr = deps.api.addr_validate(&depositor_address)?;

    // Check if already registered
    if WHITELISTED_DEPOSITORS.has(deps.storage, depositor_addr.clone()) {
        return Err(ContractError::DepositorAlreadyRegistered {
            depositor_address: depositor_addr.to_string(),
        });
    }

    // Add to pending depositors list
    let mut pending = PENDING_DEPOSITORS
        .may_load(deps.storage)?
        .unwrap_or_default();
    pending.push(depositor_addr.clone());
    PENDING_DEPOSITORS.save(deps.storage, &pending)?;

    // Initialize with empty account ID (will be set in reply handler) and enabled=true
    let depositor = Depositor {
        mars_account_id: String::new(),
        enabled: true,
    };
    WHITELISTED_DEPOSITORS.save(deps.storage, depositor_addr.clone(), &depositor)?;

    // Create Mars account. We use "default" as the account_kind
    let create_account_msg =
        mars::create_mars_account_msg(config.mars_credit_manager, Some("default".to_string()))?;

    let sub_msg = SubMsg::reply_on_success(create_account_msg, REPLY_CREATE_CREDIT_ACCOUNT_ID);

    Ok(sub_msg)
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
            dispatch_execute_interface(deps, env, info, interface_msg)
        }
        ExecuteMsg::CustomAction(custom_msg) => dispatch_execute_custom(deps, info, custom_msg),
    }
}

/// Dispatch standard adapter interface messages
fn dispatch_execute_interface(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: AdapterInterfaceMsg,
) -> Result<Response, ContractError> {
    match msg {
        AdapterInterfaceMsg::Deposit {} => execute_deposit(deps, env, info),
        AdapterInterfaceMsg::Withdraw { coin } => execute_withdraw(deps, env, info, coin),
        AdapterInterfaceMsg::RegisterDepositor {
            depositor_address,
            metadata: _,
        } => execute_register_depositor(deps, env, info, depositor_address),
        AdapterInterfaceMsg::UnregisterDepositor { depositor_address } => {
            execute_unregister_depositor(deps, env, info, depositor_address)
        }
        AdapterInterfaceMsg::SetDepositorEnabled {
            depositor_address,
            enabled,
        } => execute_set_depositor_enabled(deps, env, info, depositor_address, enabled),
    }
}

/// Dispatch Mars adapter-specific custom messages
fn dispatch_execute_custom(
    deps: DepsMut,
    info: MessageInfo,
    msg: MarsAdapterMsg,
) -> Result<Response, ContractError> {
    // All custom operations require admin
    validate_admin_caller(&deps, &info)?;

    match msg {
        MarsAdapterMsg::UpdateConfig {
            mars_credit_manager,
            mars_params,
            mars_red_bank,
            supported_denoms,
        } => execute_update_config(
            deps,
            mars_credit_manager,
            mars_params,
            mars_red_bank,
            supported_denoms,
        ),
    }
}

/// Retrieves the depositor info for a given address or return DepositorNotRegistered error
fn get_depositor(deps: Deps, depositor_addr: Addr) -> Result<Depositor, ContractError> {
    WHITELISTED_DEPOSITORS
        .load(deps.storage, depositor_addr.clone())
        .map_err(|_| ContractError::DepositorNotRegistered {
            depositor_address: depositor_addr.to_string(),
        })
}

/// Validates that the caller is a registered and enabled depositor
fn validate_depositor_caller(
    deps: &DepsMut,
    info: &MessageInfo,
) -> Result<Depositor, ContractError> {
    let depositor = get_depositor(deps.as_ref(), info.sender.clone())
        .map_err(|_| ContractError::Unauthorized {})?;

    // Check if depositor is enabled
    if !depositor.enabled {
        return Err(ContractError::Unauthorized {});
    }

    Ok(depositor)
}

fn validate_admin_caller(deps: &DepsMut, info: &MessageInfo) -> Result<(), ContractError> {
    let admins = ADMINS.load(deps.storage)?;

    if !admins.contains(&info.sender) {
        return Err(ContractError::UnauthorizedAdmin {});
    }

    Ok(())
}

fn execute_deposit(deps: DepsMut, _env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    let depositor = validate_depositor_caller(&deps, &info)?;

    // Must send exactly one coin
    if info.funds.len() != 1 {
        return Err(ContractError::InvalidFunds {
            count: info.funds.len(),
        });
    }

    let coin = &info.funds[0];

    // Check for zero amount
    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // Load config to validate denom and get Mars contract + shared account_id
    let config = CONFIG.load(deps.storage)?;

    // Validate denom is supported
    if !config.supported_denoms.contains(&coin.denom) {
        return Err(ContractError::UnsupportedDenom {
            denom: coin.denom.clone(),
        });
    }

    // Use the Mars account for this depositor address
    let account_id = depositor.mars_account_id;

    // Create Mars UpdateCreditAccount message with Deposit + Lend actions
    let mars_msg =
        mars::create_mars_deposit_lend_msg(config.mars_credit_manager, account_id, coin.clone())?;

    Ok(Response::new()
        .add_message(mars_msg)
        .add_attribute("action", "deposit")
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", &coin.denom))
}

fn execute_withdraw(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    coin: Coin,
) -> Result<Response, ContractError> {
    let depositor = validate_depositor_caller(&deps, &info)?;

    // Check for zero amount
    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // Load config to validate denom and get Mars contract + shared account_id
    let config = CONFIG.load(deps.storage)?;

    // Validate denom is supported
    if !config.supported_denoms.contains(&coin.denom) {
        return Err(ContractError::UnsupportedDenom {
            denom: coin.denom.clone(),
        });
    }

    // Use the Mars account for this depositor address
    let account_id = depositor.mars_account_id.clone();

    // Query Mars to get the total lent position in the shared account
    // Note: The depositor is responsible for ensuring users can only
    // withdraw amounts they're entitled to based on their share tokens.
    // This adapter just checks that the account has sufficient funds.
    let lent_amount = mars::get_lent_amount_for_denom(
        &deps.querier,
        &config.mars_credit_manager,
        account_id.clone(),
        &coin.denom,
    )?;

    // Check the shared account has sufficient lent amount to withdraw
    if lent_amount < coin.amount {
        return Err(ContractError::InsufficientBalance {});
    }

    // Create Mars UpdateCreditAccount message with Reclaim + WithdrawToWallet actions
    // Funds are sent back to the depositor
    let mars_msg = mars::create_mars_reclaim_withdraw_msg(
        config.mars_credit_manager,
        account_id,
        coin.clone(),
        info.sender.clone(),
    )?;

    Ok(Response::new()
        .add_message(mars_msg)
        .add_attribute("action", "withdraw")
        .add_attribute("recipient", info.sender)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", coin.denom))
}

/// Update Mars adapter configuration (admin-only)
/// IMPORTANT: Changing Mars contract addresses could prevent access to funds if not done carefully
fn execute_update_config(
    deps: DepsMut,
    mars_credit_manager: Option<String>,
    mars_params: Option<String>,
    mars_red_bank: Option<String>,
    supported_denoms: Option<Vec<String>>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    // Update Mars Credit Manager if provided
    if let Some(mars_credit_manager_addr) = mars_credit_manager {
        config.mars_credit_manager = deps.api.addr_validate(&mars_credit_manager_addr)?;
    }

    // Update Mars Params if provided
    if let Some(mars_params_addr) = mars_params {
        config.mars_params = deps.api.addr_validate(&mars_params_addr)?;
    }

    // Update Mars Red Bank if provided
    if let Some(mars_red_bank_addr) = mars_red_bank {
        config.mars_red_bank = deps.api.addr_validate(&mars_red_bank_addr)?;
    }

    // Update supported denoms if provided
    if let Some(denoms) = supported_denoms {
        if denoms.is_empty() {
            return Err(ContractError::AtLeastOneDenom {});
        }
        config.supported_denoms = denoms;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("action", "update_config")
        .add_attribute(
            "mars_credit_manager",
            config.mars_credit_manager.to_string(),
        )
        .add_attribute("mars_params", config.mars_params.to_string())
        .add_attribute("mars_red_bank", config.mars_red_bank.to_string()))
}

/// Registers a new depositor with its Mars account ID
fn execute_register_depositor(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    depositor_address: String,
) -> Result<Response, ContractError> {
    // Only an admin can register depositors
    validate_admin_caller(&deps, &info)?;

    let sub_msg = create_depositor_account(deps, depositor_address.clone())?;

    Ok(Response::new()
        .add_submessage(sub_msg)
        .add_attribute("action", "register_depositor")
        .add_attribute("depositor_address", depositor_address))
}

/// Unregisters a depositor
/// This function should only be executed on empty accounts or it could lead to loss of funds
fn execute_unregister_depositor(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    depositor_address: String,
) -> Result<Response, ContractError> {
    // Only an admin can unregister depositors
    validate_admin_caller(&deps, &info)?;

    let depositor_addr = deps.api.addr_validate(&depositor_address)?;

    // Load to ensure it exists (will error if not found)
    let depositor = WHITELISTED_DEPOSITORS
        .load(deps.storage, depositor_addr.clone())
        .map_err(|_| ContractError::DepositorNotRegistered {
            depositor_address: depositor_addr.to_string(),
        })?;

    WHITELISTED_DEPOSITORS.remove(deps.storage, depositor_addr.clone());

    Ok(Response::new()
        .add_attribute("action", "unregister_depositor")
        .add_attribute("depositor_address", depositor_addr.to_string())
        .add_attribute("mars_account_id", depositor.mars_account_id))
}

/// Toggles whether a depositor is enabled or disabled
fn execute_set_depositor_enabled(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    depositor_address: String,
    enabled: bool,
) -> Result<Response, ContractError> {
    // Only an admin can toggle depositors enabled status
    validate_admin_caller(&deps, &info)?;

    let depositor_addr = deps.api.addr_validate(&depositor_address)?;

    // Load existing depositor
    let mut depositor = WHITELISTED_DEPOSITORS
        .load(deps.storage, depositor_addr.clone())
        .map_err(|_| ContractError::DepositorNotRegistered {
            depositor_address: depositor_addr.to_string(),
        })?;

    // Update enabled status
    depositor.enabled = enabled;
    WHITELISTED_DEPOSITORS.save(deps.storage, depositor_addr.clone(), &depositor)?;

    Ok(Response::new()
        .add_attribute("action", "toggle_depositor_enabled")
        .add_attribute("depositor_address", depositor_addr.to_string())
        .add_attribute("enabled", enabled.to_string()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::StandardQuery(interface_msg) => dispatch_query_interface(deps, interface_msg),
        QueryMsg::CustomQuery(custom_msg) => dispatch_query_custom(deps, custom_msg),
    }
}

/// Dispatch standard adapter interface queries
fn dispatch_query_interface(deps: Deps, msg: AdapterInterfaceQueryMsg) -> StdResult<Binary> {
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
            depositor_address,
            denom,
        )?),
        AdapterInterfaceQueryMsg::TimeToWithdraw {
            depositor_address,
            coin,
        } => to_json_binary(&query_time_to_withdraw(deps, depositor_address, coin)?),
        AdapterInterfaceQueryMsg::RegisteredDepositors { enabled } => {
            to_json_binary(&query_registered_depositors(deps, enabled)?)
        }
        AdapterInterfaceQueryMsg::AllPositions {} => to_json_binary(&query_all_positions(deps)?),
        AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address,
            denom,
        } => to_json_binary(&query_depositor_position(deps, depositor_address, denom)?),
        AdapterInterfaceQueryMsg::DepositorPositions { depositor_address } => {
            to_json_binary(&query_depositor_positions(deps, depositor_address)?)
        }
    }
}

/// Dispatch Mars adapter-specific custom queries
fn dispatch_query_custom(_deps: Deps, msg: MarsAdapterQueryMsg) -> StdResult<Binary> {
    match msg {
        // No custom queries yet, but the match is required
    }
}

fn query_config(deps: Deps) -> StdResult<MarsConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    let admins = ADMINS.load(deps.storage)?;

    Ok(MarsConfigResponse {
        admins: admins.iter().map(|a| a.to_string()).collect(),
        mars_credit_manager: config.mars_credit_manager.to_string(),
        mars_params: config.mars_params.to_string(),
        mars_red_bank: config.mars_red_bank.to_string(),
        supported_denoms: config.supported_denoms,
    })
}

fn query_available_for_deposit(
    deps: Deps,
    depositor_address: String,
    denom: String,
) -> StdResult<AvailableAmountResponse> {
    let config = CONFIG.load(deps.storage)?;

    // Ensure depositor is registered
    let depositor_addr = deps.api.addr_validate(&depositor_address)?;
    get_depositor(deps, depositor_addr).map_err(|e| StdError::generic_err(e.to_string()))?;

    // Query Mars Params for total deposit cap and current amount
    let total_deposit = mars::query_mars_total_deposit(&deps.querier, &config.mars_params, denom)?;

    // Available = cap - amount (saturating to prevent underflow)
    let available = total_deposit.cap.saturating_sub(total_deposit.amount);

    Ok(AvailableAmountResponse { amount: available })
}

fn get_red_bank_balance(deps: Deps, denom: String) -> StdResult<Uint128> {
    let config = CONFIG.load(deps.storage)?;

    // Query Red Bank balance for this denom using standard bank query
    let red_bank_balance = deps
        .querier
        .query_balance(config.mars_red_bank.to_string(), denom)?;

    Ok(red_bank_balance.amount)
}

fn query_available_for_withdraw(
    deps: Deps,
    depositor_address: String,
    denom: String,
) -> StdResult<AvailableAmountResponse> {
    // Get depositor's position in Mars Credit Manager
    let depositor_position = query_depositor_position(deps, depositor_address, denom.clone())?;

    let balance_amount = get_red_bank_balance(deps, denom)?;

    // Return minimum of depositor position and Red Bank liquidity
    let amount = depositor_position.amount.min(balance_amount);

    Ok(AvailableAmountResponse { amount })
}

fn query_time_to_withdraw(
    deps: Deps,
    depositor_address: String,
    coin: Coin,
) -> StdResult<TimeEstimateResponse> {
    // Get depositor's position in Mars Credit Manager
    let depositor_position = query_depositor_position(deps, depositor_address, coin.denom.clone())?;

    // If depositor's position is less than requested amount, return error
    if depositor_position.amount < coin.amount {
        return Err(StdError::generic_err(
            "Depositor position insufficient for requested withdrawal",
        ));
    }

    let balance_amount = get_red_bank_balance(deps, coin.denom)?;

    // If sufficient liquidity is available, return zero time estimate
    if balance_amount >= coin.amount {
        return Ok(TimeEstimateResponse {
            blocks: 0,
            seconds: 0,
        });
    }

    // If liquidity is constrained, return a conservative 1-week estimate
    // This is the time it might take for liquidity to become available through:
    // - Borrowers repaying debts
    // - New lenders providing liquidity
    // 1 week = 7 days * 24 hours * 60 minutes * 60 seconds = 604,800 seconds
    // Assuming 1 block per second
    const ONE_WEEK_SECONDS: u64 = 604_800;
    Ok(TimeEstimateResponse {
        blocks: ONE_WEEK_SECONDS,
        seconds: ONE_WEEK_SECONDS,
    })
}

/// Returns list of registered depositors with their enabled status
/// Optionally filtered by enabled status
fn query_registered_depositors(
    deps: Deps,
    enabled_filter: Option<bool>,
) -> StdResult<RegisteredDepositorsResponse> {
    let depositors: Vec<RegisteredDepositorInfo> = WHITELISTED_DEPOSITORS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|entry| {
            let (addr, depositor) = entry.ok()?;

            // Skip if filter doesn't match
            if enabled_filter.is_some_and(|filter| depositor.enabled != filter) {
                return None;
            }

            Some(RegisteredDepositorInfo {
                depositor_address: addr.to_string(),
                enabled: depositor.enabled,
            })
        })
        .collect();

    Ok(RegisteredDepositorsResponse { depositors })
}

/// This query will go through all depositors registered and compute the sum of funds in each account
fn query_all_positions(deps: Deps) -> StdResult<AllPositionsResponse> {
    let config = CONFIG.load(deps.storage)?;

    // HashMap to aggregate positions by denom
    let mut aggregated_positions: HashMap<String, Uint128> = HashMap::new();

    // Iterate through all whitelisted depositors
    for entry in WHITELISTED_DEPOSITORS.range(deps.storage, None, None, Order::Ascending) {
        let (_addr, depositor) = entry?;

        // Query Mars positions for this depositor
        let positions = mars::query_mars_positions(
            &deps.querier,
            &config.mars_credit_manager,
            depositor.mars_account_id,
        )?;

        // Aggregate the lent positions by denom
        for coin in positions.lends {
            *aggregated_positions
                .entry(coin.denom)
                .or_insert(Uint128::zero()) += coin.amount;
        }
    }

    // Convert HashMap to Vec<Coin>
    let positions: Vec<Coin> = aggregated_positions
        .into_iter()
        .map(|(denom, amount)| Coin { denom, amount })
        .collect();

    Ok(AllPositionsResponse { positions })
}

fn query_depositor_position(
    deps: Deps,
    depositor_address: String,
    denom: String,
) -> StdResult<DepositorPositionResponse> {
    let config = CONFIG.load(deps.storage)?;

    let depositor_addr = deps.api.addr_validate(&depositor_address)?;

    // Retrieve the depositor account
    let depositor =
        get_depositor(deps, depositor_addr).map_err(|e| StdError::generic_err(e.to_string()))?;

    // Query Mars to get the total lent position in the account for the depositor address (includes yield)
    let amount = mars::get_lent_amount_for_denom(
        &deps.querier,
        &config.mars_credit_manager,
        depositor.mars_account_id,
        &denom,
    )?;

    Ok(DepositorPositionResponse { amount })
}

/// This query will return all the positions for a depositor across all denoms
fn query_depositor_positions(
    deps: Deps,
    depositor_address: String,
) -> StdResult<DepositorPositionsResponse> {
    let config = CONFIG.load(deps.storage)?;

    let depositor_addr = deps.api.addr_validate(&depositor_address)?;

    // Retrieve the depositor info
    let depositor =
        get_depositor(deps, depositor_addr).map_err(|e| StdError::generic_err(e.to_string()))?;

    // Query Mars to get all lent positions in the depositor account (includes yield)
    let positions = mars::query_mars_positions(
        &deps.querier,
        &config.mars_credit_manager,
        depositor.mars_account_id,
    )?;

    Ok(DepositorPositionsResponse {
        positions: positions.lends,
    })
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        REPLY_CREATE_CREDIT_ACCOUNT_ID => handle_create_credit_account_reply(deps, msg),
        _ => Err(ContractError::Std(cosmwasm_std::StdError::generic_err(
            format!("Unknown reply ID: {}", msg.id),
        ))),
    }
}

/// Handler for account creation (stores account_id)
fn handle_create_credit_account_reply(
    deps: DepsMut,
    msg: Reply,
) -> Result<Response, ContractError> {
    // Extract the account_id from Mars's response
    let account_id = parse_account_id_from_reply(&msg)?;

    // Find the first depositor with an empty account_id (they're waiting for this reply)
    let pending = PENDING_DEPOSITORS.load(deps.storage)?;

    let mut depositor_addr: Option<Addr> = None;
    for addr in &pending {
        let depositor = WHITELISTED_DEPOSITORS.load(deps.storage, addr.clone())?;
        if depositor.mars_account_id.is_empty() {
            depositor_addr = Some(addr.clone());
            break;
        }
    }

    let depositor_addr = depositor_addr.ok_or_else(|| {
        ContractError::Std(cosmwasm_std::StdError::generic_err(
            "No pending depositor found for account creation",
        ))
    })?;

    // Update the depositor's account ID
    let depositor = Depositor {
        mars_account_id: account_id.clone(),
        enabled: true,
    };
    WHITELISTED_DEPOSITORS.save(deps.storage, depositor_addr.clone(), &depositor)?;

    // Remove this depositor from pending list
    let pending: Vec<Addr> = pending
        .into_iter()
        .filter(|a| a != depositor_addr)
        .collect();
    if pending.is_empty() {
        PENDING_DEPOSITORS.remove(deps.storage);
    } else {
        PENDING_DEPOSITORS.save(deps.storage, &pending)?;
    }

    Ok(Response::new()
        .add_attribute("action", "account_created")
        .add_attribute("depositor", depositor_addr)
        .add_attribute("mars_account_id", account_id))
}

/// Parse account_id from Mars CreateCreditAccount reply
fn parse_account_id_from_reply(reply: &Reply) -> Result<String, ContractError> {
    // Mars returns the token_id in the events (credit accounts are NFTs)
    // Look for a "wasm" event with "action" = "mint" and extract "token_id"
    let response = match &reply.result {
        SubMsgResult::Ok(response) => response,
        SubMsgResult::Err(err) => {
            return Err(ContractError::MarsProtocolError {
                msg: format!("CreateCreditAccount failed: {}", err),
            });
        }
    };

    for event in &response.events {
        if event.ty == "wasm" {
            // Check if this is a mint action
            let is_mint = event
                .attributes
                .iter()
                .any(|attr| attr.key == "action" && attr.value == "mint");

            if is_mint {
                // Extract token_id from this event
                for attr in &event.attributes {
                    if attr.key == "token_id" {
                        return Ok(attr.value.clone());
                    }
                }
            }
        }
    }

    Err(ContractError::MarsProtocolError {
        msg: "token_id not found in Mars mint event".to_string(),
    })
}
