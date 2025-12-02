use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Order, Reply,
    Response, StdError, StdResult, SubMsg, Uint128,
};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::mars;
use crate::msg::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AllPositionsResponse, AvailableAmountResponse,
    DepositorPositionResponse, DepositorPositionsResponse, ExecuteMsg, InstantiateMsg,
    MarsAdapterMsg, MarsAdapterQueryMsg, MarsConfigResponse, QueryMsg, RegisteredDepositorInfo,
    RegisteredDepositorsResponse, TimeEstimateResponse,
};
use crate::state::{
    Config, Depositor, ADMINS, CONFIG, PENDING_DEPOSITOR_SETUP, WHITELISTED_DEPOSITORS,
};

/// Contract name that is used for migration
const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Reply ID for CreateCreditAccount submessage during instantiation
const REPLY_CREATE_ACCOUNT_INSTANTIATE: u64 = 1;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
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

    // Validate Mars contract address
    let mars_contract = deps.api.addr_validate(&msg.mars_contract)?;

    // Save configuration
    let config = Config {
        mars_contract: mars_contract.clone(),
        supported_denoms: msg.supported_denoms.clone(),
    };
    CONFIG.save(deps.storage, &config)?;

    let mut response = Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("mars_contract", mars_contract.to_string())
        .add_attribute("supported_denoms", msg.supported_denoms.join(", "));

    // If a depositor address is provided, create a Mars account for it
    if let Some(depositor_address) = msg.depositor_address {
        let sub_msg = create_depositor_account(deps, depositor_address.clone())?;

        response = response
            .add_submessage(sub_msg)
            .add_attribute("depositor_address", depositor_address);
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

    // Store pending setup info
    PENDING_DEPOSITOR_SETUP.save(deps.storage, &depositor_addr)?;

    // Initialize with empty account ID (will be set in reply handler) and enabled=true
    let depositor = Depositor {
        mars_account_id: String::new(),
        enabled: true,
    };
    WHITELISTED_DEPOSITORS.save(deps.storage, depositor_addr.clone(), &depositor)?;

    // Create Mars account. We use "default" as the account_kind
    let create_account_msg =
        mars::create_mars_account_msg(config.mars_contract, Some("default".to_string()))?;

    let sub_msg = SubMsg::reply_on_success(create_account_msg, REPLY_CREATE_ACCOUNT_INSTANTIATE);

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
        ExecuteMsg::Interface(interface_msg) => {
            dispatch_execute_interface(deps, env, info, interface_msg)
        }
        ExecuteMsg::Custom(custom_msg) => dispatch_execute_custom(deps, info, custom_msg),
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
        AdapterInterfaceMsg::ToggleDepositorEnabled {
            depositor_address,
            enabled,
        } => execute_toggle_depositor_enabled(deps, env, info, depositor_address, enabled),
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
            mars_contract,
            supported_denoms,
        } => execute_update_config(deps, mars_contract, supported_denoms),
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
        return Err(ContractError::Unauthorized {});
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
        mars::create_mars_deposit_lend_msg(config.mars_contract, account_id, coin.clone())?;

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
        &config.mars_contract,
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
        config.mars_contract,
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
/// IMPORTANT: Changing mars_contract could prevent access to funds if not done carefully
fn execute_update_config(
    deps: DepsMut,
    mars_contract: Option<String>,
    supported_denoms: Option<Vec<String>>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut response = Response::new().add_attribute("action", "update_config");

    // Update mars_contract if provided
    if let Some(mars_contract) = mars_contract {
        config.mars_contract = deps.api.addr_validate(&mars_contract)?;
        response = response.add_attribute("new_mars_contract", mars_contract);
    }

    // Update supported_denoms if provided
    if let Some(supported_denoms) = supported_denoms {
        if supported_denoms.is_empty() {
            return Err(ContractError::AtLeastOneDenom {});
        }
        config.supported_denoms = supported_denoms.clone();
        response = response.add_attribute("new_supported_denoms", supported_denoms.join(","));
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(response)
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
fn execute_toggle_depositor_enabled(
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
        QueryMsg::Interface(interface_msg) => dispatch_query_interface(deps, interface_msg),
        QueryMsg::Custom(custom_msg) => dispatch_query_custom(deps, custom_msg),
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
        mars_contract: config.mars_contract.to_string(),
        supported_denoms: config.supported_denoms,
    })
}

fn query_available_for_deposit(
    _deps: Deps,
    _depositor_address: String,
    _denom: String,
) -> StdResult<AvailableAmountResponse> {
    // For Mars lending, there's typically no hard cap
    // Return a very large number to indicate no practical limit
    // This can be refined later to query actual Mars deposit caps
    Ok(AvailableAmountResponse {
        amount: Uint128::MAX,
    })
}

/// As Mars lending allows immediate withdrawal, we can actually return
/// the same amount as the depositor's current position
fn query_available_for_withdraw(
    deps: Deps,
    depositor_address: String,
    denom: String,
) -> StdResult<AvailableAmountResponse> {
    let depositor_position = query_depositor_position(deps, depositor_address, denom)?;

    Ok(AvailableAmountResponse {
        amount: depositor_position.amount,
    })
}

fn query_time_to_withdraw(
    _deps: Deps,
    _depositor_address: String,
    _coin: Coin,
) -> StdResult<TimeEstimateResponse> {
    // Mars lending allows instant withdrawals
    Ok(TimeEstimateResponse {
        blocks: 0,
        seconds: 0,
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
    use std::collections::HashMap;

    let config = CONFIG.load(deps.storage)?;

    // HashMap to aggregate positions by denom
    let mut aggregated_positions: HashMap<String, Uint128> = HashMap::new();

    // Iterate through all whitelisted depositors
    for entry in WHITELISTED_DEPOSITORS.range(deps.storage, None, None, Order::Ascending) {
        let (_addr, depositor) = entry?;

        // Query Mars positions for this depositor
        let positions = mars::query_mars_positions(
            &deps.querier,
            &config.mars_contract,
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
        &config.mars_contract,
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
        &config.mars_contract,
        depositor.mars_account_id,
    )?;

    Ok(DepositorPositionsResponse {
        positions: positions.lends,
    })
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        REPLY_CREATE_ACCOUNT_INSTANTIATE => handle_create_account_instantiate_reply(deps, msg),
        _ => Err(ContractError::Std(cosmwasm_std::StdError::generic_err(
            format!("Unknown reply ID: {}", msg.id),
        ))),
    }
}

/// Handler for account creation during instantiation (stores account_id in config)
fn handle_create_account_instantiate_reply(
    deps: DepsMut,
    msg: Reply,
) -> Result<Response, ContractError> {
    // Extract the account_id from Mars's response
    let account_id = parse_account_id_from_reply(&msg)?;

    // Get which depositor this account is for
    let depositor_addr = PENDING_DEPOSITOR_SETUP.load(deps.storage)?;

    // Update the depositor's account ID
    let depositor = Depositor {
        mars_account_id: account_id.clone(),
        enabled: true,
    };
    WHITELISTED_DEPOSITORS.save(deps.storage, depositor_addr.clone(), &depositor)?;

    // Clean up temporary storage
    PENDING_DEPOSITOR_SETUP.remove(deps.storage);

    Ok(Response::new()
        .add_attribute("action", "account_created")
        .add_attribute("depositor", depositor_addr)
        .add_attribute("mars_account_id", account_id))
}

/// Parse account_id from Mars CreateCreditAccount reply
fn parse_account_id_from_reply(reply: &Reply) -> Result<String, ContractError> {
    // Mars returns the token_id in the events (credit accounts are NFTs)
    // Look for a "wasm" event with "action" = "mint" and extract "token_id"
    use cosmwasm_std::SubMsgResult;

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
