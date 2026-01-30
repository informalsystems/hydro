use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Order,
    Response, StdResult, Uint128, WasmMsg,
};
use cw2::set_contract_version;
use interface::{
    inflow_control_center::{
        AccrueFeesResponse, Config, ConfigResponse, DeploymentDirection, ExecuteMsg,
        FeeAccrualInfoResponse, FeeConfig, FeeConfigResponse, PoolInfoResponse, QueryMsg,
        SubvaultsResponse, UpdateConfigData, VaultFeeSharesMinted, WhitelistResponse,
    },
    inflow_vault::{
        ExecuteMsg as VaultExecuteMsg, PoolInfoResponse as VaultPoolInfoResponse,
        QueryMsg as VaultQueryMsg,
    },
};
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

use crate::{
    error::{new_generic_error, ContractError},
    msg::InstantiateMsg,
    state::{
        load_config, load_fee_config, CONFIG, DEPLOYED_AMOUNT, FEE_CONFIG, HIGH_WATER_MARK_PRICE,
        SUBVAULTS, WHITELIST,
    },
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

    // Initialize fee config
    // Fees are enabled when fee_rate > 0
    let fee_config = match msg.fee_config {
        Some(init) => {
            // Validate fee_rate (0-100%)
            if init.fee_rate > Decimal::one() {
                return Err(ContractError::InvalidFeeRate);
            }
            let fee_recipient = deps.api.addr_validate(&init.fee_recipient)?;
            FeeConfig {
                fee_rate: init.fee_rate,
                fee_recipient,
            }
        }
        None => {
            // Default: fees disabled (fee_rate = 0)
            FeeConfig {
                fee_rate: Decimal::zero(),
                fee_recipient: Addr::unchecked(""),
            }
        }
    };

    FEE_CONFIG.save(deps.storage, &fee_config)?;

    // Initialize high-water mark to 1.0
    // This will be updated on first accrual if shares exist
    HIGH_WATER_MARK_PRICE.save(deps.storage, &Decimal::one())?;

    let fee_enabled = !fee_config.fee_rate.is_zero();

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
        )
        .add_attribute("fee_enabled", fee_enabled.to_string()))
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
        ExecuteMsg::UpdateDeployedAmount { amount, direction } => {
            update_deployed_amount(deps, env, info, amount, direction)
        }
        ExecuteMsg::AddToWhitelist { address } => add_to_whitelist(deps, env, info, address),
        ExecuteMsg::RemoveFromWhitelist { address } => {
            remove_from_whitelist(deps, env, info, address)
        }
        ExecuteMsg::AddSubvault { address } => add_subvault(deps, info, address),
        ExecuteMsg::RemoveSubvault { address } => remove_subvault(deps, info, address),
        ExecuteMsg::UpdateConfig { config } => update_config(deps, info, config),
        ExecuteMsg::AccrueFees {} => accrue_fees(deps, env),
        ExecuteMsg::UpdateFeeConfig {
            fee_rate,
            fee_recipient,
        } => update_fee_config(deps, env, info, fee_rate, fee_recipient),
    }
}

fn submit_deployed_amount(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    // Try to accrue fees before updating the deployed amount.
    // This ensures fees are calculated at the correct share price before the value changes.
    // We silently ignore cases where fees cannot be accrued (disabled, no shares, below HWM, dust).
    let mut response = Response::new();
    if let AccrueFeesResult::Accrued {
        msgs,
        response_data,
        total_yield,
        fee_amount,
        current_share_price,
    } = try_accrue_fees_internal(&mut deps, &env)?
    {
        response = response
            .add_messages(msgs)
            .add_attribute("fees_accrued", "true")
            .add_attribute("fee_yield", total_yield.to_string())
            .add_attribute("fee_amount", fee_amount.to_string())
            .add_attribute(
                "fee_shares_minted",
                response_data.total_shares_minted.to_string(),
            )
            .add_attribute("fee_share_price", current_share_price.to_string());
    }

    DEPLOYED_AMOUNT.save(deps.storage, &amount, env.block.height)?;

    Ok(response
        .add_attribute("action", "submit_deployed_amount")
        .add_attribute("sender", info.sender)
        .add_attribute("amount", amount))
}

fn update_deployed_amount(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
    direction: DeploymentDirection,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Only registered sub-vaults can execute this function.
    // This happens when a whitelisted address:
    // - withdraws funds for deployment
    // or
    // - deposits funds from deployment
    // from a sub-vault.
    // This can also happen during adapter deposit/withdraw flows.
    if !SUBVAULTS.has(deps.storage, info.sender.clone()) {
        return Err(ContractError::Unauthorized);
    }

    DEPLOYED_AMOUNT.update(deps.storage, env.block.height, |current_value| {
        let current = current_value.unwrap_or_default();
        match direction {
            DeploymentDirection::Add => current
                .checked_add(amount)
                .map_err(|e| new_generic_error(format!("overflow error: {e}"))),
            DeploymentDirection::Subtract => current
                .checked_sub(amount)
                .map_err(|e| new_generic_error(format!("underflow error: {e}"))),
        }
    })?;

    Ok(Response::new()
        .add_attribute("action", "update_deployed_amount")
        .add_attribute("sender", info.sender)
        .add_attribute("amount", amount)
        .add_attribute("direction", format!("{:?}", direction)))
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

/// Result of attempting to accrue fees internally.
enum AccrueFeesResult {
    /// Fees were successfully accrued
    Accrued {
        msgs: Vec<WasmMsg>,
        response_data: AccrueFeesResponse,
        total_yield: Decimal,
        fee_amount: Decimal,
        current_share_price: Decimal,
    },
    /// Fees are disabled (fee_rate is zero)
    Disabled,
    /// No shares have been issued yet
    NoShares,
    /// Current share price is at or below the high-water mark
    BelowHighWaterMark {
        current_share_price: Decimal,
        high_water_mark_price: Decimal,
    },
    /// Yield is too small to mint any shares (dust)
    DustYield { current_share_price: Decimal },
}

/// Internal helper to accrue fees. Returns the result without building a Response,
/// allowing callers to handle the result appropriately.
fn try_accrue_fees_internal(
    deps: &mut DepsMut<NeutronQuery>,
    env: &Env,
) -> Result<AccrueFeesResult, ContractError> {
    let fee_config = load_fee_config(deps.storage)?;

    // Fees are disabled when fee_rate is zero
    if fee_config.fee_rate.is_zero() {
        return Ok(AccrueFeesResult::Disabled);
    }

    let high_water_mark_price = HIGH_WATER_MARK_PRICE.load(deps.storage)?;

    // Get current pool state
    let pool_info = query_pool_info(&deps.as_ref(), env)?;

    if pool_info.total_shares_issued.is_zero() {
        return Ok(AccrueFeesResult::NoShares);
    }

    // Calculate current share price
    let current_share_price =
        Decimal::from_ratio(pool_info.total_pool_value, pool_info.total_shares_issued);

    // High-water mark check: only charge fees on gains above the last accrual price
    if current_share_price <= high_water_mark_price {
        return Ok(AccrueFeesResult::BelowHighWaterMark {
            current_share_price,
            high_water_mark_price,
        });
    }

    // Calculate fee
    let yield_per_share: Decimal = current_share_price - high_water_mark_price;
    // Convert total_shares to Decimal for multiplication
    let total_shares_decimal = Decimal::from_ratio(pool_info.total_shares_issued, 1u128);
    // total_yield is in base token units (as Decimal)
    let total_yield = yield_per_share * total_shares_decimal;
    let fee_amount = total_yield * fee_config.fee_rate;
    let shares_to_mint = fee_amount / current_share_price;

    // Convert shares_to_mint from Decimal to Uint128
    let shares_to_mint_uint = Uint128::new(shares_to_mint.to_uint_floor().u128());

    // Handle dust case: if the floored value is zero, return early without updating
    // the high water mark so dust can accumulate across multiple accrual calls
    if shares_to_mint_uint.is_zero() {
        return Ok(AccrueFeesResult::DustYield {
            current_share_price,
        });
    }

    // Update high-water mark only after confirming we will mint shares
    // This ensures dust yield accumulates across multiple accrual calls
    HIGH_WATER_MARK_PRICE.save(deps.storage, &current_share_price)?;

    // Get all subvaults with non-zero shares
    let subvaults: Vec<Addr> = SUBVAULTS
        .keys(deps.storage, None, None, Order::Ascending)
        .filter_map(|v| v.ok())
        .collect();

    // Collect vault info for vaults with non-zero shares and track total for invariant check
    let mut vaults_with_shares: Vec<(Addr, Uint128)> = vec![];
    let mut sum_of_vault_shares = Uint128::zero();
    for subvault in subvaults {
        let vault_info: VaultPoolInfoResponse = deps
            .querier
            .query_wasm_smart(subvault.to_string(), &VaultQueryMsg::PoolInfo {})?;

        sum_of_vault_shares = sum_of_vault_shares.checked_add(vault_info.shares_issued)?;
        if !vault_info.shares_issued.is_zero() {
            vaults_with_shares.push((subvault, vault_info.shares_issued));
        }
    }

    // Invariant check: ensure pool_info.total_shares_issued equals sum of individual vault shares.
    // This is assumed by the remainder logic below.
    if sum_of_vault_shares != pool_info.total_shares_issued {
        return Err(new_generic_error(format!(
            "shares invariant violated: pool total {} != sum of vaults {}",
            pool_info.total_shares_issued, sum_of_vault_shares
        )));
    }

    // Calculate proportional minting for each subvault with shares
    let mut msgs: Vec<WasmMsg> = vec![];
    let mut total_minted = Uint128::zero();
    let mut vault_mints: Vec<VaultFeeSharesMinted> = vec![];

    for (i, (subvault, shares_issued)) in vaults_with_shares.iter().enumerate() {
        // Calculate this vault's share of the fee shares to mint
        let vault_mint_amount = if i == vaults_with_shares.len() - 1 {
            // Last vault with shares gets the remainder to handle rounding
            shares_to_mint_uint.checked_sub(total_minted)?
        } else {
            shares_to_mint_uint.multiply_ratio(*shares_issued, pool_info.total_shares_issued)
        };

        if vault_mint_amount.is_zero() {
            continue;
        }

        total_minted = total_minted.checked_add(vault_mint_amount)?;

        vault_mints.push(VaultFeeSharesMinted {
            vault: subvault.clone(),
            shares_minted: vault_mint_amount,
        });

        let mint_msg = WasmMsg::Execute {
            contract_addr: subvault.to_string(),
            msg: to_json_binary(&VaultExecuteMsg::MintFeeShares {
                amount: vault_mint_amount,
                recipient: fee_config.fee_recipient.to_string(),
            })?,
            funds: vec![],
        };
        msgs.push(mint_msg);
    }

    let response_data = AccrueFeesResponse {
        total_shares_minted: shares_to_mint_uint,
        vaults: vault_mints,
    };

    Ok(AccrueFeesResult::Accrued {
        msgs,
        response_data,
        total_yield,
        fee_amount,
        current_share_price,
    })
}

/// Accrues performance fees based on yield since last accrual.
/// This is a permissionless operation - anyone can call it.
/// Fees are only accrued if fee_rate > 0.
fn accrue_fees(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
) -> Result<Response<NeutronMsg>, ContractError> {
    match try_accrue_fees_internal(&mut deps, &env)? {
        AccrueFeesResult::Disabled => Err(ContractError::FeeAccrualDisabled),
        AccrueFeesResult::NoShares => Err(ContractError::NoSharesIssued),
        AccrueFeesResult::BelowHighWaterMark {
            current_share_price,
            high_water_mark_price,
        } => Ok(Response::new()
            .add_attribute("action", "accrue_fees")
            .add_attribute("result", "below_high_water_mark")
            .add_attribute("current_share_price", current_share_price.to_string())
            .add_attribute("high_water_mark_price", high_water_mark_price.to_string())),
        AccrueFeesResult::DustYield {
            current_share_price,
        } => Ok(Response::new()
            .add_attribute("action", "accrue_fees")
            .add_attribute("result", "dust_yield")
            .add_attribute("current_share_price", current_share_price.to_string())),
        AccrueFeesResult::Accrued {
            msgs,
            response_data,
            total_yield,
            fee_amount,
            current_share_price,
        } => Ok(Response::new()
            .add_messages(msgs)
            .set_data(to_json_binary(&response_data)?)
            .add_attribute("action", "accrue_fees")
            .add_attribute("result", "fees_accrued")
            .add_attribute("yield", total_yield.to_string())
            .add_attribute("fee_amount", fee_amount.to_string())
            .add_attribute(
                "shares_minted",
                response_data.total_shares_minted.to_string(),
            )
            .add_attribute("current_share_price", current_share_price.to_string())),
    }
}

/// Updates the fee configuration. Only whitelisted addresses can call this.
/// Set fee_rate to 0 to disable fee accrual.
/// When re-enabling fees (transitioning from rate=0 to rate>0), the high-water
/// mark is reset to the current share price to avoid charging fees on yield
/// that occurred while fees were disabled.
fn update_fee_config(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    fee_rate: Option<Decimal>,
    fee_recipient: Option<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    let mut fee_config = load_fee_config(deps.storage)?;

    if let Some(rate) = fee_rate {
        if rate > Decimal::one() {
            return Err(ContractError::InvalidFeeRate);
        }
        // If setting a non-zero rate, ensure we have a valid recipient
        // (either from this update or from existing config)
        let has_valid_recipient = fee_recipient.as_ref().is_some_and(|r| !r.is_empty())
            || !fee_config.fee_recipient.as_str().is_empty();
        if !rate.is_zero() && !has_valid_recipient {
            return Err(ContractError::FeeRecipientNotSet);
        }

        // If enabling fees (transitioning from zero to non-zero rate),
        // set high-water mark to max(existing HWM, current share price).
        // This avoids charging fees on yield that occurred while fees were disabled,
        // while preserving the actual high-water mark if the price has dropped.
        if fee_config.fee_rate.is_zero() && !rate.is_zero() {
            let existing_hwm = HIGH_WATER_MARK_PRICE.load(deps.storage)?;
            let pool_info = query_pool_info(&deps.as_ref(), &env)?;
            let current_share_price = if pool_info.total_shares_issued.is_zero() {
                Decimal::one()
            } else {
                Decimal::from_ratio(pool_info.total_pool_value, pool_info.total_shares_issued)
            };
            let new_hwm = existing_hwm.max(current_share_price);
            HIGH_WATER_MARK_PRICE.save(deps.storage, &new_hwm)?;
        }

        fee_config.fee_rate = rate;
    }

    if let Some(recipient) = fee_recipient {
        fee_config.fee_recipient = deps.api.addr_validate(&recipient)?;
    }

    FEE_CONFIG.save(deps.storage, &fee_config)?;

    let fee_enabled = !fee_config.fee_rate.is_zero();

    Ok(Response::new()
        .add_attribute("action", "update_fee_config")
        .add_attribute("sender", info.sender)
        .add_attribute("fee_rate", fee_config.fee_rate.to_string())
        .add_attribute("fee_recipient", fee_config.fee_recipient.to_string())
        .add_attribute("fee_enabled", fee_enabled.to_string()))
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
        QueryMsg::FeeConfig {} => to_json_binary(&query_fee_config(&deps)?),
        QueryMsg::FeeAccrualInfo {} => to_json_binary(&query_fee_accrual_info(&deps, &env)?),
    }
}

fn query_config(deps: &Deps<NeutronQuery>) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: load_config(deps.storage)?,
    })
}

pub fn query_pool_info(deps: &Deps<NeutronQuery>, _env: &Env) -> StdResult<PoolInfoResponse> {
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

fn query_fee_config(deps: &Deps<NeutronQuery>) -> StdResult<FeeConfigResponse> {
    let fee_config = load_fee_config(deps.storage)?;
    Ok(FeeConfigResponse {
        fee_rate: fee_config.fee_rate,
        fee_recipient: fee_config.fee_recipient,
    })
}

fn query_fee_accrual_info(
    deps: &Deps<NeutronQuery>,
    env: &Env,
) -> StdResult<FeeAccrualInfoResponse> {
    let fee_config = load_fee_config(deps.storage)?;
    let high_water_mark_price = HIGH_WATER_MARK_PRICE.load(deps.storage)?;
    let pool_info = query_pool_info(deps, env)?;

    // Calculate current share price (handle zero shares case)
    let current_share_price = if pool_info.total_shares_issued.is_zero() {
        Decimal::one()
    } else {
        Decimal::from_ratio(pool_info.total_pool_value, pool_info.total_shares_issued)
    };

    // Calculate pending yield and fee
    let (pending_yield, pending_fee) = if current_share_price > high_water_mark_price {
        let yield_per_share = current_share_price - high_water_mark_price;
        let total_shares_decimal = Decimal::from_ratio(pool_info.total_shares_issued, 1u128);
        let total_yield = yield_per_share * total_shares_decimal;
        let fee_amount = total_yield * fee_config.fee_rate;
        (
            Uint128::new(total_yield.to_uint_floor().u128()),
            Uint128::new(fee_amount.to_uint_floor().u128()),
        )
    } else {
        (Uint128::zero(), Uint128::zero())
    };

    Ok(FeeAccrualInfoResponse {
        high_water_mark_price,
        current_share_price,
        pending_yield,
        pending_fee,
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
