use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    env, vec,
};

use cosmos_sdk_proto::cosmos::bank::v1beta1::{DenomUnit, Metadata};
use cosmwasm_std::{
    entry_point, from_json, to_json_binary, to_json_vec, Addr, AnyMsg, BankMsg, Binary, Coin,
    ConversionOverflowError, CosmosMsg, Decimal, Deps, DepsMut, Env, Int128, MessageInfo, Order,
    Reply, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;
use interface::{
    inflow_control_center::{
        ConfigResponse as ControlCenterConfigResponse, DeploymentDirection,
        ExecuteMsg as ControlCenterExecuteMsg, PoolInfoResponse as ControlCenterPoolInfoResponse,
        QueryMsg as ControlCenterQueryMsg,
    },
    inflow_vault::{
        AdapterInfo, AdapterInfoResponse, AdaptersListResponse, AllocationMode, Config,
        ConfigResponse, DeploymentTracking, ExecuteMsg, FundedWithdrawalRequestsResponse,
        PayoutEntry, PoolInfoResponse, QueryMsg, UpdateConfigData, UserPayoutsHistoryResponse,
        UserWithdrawalRequestsResponse, WhitelistResponse, WithdrawalEntry, WithdrawalQueueInfo,
        WithdrawalQueueInfoResponse,
    },
    token_info_provider::TokenInfoProviderQueryMsg,
};
use neutron_sdk::{
    bindings::{msg::NeutronMsg, query::NeutronQuery},
    proto_types::osmosis::tokenfactory::v1beta1::MsgSetDenomMetadata,
    query::token_factory::query_full_denom,
};

use prost::Message;

use crate::{
    error::{new_generic_error, ContractError},
    msg::{DenomMetadata, InstantiateMsg, ReplyPayload},
    state::{
        get_next_payout_id, get_next_withdrawal_id, load_config, load_withdrawal_queue_info,
        ADAPTERS, CONFIG, LAST_FUNDED_WITHDRAWAL_ID, NEXT_PAYOUT_ID, NEXT_WITHDRAWAL_ID,
        PAYOUTS_HISTORY, USER_WITHDRAWAL_REQUESTS, WHITELIST, WITHDRAWAL_QUEUE_INFO,
        WITHDRAWAL_REQUESTS,
    },
};

use interface::inflow_adapter::{
    serialize_adapter_interface_msg, AdapterInterfaceMsg, AdapterInterfaceQuery,
    AdapterInterfaceQueryMsg, AvailableAmountResponse, DepositorPositionResponse,
};

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const UNUSED_MSG_ID: u64 = 0;

/// Minimum number of shares required at instantiation to prevent share price manipulation
const MINIMUM_INITIAL_SHARES: u128 = 1_000_000;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let control_center_contract = deps.api.addr_validate(&msg.control_center_contract)?;
    let token_info_provider_contract = match msg.token_info_provider_contract {
        None => None,
        Some(address) => Some(deps.api.addr_validate(&address)?),
    };

    // Validate max_withdrawals_per_user to prevent withdrawal freeze
    if msg.max_withdrawals_per_user < 1 {
        return Err(new_generic_error(
            "max_withdrawals_per_user must be at least 1",
        ));
    }

    // Validate initial_shares_recipient
    let initial_shares_recipient = deps.api.addr_validate(&msg.initial_shares_recipient)?;

    // Require initial deposit funds to be sent
    let initial_deposit = cw_utils::must_pay(&info, &msg.deposit_denom)?;

    // Convert deposit tokens to base tokens using token info provider ratio
    let initial_shares_to_mint = match &token_info_provider_contract {
        Some(provider) => {
            // Query the token info provider for the ratio
            let ratio: Decimal = deps.querier.query_wasm_smart(
                provider.to_string(),
                &TokenInfoProviderQueryMsg::RatioToBaseToken {
                    denom: msg.deposit_denom.clone(),
                },
            )?;
            // deposit_tokens * ratio = base_tokens = shares_to_mint
            initial_deposit
                .checked_multiply_ratio(ratio.atomics(), Decimal::one().atomics())
                .map_err(|e| new_generic_error(format!("overflow computing initial shares: {e}")))?
        }
        None => {
            // No token info provider means deposit token IS the base token (1:1)
            // shares_to_mint = deposit_amount
            initial_deposit
        }
    };

    // Validate minimum shares threshold
    if initial_shares_to_mint < Uint128::new(MINIMUM_INITIAL_SHARES) {
        return Err(new_generic_error(format!(
            "insufficient collateral: deposit of {} tokens would mint {} shares, \
             but minimum {} shares are required",
            initial_deposit, initial_shares_to_mint, MINIMUM_INITIAL_SHARES
        )));
    }

    CONFIG.save(
        deps.storage,
        &Config {
            deposit_denom: msg.deposit_denom.clone(),
            vault_shares_denom: String::new(),
            control_center_contract: control_center_contract.clone(),
            token_info_provider_contract: token_info_provider_contract.clone(),
            max_withdrawals_per_user: msg.max_withdrawals_per_user,
        },
    )?;

    NEXT_WITHDRAWAL_ID.save(deps.storage, &0u64)?;
    NEXT_PAYOUT_ID.save(deps.storage, &0u64)?;

    WITHDRAWAL_QUEUE_INFO.save(
        deps.storage,
        &WithdrawalQueueInfo {
            total_shares_burned: Uint128::zero(),
            total_withdrawal_amount: Uint128::zero(),
            non_funded_withdrawal_amount: Uint128::zero(),
        },
    )?;

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

    // Send SubMsg to the TokenFactory module to create a new denom
    let create_denom_msg = SubMsg::reply_on_success(
        NeutronMsg::submit_create_denom(msg.subdenom.clone()),
        UNUSED_MSG_ID,
    )
    .with_payload(to_json_vec(&ReplyPayload::CreateDenom {
        subdenom: msg.subdenom.clone(),
        metadata: msg.token_metadata,
        initial_shares_recipient: initial_shares_recipient.to_string(),
        initial_shares_amount: initial_shares_to_mint,
    })?);

    Ok(Response::new()
        .add_submessage(create_denom_msg)
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender)
        .add_attribute("deposit_token_denom", msg.deposit_denom)
        .add_attribute("subdenom", msg.subdenom)
        .add_attribute("control_center_contract", control_center_contract)
        .add_attribute(
            "token_info_provider_contract",
            token_info_provider_contract
                .map(|addr| addr.to_string())
                .unwrap_or_default(),
        )
        .add_attribute(
            "whitelist",
            whitelist_addresses
                .iter()
                .map(|addr| addr.to_string())
                .collect::<Vec<String>>()
                .join(", "),
        )
        .add_attribute(
            "max_withdrawals_per_user",
            msg.max_withdrawals_per_user.to_string(),
        )
        .add_attribute("initial_shares_minted", initial_shares_to_mint)
        .add_attribute("initial_shares_recipient", initial_shares_recipient))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    let config = load_config(deps.storage)?;

    match msg {
        ExecuteMsg::Deposit { on_behalf_of } => deposit(deps, env, info, &config, on_behalf_of),
        ExecuteMsg::Withdraw { on_behalf_of } => withdraw(deps, env, info, &config, on_behalf_of),
        ExecuteMsg::CancelWithdrawal { withdrawal_ids } => {
            cancel_withdrawal(deps, env, info, config, withdrawal_ids)
        }
        ExecuteMsg::FulfillPendingWithdrawals { limit } => {
            fulfill_pending_withdrawals(deps, env, info, config, limit)
        }
        ExecuteMsg::ClaimUnbondedWithdrawals { withdrawal_ids } => {
            claim_unbonded_withdrawals(deps, env, info, config, withdrawal_ids)
        }
        ExecuteMsg::WithdrawForDeployment { amount } => {
            withdraw_for_deployment(deps, env, info, config, amount)
        }
        ExecuteMsg::DepositFromDeployment {} => deposit_from_deployment(deps, env, info, config),
        ExecuteMsg::SetTokenInfoProviderContract { address } => {
            set_token_info_provider_contract(deps, info, config, address)
        }
        ExecuteMsg::AddToWhitelist { address } => add_to_whitelist(deps, env, info, address),
        ExecuteMsg::RemoveFromWhitelist { address } => {
            remove_from_whitelist(deps, env, info, address)
        }
        ExecuteMsg::UpdateConfig {
            config: config_update,
        } => update_config(deps, info, config, config_update),
        ExecuteMsg::RegisterAdapter {
            name,
            address,
            description,
            allocation_mode,
            deployment_tracking,
        } => register_adapter(
            deps,
            info,
            name,
            address,
            description,
            allocation_mode,
            deployment_tracking,
        ),
        ExecuteMsg::UnregisterAdapter { name } => {
            unregister_adapter(deps, env, info, &config, name)
        }
        ExecuteMsg::SetAdapterAllocationMode {
            name,
            allocation_mode,
        } => set_adapter_allocation_mode(deps, info, name, allocation_mode),
        ExecuteMsg::SetAdapterDeploymentTracking {
            name,
            deployment_tracking,
        } => set_adapter_deployment_tracking(deps, env, info, &config, name, deployment_tracking),
        ExecuteMsg::WithdrawFromAdapter {
            adapter_name,
            amount,
        } => withdraw_from_adapter(deps, info, &config, adapter_name, amount),
        ExecuteMsg::DepositToAdapter {
            adapter_name,
            amount,
        } => deposit_to_adapter(deps, env, info, &config, adapter_name, amount),
        ExecuteMsg::MoveAdapterFunds {
            from_adapter,
            to_adapter,
            coin,
        } => move_adapter_funds(deps, env, info, &config, from_adapter, to_adapter, coin),
    }
}

// Deposits tokens accepted by the vault and issues certain amount of vault shares tokens in return.
fn deposit(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: &Config,
    on_behalf_of: Option<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let deposit_amount = cw_utils::must_pay(&info, &config.deposit_denom)?;
    let recipient = match on_behalf_of {
        Some(addr) => deps.api.addr_validate(&addr)?,
        None => info.sender.clone(),
    };

    let deposit_cap = get_deposit_cap(&deps.as_ref(), &config.control_center_contract)?;
    let pool_info = get_control_center_pool_info(&deps.as_ref(), &config.control_center_contract)?;
    let total_pool_value = pool_info.total_pool_value;
    let total_shares_issued = pool_info.total_shares_issued;

    // Total value also includes the deposit amount, since the tokens are previously sent to the contract
    if total_pool_value > deposit_cap {
        return Err(new_generic_error("deposit cap has been reached"));
    }

    let deposit_amount_base_tokens =
        convert_deposit_token_into_base_token(&deps.as_ref(), config, deposit_amount)?;

    let vault_shares_to_mint = calculate_number_of_shares_to_mint(
        deposit_amount_base_tokens,
        total_pool_value,
        total_shares_issued,
    )?;

    // Prevent minting zero shares
    if vault_shares_to_mint.is_zero() {
        return Err(new_generic_error(
            "deposit amount too small: would mint zero shares",
        ));
    }

    let mut messages = vec![];

    // Determine where to deploy funds
    let allocation_result = calculate_venues_allocation(
        &deps.as_ref(),
        &env,
        deposit_amount,
        config.deposit_denom.clone(),
        true, // is_deposit
    )?;

    // Track total amount deposited to tracked adapters for single deployed amount update
    let mut total_tracked_amount = Uint128::zero();

    // If allocations exist, send funds to adapters
    for (adapter_name, amount) in allocation_result.allocations {
        // Should never happen, calculate_venues_allocation already retrieves adapters from ADAPTERS
        let adapter_info = ADAPTERS
            .may_load(deps.storage, adapter_name.clone())?
            .ok_or_else(|| ContractError::AdapterNotFound {
                name: adapter_name.clone(),
            })?;

        // Should never happen, because we filter out in calculate_venues_allocation
        if !matches!(adapter_info.allocation_mode, AllocationMode::Automated) {
            return Err(ContractError::AdapterNotIncludedInAutomatedAllocation {
                name: adapter_name.clone(),
            });
        }

        // Create adapter deposit message
        let deposit_msg = AdapterInterfaceMsg::Deposit {};

        let wasm_msg = WasmMsg::Execute {
            contract_addr: adapter_info.address.to_string(),
            msg: serialize_adapter_interface_msg(&deposit_msg)?,
            funds: vec![Coin {
                denom: config.deposit_denom.clone(),
                amount,
            }],
        };

        messages.push(CosmosMsg::Wasm(wasm_msg));

        // If adapter is tracked in deployed amount, accumulate for batch update
        if matches!(
            adapter_info.deployment_tracking,
            DeploymentTracking::Tracked
        ) {
            total_tracked_amount = total_tracked_amount.checked_add(amount)?;
        }
    }

    // If any tracked adapters received deposits, update deployed amount once
    if !total_tracked_amount.is_zero() {
        let total_tracked_amount_in_base_tokens =
            convert_deposit_token_into_base_token(&deps.as_ref(), config, total_tracked_amount)?;
        let update_msg = build_update_deployed_amount_msg(
            total_tracked_amount_in_base_tokens,
            DeploymentDirection::Add,
            config,
        )?;
        messages.push(update_msg);
    }

    // Mint vault shares to the user
    let mint_vault_shares_msg = NeutronMsg::submit_mint_tokens(
        &config.vault_shares_denom,
        vault_shares_to_mint,
        recipient.to_string(),
    );

    messages.push(CosmosMsg::Custom(mint_vault_shares_msg));

    let mut response = Response::new()
        .add_messages(messages)
        .add_attribute("action", "deposit")
        .add_attribute("sender", info.sender)
        .add_attribute("recipient", recipient.to_string())
        .add_attribute("deposit_amount", deposit_amount)
        .add_attribute("vault_shares_minted", vault_shares_to_mint);

    // Add failed adapter queries attribute if any adapters failed
    if !allocation_result.failed_adapters.is_empty() {
        response = response.add_attribute(
            "failed_adapter_queries",
            allocation_result.failed_adapters.join(","),
        );
    }

    Ok(response)
}

// User initiates withdrawal request by sending a certain number of vault shares tokens to the contract
// in order to redeem them for the underlying deposit tokens.
//
// The withdrawal follows an "all-or-nothing" approach:
// 1. Calculate total available funds (unreserved contract balance + available from adapters)
// 2. If total available >= withdrawal amount: Fulfill the entire withdrawal
//    - Withdraw from adapters if needed
//    - Send the full amount to the user immediately
// 3. If total available < withdrawal amount: Queue the entire withdrawal
//    - No adapter withdrawals are executed
//    - No immediate payout is sent
//    - The entire withdrawal request is added to the queue
//
// In all cases, the vault shares tokens sent by the user are burned immediately.
fn withdraw(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: &Config,
    on_behalf_of: Option<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let vault_shares_denom = config.vault_shares_denom.clone();
    let vault_shares_sent = cw_utils::must_pay(&info, &vault_shares_denom)?;
    let withdrawer = match on_behalf_of {
        Some(address) => deps.api.addr_validate(&address)?,
        None => info.sender.clone(),
    };

    // Calculate how many deposit tokens the sent vault shares are worth
    let amount_to_withdraw =
        query_shares_equivalent_value(&deps.as_ref(), config, vault_shares_sent)?;

    if amount_to_withdraw.is_zero() {
        return Err(new_generic_error("cannot withdraw zero amount"));
    }

    let mut response = Response::new()
        .add_attribute("action", "withdraw")
        .add_attribute("sender", info.sender.clone())
        .add_attribute("withdrawer", withdrawer.to_string())
        .add_attribute("vault_shares_sent", vault_shares_sent);

    // === STEP 1: Check if we can fulfill the entire withdrawal ===
    // All-or-nothing: either fulfill completely or queue entirely
    let withdrawal_queue_info = load_withdrawal_queue_info(deps.storage)?;
    let available_contract_balance = get_balance_available_for_pending_withdrawals(
        &deps.as_ref(),
        env.contract.address.as_ref(),
        &config.deposit_denom,
        &withdrawal_queue_info,
    )?;

    // Calculate how much adapters can provide (without withdrawing yet)
    let remaining_after_contract = amount_to_withdraw.saturating_sub(available_contract_balance);
    let allocation_result = if remaining_after_contract > Uint128::zero() {
        calculate_venues_allocation(
            &deps.as_ref(),
            &env,
            remaining_after_contract,
            config.deposit_denom.clone(),
            false, // is_deposit = false
        )?
    } else {
        AllocationResult {
            allocations: vec![],
            failed_adapters: vec![],
        }
    };

    // Add failed adapter queries attribute if any adapters failed
    if !allocation_result.failed_adapters.is_empty() {
        response = response.add_attribute(
            "failed_adapter_queries",
            allocation_result.failed_adapters.join(","),
        );
    }

    let total_from_adapters: Uint128 = allocation_result
        .allocations
        .iter()
        .map(|(_, amt)| *amt)
        .sum();
    let total_available = available_contract_balance.checked_add(total_from_adapters)?;

    // Determine if we can fulfill entirely - all or nothing
    let can_fulfill_entirely = total_available >= amount_to_withdraw;

    // === STEP 2: If we can fulfill entirely, withdraw from adapters ===
    let mut messages = vec![];
    if can_fulfill_entirely && !allocation_result.allocations.is_empty() {
        // Track total amount withdrawn from tracked adapters for single deployed amount update
        let mut total_tracked_amount = Uint128::zero();

        for (adapter_name, amount) in allocation_result.allocations {
            // Should always succeed, as calculate_venues_allocation already retrieves adapters from ADAPTERS
            let adapter_info = ADAPTERS
                .may_load(deps.storage, adapter_name.clone())?
                .ok_or_else(|| ContractError::AdapterNotFound {
                    name: adapter_name.clone(),
                })?;

            // Should never happen, because we filter out in calculate_venues_allocation
            if !matches!(adapter_info.allocation_mode, AllocationMode::Automated) {
                return Err(ContractError::AdapterNotIncludedInAutomatedAllocation {
                    name: adapter_name.clone(),
                });
            }

            // The amount has been retrieved from calculate_venues_allocation, which ensures
            // that the adapter has sufficient available balance for the withdrawal.
            // However, if the adapter cannot process the withdrawal fully for any reason,
            // it MUST fail (as clarified in the adapter interface documentation)
            // and the entire transaction will revert.
            let withdraw_msg = AdapterInterfaceMsg::Withdraw {
                coin: Coin {
                    denom: config.deposit_denom.clone(),
                    amount,
                },
            };

            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: adapter_info.address.to_string(),
                msg: serialize_adapter_interface_msg(&withdraw_msg)?,
                funds: vec![],
            }));

            // If adapter is tracked in deployed amount, accumulate for batch update
            if matches!(
                adapter_info.deployment_tracking,
                DeploymentTracking::Tracked
            ) {
                total_tracked_amount = total_tracked_amount.checked_add(amount)?;
            }
        }

        // If any tracked adapters had withdrawals, update deployed amount once
        if !total_tracked_amount.is_zero() {
            let total_tracked_amount_in_base_tokens = convert_deposit_token_into_base_token(
                &deps.as_ref(),
                config,
                total_tracked_amount,
            )?;
            let update_msg = build_update_deployed_amount_msg(
                total_tracked_amount_in_base_tokens,
                DeploymentDirection::Subtract,
                config,
            )?;
            messages.push(update_msg);
        }
    }

    // === STEP 3a: Queue everything if cannot fulfill entirely ===
    if !can_fulfill_entirely {
        // Queue the ENTIRE withdrawal
        let withdrawal_id = get_next_withdrawal_id(deps.storage)?;

        let withdrawal_entry = WithdrawalEntry {
            id: withdrawal_id,
            initiated_at: env.block.time,
            withdrawer: withdrawer.clone(),
            shares_burned: vault_shares_sent,
            amount_to_receive: amount_to_withdraw,
            is_funded: false,
        };

        // Add the new withdrawal entry to the queue
        WITHDRAWAL_REQUESTS.save(deps.storage, withdrawal_id, &withdrawal_entry)?;

        // Update the withdrawal queue info with the full amount and all shares
        update_withdrawal_queue_info(
            deps.storage,
            Some(Int128::try_from(vault_shares_sent)?),
            Some(Int128::try_from(amount_to_withdraw)?),
            Some(Int128::try_from(amount_to_withdraw)?),
        )?;

        // Add the new withdrawal id to the list of user's withdrawal requests
        update_user_withdrawal_requests_info(
            deps.storage,
            withdrawer.clone(),
            config,
            Some(vec![withdrawal_id]),
            None,
            true,
        )?;

        response = response
            .add_attribute("withdrawal_id", withdrawal_id.to_string())
            .add_attribute("amount_queued_for_withdrawal", amount_to_withdraw);
    }

    // === STEP 3b: Send full amount if fulfilled ===
    if can_fulfill_entirely {
        // This message is added after adapter withdrawals to ensure funds are available
        messages.push(CosmosMsg::Bank(BankMsg::Send {
            to_address: withdrawer.to_string(),
            amount: vec![Coin::new(amount_to_withdraw, config.deposit_denom.clone())],
        }));

        response = response.add_attribute("paid_out_amount", amount_to_withdraw);

        // Add entry to the payout history with all shares
        add_payout_history_entry(
            deps.storage,
            &env,
            &withdrawer,
            vault_shares_sent,
            amount_to_withdraw,
        )?;
    }

    // Burn the vault shares tokens sent by the user
    let burn_shares_msg = NeutronMsg::submit_burn_tokens(&vault_shares_denom, vault_shares_sent);
    messages.push(CosmosMsg::Custom(burn_shares_msg));

    Ok(response.add_messages(messages))
}

// Users can cancel any of their pending withdrawal requests until the funds for those withdrawals
// have been provided to the smart contract. Users will receive back certain number of vault shares.
// The number of vault shares to be minted back is calculated based on the sum of the amounts to be
// received from all withdrawal requests that are being canceled. Withdrawals are not allowed if the
// vault deposit cap has been reached.
fn cancel_withdrawal(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    config: Config,
    withdrawal_ids: Vec<u64>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let deposit_cap = get_deposit_cap(&deps.as_ref(), &config.control_center_contract)?;
    let pool_info = get_control_center_pool_info(&deps.as_ref(), &config.control_center_contract)?;
    let total_pool_value = pool_info.total_pool_value;
    let total_shares_issued = pool_info.total_shares_issued;

    if total_pool_value >= deposit_cap {
        return Err(new_generic_error(
            "cannot cancel withdrawals- deposit cap has been reached",
        ));
    }

    // Users can only cancel withdrawals for which the funds have not been provided yet.
    let lowest_id_allowed_to_cancel = LAST_FUNDED_WITHDRAWAL_ID
        .may_load(deps.storage)?
        .map(|id| id + 1)
        .unwrap_or_default();

    let user_withdrawals: HashSet<u64> = USER_WITHDRAWAL_REQUESTS
        .may_load(deps.storage, info.sender.clone())?
        .unwrap_or_default()
        .into_iter()
        .collect();

    let mut withdrawals_to_process: HashSet<u64> = HashSet::new();
    let mut withdrawals_to_skip: HashSet<u64> = HashSet::new();

    // Ensure that:
    //   1. Sender is the owner of withdrawal requests
    //   2. Withdrawal requests have not been funded yet
    //   3. No duplicate withdrawal IDs are processed
    for withdrawal_id in withdrawal_ids {
        if !user_withdrawals.contains(&withdrawal_id) || withdrawal_id < lowest_id_allowed_to_cancel
        {
            withdrawals_to_skip.insert(withdrawal_id);
        } else {
            withdrawals_to_process.insert(withdrawal_id);
        }
    }

    let response = Response::new()
        .add_attribute("action", "cancel_withdrawal")
        .add_attribute("sender", info.sender.clone())
        .add_attribute(
            "withdrawal_ids_processed",
            get_slice_as_attribute(
                withdrawals_to_process
                    .iter()
                    .collect::<Vec<&u64>>()
                    .as_slice(),
            ),
        )
        .add_attribute(
            "withdrawal_ids_skipped",
            get_slice_as_attribute(withdrawals_to_skip.iter().collect::<Vec<&u64>>().as_slice()),
        );

    if withdrawals_to_process.is_empty() {
        return Ok(response);
    }

    let mut shares_burned = Uint128::zero();
    let mut amount_to_withdraw = Uint128::zero();

    for withdrawal_id in &withdrawals_to_process {
        // USER_WITHDRAWAL_REQUESTS should always be in sync with WITHDRAWAL_REQUESTS.
        // If the withdrawal entry is not found, the entire execute() action should fail.
        let withdrawal_entry = WITHDRAWAL_REQUESTS.load(deps.storage, *withdrawal_id)?;

        // Double check that the withdrawal belongs to the sender and has not been funded yet
        if withdrawal_entry.withdrawer != info.sender || withdrawal_entry.is_funded {
            return Err(new_generic_error(format!(
                "withdrawal request {withdrawal_id} cannot be cancelled"
            )));
        }

        shares_burned = shares_burned.checked_add(withdrawal_entry.shares_burned)?;
        amount_to_withdraw = amount_to_withdraw.checked_add(withdrawal_entry.amount_to_receive)?;

        // Remove the withdrawal entry from the queue
        WITHDRAWAL_REQUESTS.remove(deps.storage, withdrawal_entry.id);
    }

    // Remove canceled withdrawal ids from the list of user's pending withdrawal requests
    update_user_withdrawal_requests_info(
        deps.storage,
        info.sender.clone(),
        &config,
        None,
        Some(withdrawals_to_process.into_iter().collect()),
        false,
    )?;

    // Subtract the burned shares and canceled amounts from the withdrawal queue info
    update_withdrawal_queue_info(
        deps.storage,
        Some(Int128::try_from(shares_burned)?.strict_neg()),
        Some(Int128::try_from(amount_to_withdraw)?.strict_neg()),
        Some(Int128::try_from(amount_to_withdraw)?.strict_neg()),
    )?;

    // Convert the amount to withdraw into the base tokens in order to calculate how many shares to mint back
    let amount_to_withdraw_base_tokens =
        convert_deposit_token_into_base_token(&deps.as_ref(), &config, amount_to_withdraw)?;

    // We need to recalculate the total pool value, since the withdrawal queue info has changed.
    let total_pool_value = total_pool_value.checked_add(amount_to_withdraw_base_tokens)?;

    // Calculate how many vault shares should be minted back to the user
    // Take the minimum of the shares they burned and the shares they would receive if they deposited the withdrawal amount right now
    // This is safe against attempts by users to "hedge" against a ratio decrease by creating and cancelling withdrawals
    let shares_to_mint = min(
        shares_burned,
        calculate_number_of_shares_to_mint(
            amount_to_withdraw_base_tokens,
            total_pool_value,
            total_shares_issued,
        )?,
    );

    // Prevent minting zero shares
    if shares_to_mint.is_zero() {
        return Err(new_generic_error(
            "cannot cancel withdrawal: would mint zero shares",
        ));
    }

    // Mint the vault shares tokens
    let mint_vault_shares_msg =
        NeutronMsg::submit_mint_tokens(&config.vault_shares_denom, shares_to_mint, &info.sender);

    Ok(response
        .add_message(mint_vault_shares_msg)
        .add_attribute("canceled_withdrawal_amount", amount_to_withdraw)
        .add_attribute("shares_initially_burned", shares_burned)
        .add_attribute("shares_minted_back", shares_to_mint))
}

// Permissionless action that iterates over the withdrawal requests queue and marks as funded all
// those withdrawal requests that can be paid out with the funds held by the contract, but also
// considering the funds already allocated for earlier requests that have not been paid out yet.
fn fulfill_pending_withdrawals(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: Config,
    limit: u64,
) -> Result<Response<NeutronMsg>, ContractError> {
    // If this action is being executed for the first time, start from withdrawal ID 0.
    // Otherwise, start from the last funded withdrawal ID increased by 1.
    let start = LAST_FUNDED_WITHDRAWAL_ID
        .may_load(deps.storage)?
        .map(|id| id + 1)
        .unwrap_or_default();

    let withdrawals = WITHDRAWAL_REQUESTS
        .range(
            deps.storage,
            Some(Bound::inclusive(start)),
            None,
            Order::Ascending,
        )
        .take(limit as usize)
        .filter_map(|withdrawal| {
            match withdrawal {
                Ok((_, withdrawal)) => {
                    // It should never happen that a withdrawal entry after the last funded ID
                    // has already been provided with the funds, but we check that just in case.
                    if withdrawal.is_funded {
                        return None;
                    }

                    Some(withdrawal)
                }
                Err(_) => None,
            }
        })
        .collect::<Vec<WithdrawalEntry>>();

    let withdrawal_queue_info = load_withdrawal_queue_info(deps.storage)?;

    let mut available_balance = get_balance_available_for_pending_withdrawals(
        &deps.as_ref(),
        env.contract.address.as_ref(),
        &config.deposit_denom,
        &withdrawal_queue_info,
    )?;

    let response = Response::new()
        .add_attribute("action", "fulfill_pending_withdrawals")
        .add_attribute("sender", info.sender);

    let mut total_amount_funded = Uint128::zero();
    let mut funded_withdrawal_ids = vec![];

    for mut withdrawal in withdrawals {
        if withdrawal.amount_to_receive > available_balance {
            break;
        }

        withdrawal.is_funded = true;
        total_amount_funded = total_amount_funded.checked_add(withdrawal.amount_to_receive)?;
        funded_withdrawal_ids.push(withdrawal.id);
        available_balance = available_balance.checked_sub(withdrawal.amount_to_receive)?;

        WITHDRAWAL_REQUESTS.save(deps.storage, withdrawal.id, &withdrawal)?;
    }

    if !total_amount_funded.is_zero() {
        // Update the withdrawal queue info by reducing the non-funded amount
        update_withdrawal_queue_info(
            deps.storage,
            None,
            None,
            Some(Int128::try_from(total_amount_funded)?.strict_neg()),
        )?;

        // Update the last funded withdrawal ID
        LAST_FUNDED_WITHDRAWAL_ID
            .save(deps.storage, funded_withdrawal_ids.iter().max().unwrap())?;
    }

    Ok(response
        .add_attribute(
            "funded_withdrawal_ids",
            get_slice_as_attribute(funded_withdrawal_ids.as_slice()),
        )
        .add_attribute("total_amount_funded", total_amount_funded))
}

// Permissionless action that iterates over the provided list of withdrawal IDs and executes the
// actual payouts for those withdrawals that have already had the funds provided to the contract.
// Withdrawal entries must first be marked as ready for payout, which is achieved by executing
// the `fulfill_pending_withdrawals()` action.
fn claim_unbonded_withdrawals(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: Config,
    withdrawal_ids: Vec<u64>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let Some(last_funded_withdrawal_id) = LAST_FUNDED_WITHDRAWAL_ID.may_load(deps.storage)? else {
        return Err(new_generic_error(
            "no withdrawal requests have been funded yet",
        ));
    };

    // Remove duplicates and filter out any withdrawal IDs that haven't been marked as funded yet
    let withdrawal_ids = withdrawal_ids
        .into_iter()
        .filter(|withdrawal_id| withdrawal_id <= &last_funded_withdrawal_id)
        .collect::<HashSet<u64>>();

    let withdrawals = withdrawal_ids
        .into_iter()
        .filter_map(|withdrawal_id| {
            match WITHDRAWAL_REQUESTS.may_load(deps.storage, withdrawal_id) {
                Ok(withdrawal) => {
                    let withdrawal = withdrawal?;

                    // Double check that the withdrawal has been funded, just in case
                    if !withdrawal.is_funded {
                        return None;
                    }

                    Some(withdrawal)
                }
                Err(_) => None,
            }
        })
        .collect::<Vec<WithdrawalEntry>>();

    if withdrawals.is_empty() {
        return Err(new_generic_error(
            "must provide at least one valid withdrawal id",
        ));
    }

    let mut total_shares_burned = Uint128::zero();
    let mut total_amount_withdrawn = Uint128::zero();
    let mut users_withdrawals: HashMap<Addr, Vec<WithdrawalEntry>> = HashMap::new();

    for withdrawal in &withdrawals {
        let user_withdrawals = users_withdrawals
            .entry(withdrawal.withdrawer.clone())
            .or_default();

        user_withdrawals.push(withdrawal.clone());

        total_amount_withdrawn =
            total_amount_withdrawn.checked_add(withdrawal.amount_to_receive)?;

        total_shares_burned = total_shares_burned.checked_add(withdrawal.shares_burned)?;

        WITHDRAWAL_REQUESTS.remove(deps.storage, withdrawal.id);
    }

    let mut messages = vec![];

    for user_withdrawals in &users_withdrawals {
        let mut withdrawal_ids_to_remove = vec![];
        let mut amount_to_withdraw = Uint128::zero();
        let mut vault_shares_burned = Uint128::zero();

        let recipient = user_withdrawals.0;
        for withdrawal in user_withdrawals.1 {
            withdrawal_ids_to_remove.push(withdrawal.id);
            amount_to_withdraw = amount_to_withdraw.checked_add(withdrawal.amount_to_receive)?;
            vault_shares_burned = vault_shares_burned.checked_add(withdrawal.shares_burned)?;
        }

        // Remove the processed withdrawal ids from the list of user's pending withdrawal requests
        update_user_withdrawal_requests_info(
            deps.storage,
            recipient.clone(),
            &config,
            None,
            Some(withdrawal_ids_to_remove),
            false,
        )?;

        // Prepare bank message to send the tokens to the user
        messages.push(CosmosMsg::Bank(BankMsg::Send {
            to_address: recipient.to_string(),
            amount: vec![Coin::new(amount_to_withdraw, config.deposit_denom.clone())],
        }));

        // Add entry to the payout history
        add_payout_history_entry(
            deps.storage,
            &env,
            recipient,
            vault_shares_burned,
            amount_to_withdraw,
        )?;
    }

    update_withdrawal_queue_info(
        deps.storage,
        Some(Int128::try_from(total_shares_burned)?.strict_neg()),
        Some(Int128::try_from(total_amount_withdrawn)?.strict_neg()),
        None,
    )?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("action", "claim_unbonded_withdrawals")
        .add_attribute("sender", info.sender)
        .add_attribute(
            "withdrawal_ids",
            get_slice_as_attribute(
                withdrawals
                    .iter()
                    .map(|w| w.id)
                    .collect::<Vec<u64>>()
                    .as_slice(),
            ),
        )
        .add_attribute("total_amount_withdrawn", total_amount_withdrawn))
}

// Withdraws the specified amount for deployment.
fn withdraw_for_deployment(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: Config,
    amount: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    let available_for_deployment = query_available_for_deployment(&deps.as_ref(), &env)?;
    if available_for_deployment.is_zero() {
        return Err(new_generic_error("no funds are available for deployment"));
    }

    // If the requested amount exceeds the available amount, withdraw only what is available.
    let amount_to_withdraw = available_for_deployment.min(amount);

    let mut submsgs = vec![];

    // We can update the deployed amount immediately, since we know it is
    // now transferred to the whitelisted address for further deployments.
    // Since the deployed amount is denominated in base tokens (e.g. ATOM),
    // we need to convert amount_to_withdraw into the base denom as well.
    let amount_to_withdraw_in_base_tokens =
        convert_deposit_token_into_base_token(&deps.as_ref(), &config, amount_to_withdraw)?;

    let update_deployed_amount_msg = build_update_deployed_amount_msg(
        amount_to_withdraw_in_base_tokens,
        DeploymentDirection::Add,
        &config,
    )?;

    let send_tokens_msg = CosmosMsg::Bank(BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![Coin {
            amount: amount_to_withdraw,
            denom: config.deposit_denom.clone(),
        }],
    });

    submsgs.extend_from_slice(&[send_tokens_msg, update_deployed_amount_msg]);

    Ok(Response::new()
        .add_messages(submsgs)
        .add_attribute("action", "withdraw_for_deployment")
        .add_attribute("sender", info.sender)
        .add_attribute("amount_requested", amount)
        .add_attribute("amount_withdrawn", amount_to_withdraw))
}

// Deposits funds back from deployment.
fn deposit_from_deployment(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    config: Config,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    let deposited_amount = cw_utils::must_pay(&info, &config.deposit_denom)?;

    // Convert deposited amount to base tokens
    let deposited_amount_in_base_tokens =
        convert_deposit_token_into_base_token(&deps.as_ref(), &config, deposited_amount)?;

    // Call control center to subtract from deployed amount
    let update_deployed_amount_msg = build_update_deployed_amount_msg(
        deposited_amount_in_base_tokens,
        DeploymentDirection::Subtract,
        &config,
    )?;

    // Funds are automatically left in the vault's balance

    Ok(Response::new()
        .add_message(update_deployed_amount_msg)
        .add_attribute("action", "deposit_from_deployment")
        .add_attribute("sender", info.sender)
        .add_attribute("amount_deposited", deposited_amount))
}

fn set_token_info_provider_contract(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    mut config: Config,
    token_info_provider_contract: Option<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    let token_info_provider_contract = match token_info_provider_contract {
        None => None,
        Some(address) => Some(deps.api.addr_validate(&address)?),
    };

    config.token_info_provider_contract = token_info_provider_contract.clone();
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("action", "set_token_info_provider_contract")
        .add_attribute("sender", info.sender)
        .add_attribute(
            "token_info_provider_contract",
            token_info_provider_contract
                .map(|addr| addr.to_string())
                .unwrap_or_default(),
        ))
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

/// Query position for a single adapter
fn query_single_adapter_position(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    adapter_info: &AdapterInfo,
    deposit_denom: &str,
) -> Result<Uint128, ContractError> {
    let query_msg = AdapterInterfaceQueryMsg::DepositorPosition {
        depositor_address: env.contract.address.to_string(),
        denom: deposit_denom.to_string(),
    };

    let result: Result<DepositorPositionResponse, _> = deps.querier.query_wasm_smart(
        adapter_info.address.to_string(),
        &AdapterInterfaceQuery {
            standard_query: &query_msg,
        },
    );

    match result {
        Ok(response) => Ok(response.amount),
        Err(e) => Err(new_generic_error(format!(
            "failed to query adapter position: {}",
            e
        ))),
    }
}

fn validate_address_is_whitelisted(
    deps: &DepsMut<NeutronQuery>,
    address: Addr,
) -> Result<(), ContractError> {
    let is_whitelisted = WHITELIST.may_load(deps.storage, address)?;
    if is_whitelisted.is_none() {
        return Err(ContractError::Unauthorized);
    }

    Ok(())
}

/// Given the `deposit_amount_base_tokens`, this function will calculate how many vault shares tokens should be minted in return.
pub fn calculate_number_of_shares_to_mint(
    deposit_amount_base_tokens: Uint128,
    total_pool_value_base_tokens: Uint128,
    total_shares_issued: Uint128,
) -> Result<Uint128, ContractError> {
    // `deposit_amount` has already been added to the smart contract balance even before `execute()` is called,
    // so we need to subtract it here in order to accurately calculate number of vault shares to mint.
    let deposit_token_current_balance = total_pool_value_base_tokens
        .checked_sub(deposit_amount_base_tokens)
        .unwrap_or_default();

    // If there are currently no vault shares minted, then vault shares have 1:1 ratio with the deposit token.
    if deposit_token_current_balance.is_zero() || total_shares_issued.is_zero() {
        return Ok(deposit_amount_base_tokens);
    }

    deposit_amount_base_tokens
        .checked_multiply_ratio(total_shares_issued, deposit_token_current_balance)
        .map_err(|e| new_generic_error(format!("overflow error: {e}")))
}

// Adapter management functions

/// Registers a new adapter.
///
/// # Race Condition Warning
/// When using `DeploymentTracking::Tracked`:
/// - With `Automated` allocation: DANGEROUS - creates race conditions with `SubmitDeployedAmount`
/// - With `Manual` allocation: Ensure no `SubmitDeployedAmount` proposals are pending during operations
///
/// See `DeploymentTracking` enum documentation for details.
fn register_adapter(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    name: String,
    address: String,
    description: Option<String>,
    allocation_mode: AllocationMode,
    deployment_tracking: DeploymentTracking,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate caller is whitelisted
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    // Validate adapter address
    let adapter_addr = deps.api.addr_validate(&address)?;

    // Check if adapter already exists
    if ADAPTERS.has(deps.storage, name.clone()) {
        return Err(ContractError::AdapterAlreadyExists { name });
    }

    // Save adapter info
    let adapter_info = AdapterInfo {
        address: adapter_addr.clone(),
        allocation_mode: allocation_mode.clone(),
        deployment_tracking: deployment_tracking.clone(),
        name: name.clone(),
        description: description.clone(),
    };

    ADAPTERS.save(deps.storage, name.clone(), &adapter_info)?;

    Ok(Response::new()
        .add_attribute("action", "register_adapter")
        .add_attribute("sender", info.sender)
        .add_attribute("adapter_name", name)
        .add_attribute("adapter_address", adapter_addr)
        .add_attribute("allocation_mode", format!("{:?}", allocation_mode))
        .add_attribute("deployment_tracking", format!("{:?}", deployment_tracking))
        .add_attribute("description", description.unwrap_or_default()))
}

fn unregister_adapter(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: &Config,
    name: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate caller is whitelisted
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    // Load adapter (return error if not found)
    let adapter_info = ADAPTERS
        .may_load(deps.storage, name.clone())?
        .ok_or_else(|| ContractError::AdapterNotFound { name: name.clone() })?;

    // Query adapter position - must be zero to unregister
    let adapter_position =
        query_single_adapter_position(&deps.as_ref(), &env, &adapter_info, &config.deposit_denom)?;

    if !adapter_position.is_zero() {
        return Err(new_generic_error(format!(
            "cannot unregister adapter '{}' with non-zero position: {}",
            name, adapter_position
        )));
    }

    // Remove adapter from ADAPTERS map
    ADAPTERS.remove(deps.storage, name.clone());

    Ok(Response::new()
        .add_attribute("action", "unregister_adapter")
        .add_attribute("sender", info.sender)
        .add_attribute("adapter_name", name)
        .add_attribute("adapter_address", adapter_info.address))
}

fn set_adapter_allocation_mode(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    name: String,
    allocation_mode: AllocationMode,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate caller is whitelisted
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    // Load adapter (return error if not found)
    let mut adapter_info = ADAPTERS
        .may_load(deps.storage, name.clone())?
        .ok_or_else(|| ContractError::AdapterNotFound { name: name.clone() })?;

    // Update allocation mode
    adapter_info.allocation_mode = allocation_mode.clone();

    // Save updated adapter info
    ADAPTERS.save(deps.storage, name.clone(), &adapter_info)?;

    Ok(Response::new()
        .add_attribute("action", "set_adapter_allocation_mode")
        .add_attribute("sender", info.sender)
        .add_attribute("adapter_name", name)
        .add_attribute("allocation_mode", format!("{:?}", allocation_mode)))
}

/// Sets deployment tracking mode for an adapter.
///
/// # Race Condition Warning
/// When setting to `Tracked`:
/// - With `Automated` allocation: Creates race condition with manual `SubmitDeployedAmount` calls
/// - With `Manual` allocation: Ensure no `SubmitDeployedAmount` proposal is pending
///
/// See `DeploymentTracking` enum documentation for details.
fn set_adapter_deployment_tracking(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: &Config,
    name: String,
    deployment_tracking: DeploymentTracking,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate caller is whitelisted
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    // Load adapter (return error if not found)
    let mut adapter_info = ADAPTERS
        .may_load(deps.storage, name.clone())?
        .ok_or_else(|| ContractError::AdapterNotFound { name: name.clone() })?;

    let old_tracking = adapter_info.deployment_tracking.clone();

    // Only proceed if tracking mode is actually changing
    if old_tracking == deployment_tracking {
        return Ok(Response::new()
            .add_attribute("action", "set_adapter_deployment_tracking")
            .add_attribute("sender", info.sender)
            .add_attribute("adapter_name", name)
            .add_attribute("result", "no_change"));
    }

    // Query current adapter position to sync DEPLOYED_AMOUNT
    let adapter_position =
        query_single_adapter_position(&deps.as_ref(), &env, &adapter_info, &config.deposit_denom)?;

    let mut messages = vec![];

    // If position is non-zero, update DEPLOYED_AMOUNT based on tracking direction change
    if !adapter_position.is_zero() {
        // Convert to base tokens for the deployed amount update
        let adapter_position_base_tokens =
            convert_deposit_token_into_base_token(&deps.as_ref(), config, adapter_position)?;

        match (&old_tracking, &deployment_tracking) {
            // Tracked -> NotTracked: Subtract from DEPLOYED_AMOUNT
            (DeploymentTracking::Tracked, DeploymentTracking::NotTracked) => {
                let update_msg = build_update_deployed_amount_msg(
                    adapter_position_base_tokens,
                    DeploymentDirection::Subtract,
                    config,
                )?;
                messages.push(update_msg);
            }
            // NotTracked -> Tracked: Add to DEPLOYED_AMOUNT
            (DeploymentTracking::NotTracked, DeploymentTracking::Tracked) => {
                let update_msg = build_update_deployed_amount_msg(
                    adapter_position_base_tokens,
                    DeploymentDirection::Add,
                    config,
                )?;
                messages.push(update_msg);
            }
            // Same tracking mode - shouldn't reach here due to early return above
            _ => {}
        }
    }

    // Update deployment tracking
    adapter_info.deployment_tracking = deployment_tracking.clone();

    // Save updated adapter info
    ADAPTERS.save(deps.storage, name.clone(), &adapter_info)?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("action", "set_adapter_deployment_tracking")
        .add_attribute("sender", info.sender)
        .add_attribute("adapter_name", name)
        .add_attribute("deployment_tracking", format!("{:?}", deployment_tracking))
        .add_attribute("synced_amount", adapter_position))
}

/// Withdraws funds from an adapter to the vault contract.
/// Only callable by whitelisted addresses.
/// If the adapter's deployment tracking is Tracked, updates the Control Center's DEPLOYED_AMOUNT.
fn withdraw_from_adapter(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    config: &Config,
    adapter_name: String,
    amount: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate caller is whitelisted
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    // Load adapter (return error if not found)
    let adapter_info = ADAPTERS
        .may_load(deps.storage, adapter_name.clone())?
        .ok_or_else(|| ContractError::AdapterNotFound {
            name: adapter_name.clone(),
        })?;

    let mut messages = vec![];

    // Create adapter withdrawal message
    let withdraw_msg = AdapterInterfaceMsg::Withdraw {
        coin: Coin {
            denom: config.deposit_denom.clone(),
            amount,
        },
    };

    let wasm_msg = WasmMsg::Execute {
        contract_addr: adapter_info.address.to_string(),
        msg: serialize_adapter_interface_msg(&withdraw_msg)?,
        funds: vec![],
    };
    messages.push(CosmosMsg::Wasm(wasm_msg));

    // If adapter is tracked in deployed amount, update Control Center
    if matches!(
        adapter_info.deployment_tracking,
        DeploymentTracking::Tracked
    ) {
        let amount_in_base_tokens =
            convert_deposit_token_into_base_token(&deps.as_ref(), config, amount)?;
        let update_msg = build_update_deployed_amount_msg(
            amount_in_base_tokens,
            DeploymentDirection::Subtract,
            config,
        )?;
        messages.push(update_msg);
    }

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("action", "withdraw_from_adapter")
        .add_attribute("sender", info.sender)
        .add_attribute("adapter_name", adapter_name)
        .add_attribute("amount", amount)
        .add_attribute(
            "deployment_tracking",
            format!("{:?}", adapter_info.deployment_tracking),
        ))
}

/// Deposits funds from vault balance to an adapter.
/// Only callable by whitelisted addresses.
/// Can deposit to any adapter regardless of allocation mode.
/// Used for manual rebalancing operations between adapters.
/// If the adapter's deployment tracking is Tracked, updates the Control Center's DEPLOYED_AMOUNT.
fn deposit_to_adapter(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: &Config,
    adapter_name: String,
    amount: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate caller is whitelisted
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    // Load adapter (return error if not found)
    let adapter_info = ADAPTERS
        .may_load(deps.storage, adapter_name.clone())?
        .ok_or_else(|| ContractError::AdapterNotFound {
            name: adapter_name.clone(),
        })?;

    // Check vault has sufficient balance
    let vault_balance = deps
        .querier
        .query_balance(&env.contract.address, &config.deposit_denom)?;

    if vault_balance.amount < amount {
        return Err(ContractError::InsufficientBalance {
            available: vault_balance.amount,
            required: amount,
        });
    }

    let mut messages = vec![];

    // Create adapter deposit message with funds
    let deposit_msg = AdapterInterfaceMsg::Deposit {};

    let wasm_msg = WasmMsg::Execute {
        contract_addr: adapter_info.address.to_string(),
        msg: serialize_adapter_interface_msg(&deposit_msg)?,
        funds: vec![Coin {
            denom: config.deposit_denom.clone(),
            amount,
        }],
    };
    messages.push(CosmosMsg::Wasm(wasm_msg));

    // If adapter is tracked in deployed amount, update Control Center
    if matches!(
        adapter_info.deployment_tracking,
        DeploymentTracking::Tracked
    ) {
        let amount_in_base_tokens =
            convert_deposit_token_into_base_token(&deps.as_ref(), config, amount)?;
        let update_msg = build_update_deployed_amount_msg(
            amount_in_base_tokens,
            DeploymentDirection::Add,
            config,
        )?;
        messages.push(update_msg);
    }

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("action", "deposit_to_adapter")
        .add_attribute("sender", info.sender)
        .add_attribute("adapter_name", adapter_name)
        .add_attribute("amount", amount)
        .add_attribute(
            "deployment_tracking",
            format!("{:?}", adapter_info.deployment_tracking),
        ))
}

fn move_adapter_funds(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: &Config,
    from_adapter: String,
    to_adapter: String,
    coin: Coin,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Validate caller is whitelisted (done by both helper functions, but check early)
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    // Validate non-zero amount
    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // Check if moving deposit_denom or other denom
    if coin.denom == config.deposit_denom {
        // Case 1: Moving deposit_denom
        // Reuse existing withdraw_from_adapter and deposit_to_adapter logic

        // Withdraw from source adapter (handles DeploymentTracking automatically)
        let withdraw_response = withdraw_from_adapter(
            deps.branch(),
            info.clone(),
            config,
            from_adapter.clone(),
            coin.amount,
        )?;

        // Deposit to destination adapter (handles DeploymentTracking automatically)
        let deposit_response = deposit_to_adapter(
            deps,
            env,
            info.clone(),
            config,
            to_adapter.clone(),
            coin.amount,
        )?;

        // Combine messages and attributes from both operations
        // The messages will be executed in order: withdraw then deposit,
        // and contain the necessary deployed amount updates
        Ok(Response::new()
            .add_submessages(withdraw_response.messages)
            .add_submessages(deposit_response.messages)
            .add_attribute("action", "move_adapter_funds")
            .add_attribute("sender", info.sender)
            .add_attribute("from_adapter", from_adapter)
            .add_attribute("to_adapter", to_adapter)
            .add_attribute("denom", coin.denom)
            .add_attribute("amount", coin.amount))
    } else {
        // Case 2: Moving non-deposit_denom
        // Require matching DeploymentTracking to prevent accounting issues,
        // since no deployed amount updates will be made

        // Load both adapters for validation
        let from_adapter_info = ADAPTERS
            .may_load(deps.storage, from_adapter.clone())?
            .ok_or_else(|| ContractError::AdapterNotFound {
                name: from_adapter.clone(),
            })?;

        let to_adapter_info = ADAPTERS
            .may_load(deps.storage, to_adapter.clone())?
            .ok_or_else(|| ContractError::AdapterNotFound {
                name: to_adapter.clone(),
            })?;

        // Validate matching DeploymentTracking
        if from_adapter_info.deployment_tracking != to_adapter_info.deployment_tracking {
            return Err(ContractError::AdapterTrackingMismatch {
                from_adapter: from_adapter.clone(),
                to_adapter: to_adapter.clone(),
                from_tracking: from_adapter_info.deployment_tracking.clone(),
                to_tracking: to_adapter_info.deployment_tracking.clone(),
            });
        }

        let mut messages = vec![];

        // Withdraw from source adapter
        let withdraw_msg = AdapterInterfaceMsg::Withdraw { coin: coin.clone() };
        messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: from_adapter_info.address.to_string(),
            msg: serialize_adapter_interface_msg(&withdraw_msg)?,
            funds: vec![],
        }));

        // Deposit to destination adapter
        let deposit_msg = AdapterInterfaceMsg::Deposit {};
        messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: to_adapter_info.address.to_string(),
            msg: serialize_adapter_interface_msg(&deposit_msg)?,
            funds: vec![coin.clone()],
        }));

        // Note: No deployed amount updates for non-deposit_denom
        // Even if both are Tracked, we can't convert non-deposit_denom to base tokens
        // The tracking requirement ensures accounting consistency even without updates

        Ok(Response::new()
            .add_messages(messages)
            .add_attribute("action", "move_adapter_funds")
            .add_attribute("sender", info.sender)
            .add_attribute("from_adapter", from_adapter)
            .add_attribute("to_adapter", to_adapter)
            .add_attribute("denom", coin.denom)
            .add_attribute("amount", coin.amount))
    }
}

/// Calculates venue allocation based on registered adapters.
///
/// Smart allocation strategy:
/// - Iterates through adapters included in automated allocation in registration order (first to last)
/// - For deposits: queries AvailableForDeposit to check capacity
/// - For withdrawals: queries AvailableForWithdraw to check balance
/// - Distributes amount across multiple adapters if needed
/// - If 0 adapters included in automated allocation -> return empty vec (use contract balance)
///
/// # Arguments
/// * `deps` - Contract dependencies (for querying adapters)
/// * `env` - Environment (for inflow address)
/// * `amount` - Amount to allocate
/// * `denom` - Token denom
/// * `is_deposit` - Whether this is a deposit (true) or withdrawal (false)
///
/// # Returns
/// * `Ok(AllocationResult)` - Contains allocations and any failed adapter queries
struct AllocationResult {
    /// List of adapters with amounts to allocate
    allocations: Vec<(String, Uint128)>,
    /// Names of adapters whose queries failed
    failed_adapters: Vec<String>,
}

fn calculate_venues_allocation(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    amount: Uint128,
    denom: String,
    is_deposit: bool,
) -> Result<AllocationResult, ContractError> {
    let inflow_address = env.contract.address.to_string();

    // Get list of adapters with automated allocation (sorted by name for deterministic ordering)
    let automated_adapters: Vec<(String, AdapterInfo)> = ADAPTERS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|entry| match entry {
            Ok((name, info)) if matches!(info.allocation_mode, AllocationMode::Automated) => {
                Some((name, info))
            }
            _ => None,
        })
        .collect();

    if automated_adapters.is_empty() {
        // No adapters available for automated allocation - funds stay in contract
        return Ok(AllocationResult {
            allocations: vec![],
            failed_adapters: vec![],
        });
    }

    let mut allocations: Vec<(String, Uint128)> = Vec::new();
    let mut failed_adapters: Vec<String> = Vec::new();
    let mut remaining = amount;

    for (adapter_name, adapter_info) in automated_adapters {
        if remaining.is_zero() {
            break;
        }

        // Query adapter for available capacity/balance
        let query_msg = if is_deposit {
            AdapterInterfaceQueryMsg::AvailableForDeposit {
                depositor_address: inflow_address.clone(),
                denom: denom.clone(),
            }
        } else {
            AdapterInterfaceQueryMsg::AvailableForWithdraw {
                depositor_address: inflow_address.clone(),
                denom: denom.clone(),
            }
        };

        // Query the adapter - if it fails, track and skip to next adapter
        let available_result: Result<AvailableAmountResponse, _> = deps.querier.query_wasm_smart(
            adapter_info.address.to_string(),
            &AdapterInterfaceQuery {
                standard_query: &query_msg,
            },
        );

        match available_result {
            Ok(available_response) if available_response.amount > Uint128::zero() => {
                // Allocate the minimum of available and remaining
                let to_allocate = available_response.amount.min(remaining);
                allocations.push((adapter_name, to_allocate));
                remaining = remaining.checked_sub(to_allocate)?;
            }
            Ok(_) => {
                // Zero capacity - legitimate, no action needed
            }
            Err(_) => {
                // Query failed - track for visibility
                failed_adapters.push(adapter_name);
            }
        }
    }

    Ok(AllocationResult {
        allocations,
        failed_adapters,
    })
}

fn update_config(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    mut current_config: Config,
    config_update: UpdateConfigData,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Check if the sender is in the whitelist
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    let mut response = Response::new()
        .add_attribute("action", "update_config")
        .add_attribute("sender", info.sender);

    if let Some(max_withdrawals_per_user) = config_update.max_withdrawals_per_user {
        // Validate: must be at least 1 to prevent withdrawal freeze
        if max_withdrawals_per_user < 1 {
            return Err(new_generic_error(
                "max_withdrawals_per_user must be at least 1",
            ));
        }

        current_config.max_withdrawals_per_user = max_withdrawals_per_user;

        response = response.add_attribute(
            "max_withdrawals_per_user",
            max_withdrawals_per_user.to_string(),
        );
    }

    CONFIG.save(deps.storage, &current_config)?;

    Ok(response)
}

/// Converts the given amount of deposit token denomination into the amount denominated in base tokens.
fn convert_deposit_token_into_base_token(
    deps: &Deps<NeutronQuery>,
    config: &Config,
    amount: Uint128,
) -> StdResult<Uint128> {
    let ratio_to_base_token = get_token_ratio_to_base_token(deps, config)?.atomics();
    let denominator = Decimal::one().atomics();

    amount
        .checked_multiply_ratio(ratio_to_base_token, denominator)
        .map_err(|e| {
            StdError::generic_err(format!(
                "failed to convert deposit token into base token denomination: {e}"
            ))
        })
}

/// Converts the given amount of base token denomination into the amount denominated in deposit tokens.
fn convert_base_token_into_deposit_token(
    deps: &Deps<NeutronQuery>,
    config: &Config,
    amount_base_tokens: Uint128,
) -> StdResult<Uint128> {
    let ratio_to_base_token = get_token_ratio_to_base_token(deps, config)?.atomics();
    let numerator = Decimal::one().atomics();

    amount_base_tokens
        .checked_multiply_ratio(numerator, ratio_to_base_token)
        .map_err(|e| {
            StdError::generic_err(format!(
                "failed to convert base token into deposit token denomination: {e}"
            ))
        })
}

fn get_token_ratio_to_base_token(deps: &Deps<NeutronQuery>, config: &Config) -> StdResult<Decimal> {
    Ok(match &config.token_info_provider_contract {
        None => Decimal::one(),
        Some(token_info_provider_contract) => deps.querier.query_wasm_smart(
            token_info_provider_contract.to_string(),
            &TokenInfoProviderQueryMsg::RatioToBaseToken {
                denom: config.deposit_denom.clone(),
            },
        )?,
    })
}

fn build_update_deployed_amount_msg(
    diff_amount_in_base_tokens: Uint128,
    direction: DeploymentDirection,
    config: &Config,
) -> StdResult<CosmosMsg<NeutronMsg>> {
    let update_deployed_amount_msg = ControlCenterExecuteMsg::UpdateDeployedAmount {
        amount: diff_amount_in_base_tokens,
        direction,
    };

    Ok(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.control_center_contract.to_string(),
        msg: to_json_binary(&update_deployed_amount_msg)?,
        funds: vec![],
    }))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<NeutronQuery>, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(&deps)?),
        QueryMsg::PoolInfo {} => {
            to_json_binary(&get_pool_info(&deps, &env, load_config(deps.storage)?)?)
        }
        QueryMsg::SharesEquivalentValue { shares } => to_json_binary(
            &query_shares_equivalent_value(&deps, &load_config(deps.storage)?, shares)?,
        ),
        QueryMsg::UserSharesEquivalentValue { address } => {
            to_json_binary(&query_user_shares_equivalent_value(&deps, address)?)
        }
        QueryMsg::AvailableForDeployment {} => {
            to_json_binary(&query_available_for_deployment(&deps, &env)?)
        }
        QueryMsg::WithdrawalQueueInfo {} => to_json_binary(&query_withdrawal_queue_info(&deps)?),
        QueryMsg::AmountToFundPendingWithdrawals {} => {
            to_json_binary(&query_amount_to_fund_pending_withdrawals(&deps, &env)?)
        }
        QueryMsg::FundedWithdrawalRequests { limit } => {
            to_json_binary(&query_funded_withdrawal_requests(&deps, limit)?)
        }
        QueryMsg::UserWithdrawalRequests {
            address,
            start_from,
            limit,
        } => to_json_binary(&query_user_withdrawal_requests(
            &deps, address, start_from, limit,
        )?),
        QueryMsg::UserPayoutsHistory {
            address,
            start_from,
            limit,
            order,
        } => to_json_binary(&query_user_payouts_history(
            &deps, address, start_from, limit, order,
        )?),
        QueryMsg::Whitelist {} => to_json_binary(&query_whitelist(&deps)?),
        QueryMsg::ControlCenterPoolInfo {} => {
            let config = load_config(deps.storage)?;
            to_json_binary(&get_control_center_pool_info(
                &deps,
                &config.control_center_contract,
            )?)
        }
        QueryMsg::ListAdapters {} => to_json_binary(&query_list_adapters(&deps)?),
        QueryMsg::AdapterInfo { name } => to_json_binary(&query_adapter_info(&deps, name)?),
    }
}

fn query_config(deps: &Deps<NeutronQuery>) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: load_config(deps.storage)?,
    })
}

/// Returns the number of shares issued by this sub-vault.
fn query_shares_issued(deps: &Deps<NeutronQuery>) -> StdResult<Uint128> {
    Ok(deps
        .querier
        .query_supply(load_config(deps.storage)?.vault_shares_denom)?
        .amount)
}

/// Returns the value equivalent of a given amount of shares based on the current total shares and pool value.
/// Returned value is denominated in the deposit token.
fn query_shares_equivalent_value(
    deps: &Deps<NeutronQuery>,
    config: &Config,
    shares: Uint128,
) -> StdResult<Uint128> {
    let pool_info = get_control_center_pool_info(deps, &config.control_center_contract)?;
    let total_pool_value = pool_info.total_pool_value;
    let total_shares_issued = pool_info.total_shares_issued;

    calculate_shares_value(deps, config, shares, total_shares_issued, total_pool_value)
}

/// Returns the value equivalent of a user's shares by querying their balance and calculating its worth based on total shares and pool value.
fn query_user_shares_equivalent_value(
    deps: &Deps<NeutronQuery>,
    address: String,
) -> StdResult<Uint128> {
    let config = load_config(deps.storage)?;

    // Get the current balance of this address in the shares denom
    let shares_balance: Uint128 = deps
        .querier
        .query_balance(address, &config.vault_shares_denom)?
        .amount;

    query_shares_equivalent_value(deps, &config, shares_balance)
}

pub fn query_available_for_deployment(deps: &Deps<NeutronQuery>, env: &Env) -> StdResult<Uint128> {
    let config = load_config(deps.storage)?;
    let withdrawal_queue_info = load_withdrawal_queue_info(deps.storage)?;

    let contract_balance = deps
        .querier
        .query_balance(env.contract.address.as_str(), config.deposit_denom)?
        .amount;

    // If the total withdrawal amount exceeds the contract balance, then return zero
    Ok(contract_balance
        .checked_sub(withdrawal_queue_info.total_withdrawal_amount)
        .unwrap_or_default())
}

pub fn query_withdrawal_queue_info(
    deps: &Deps<NeutronQuery>,
) -> StdResult<WithdrawalQueueInfoResponse> {
    Ok(WithdrawalQueueInfoResponse {
        info: load_withdrawal_queue_info(deps.storage)?,
    })
}

pub fn query_amount_to_fund_pending_withdrawals(
    deps: &Deps<NeutronQuery>,
    env: &Env,
) -> StdResult<Uint128> {
    let config = load_config(deps.storage)?;
    let withdrawal_queue_info = load_withdrawal_queue_info(deps.storage)?;

    // Determine how much is already available to fund pending withdrawals
    let available_balance = get_balance_available_for_pending_withdrawals(
        deps,
        env.contract.address.as_ref(),
        &config.deposit_denom,
        &withdrawal_queue_info,
    )?;

    Ok(
        if available_balance >= withdrawal_queue_info.non_funded_withdrawal_amount {
            Uint128::zero()
        } else {
            withdrawal_queue_info
                .non_funded_withdrawal_amount
                .checked_sub(available_balance)?
        },
    )
}

fn query_funded_withdrawal_requests(
    deps: &Deps<NeutronQuery>,
    limit: u64,
) -> StdResult<FundedWithdrawalRequestsResponse> {
    // If no withdrawals have been funded yet, return an empty list. Otherwise, use last_funded_withdrawal_id
    // to restrict the query range in order to make it more gas efficient.
    let Some(last_funded_withdrawal_id) = LAST_FUNDED_WITHDRAWAL_ID.may_load(deps.storage)? else {
        return Ok(FundedWithdrawalRequestsResponse {
            withdrawal_ids: vec![],
        });
    };

    let withdrawal_ids = WITHDRAWAL_REQUESTS
        .range(
            deps.storage,
            None,
            Some(Bound::inclusive(last_funded_withdrawal_id)),
            Order::Ascending,
        )
        .take(limit as usize)
        .filter_map(|entry| match entry {
            Ok((_, withdrawal)) => {
                if withdrawal.is_funded {
                    Some(withdrawal.id)
                } else {
                    None
                }
            }
            Err(_) => None,
        })
        .collect::<Vec<u64>>();

    Ok(FundedWithdrawalRequestsResponse { withdrawal_ids })
}

pub fn query_user_withdrawal_requests(
    deps: &Deps<NeutronQuery>,
    address: String,
    start_from: u32,
    limit: u32,
) -> StdResult<UserWithdrawalRequestsResponse> {
    let user_address = deps.api.addr_validate(&address)?;

    let mut withdrawal_ids = USER_WITHDRAWAL_REQUESTS
        .may_load(deps.storage, user_address.clone())?
        .unwrap_or_default();

    // Make sure that the withdrawl request IDs are always returned in the same order
    withdrawal_ids.sort();

    let withdrawals = withdrawal_ids
        .into_iter()
        .skip(start_from as usize)
        .take(limit as usize)
        .filter_map(|withdrawal_id| WITHDRAWAL_REQUESTS.load(deps.storage, withdrawal_id).ok())
        .collect::<Vec<WithdrawalEntry>>();

    Ok(UserWithdrawalRequestsResponse { withdrawals })
}

pub fn query_user_payouts_history(
    deps: &Deps<NeutronQuery>,
    address: String,
    start_from: u32,
    limit: u32,
    order: Order,
) -> StdResult<UserPayoutsHistoryResponse> {
    let user_address = deps.api.addr_validate(&address)?;

    let payouts = PAYOUTS_HISTORY
        .prefix(user_address)
        .range(deps.storage, None, None, order)
        .skip(start_from as usize)
        .take(limit as usize)
        .filter_map(|entry| match entry {
            Ok((_, payout)) => Some(payout),
            Err(_) => None,
        })
        .collect::<Vec<PayoutEntry>>();

    Ok(UserPayoutsHistoryResponse { payouts })
}

fn query_whitelist(deps: &Deps<NeutronQuery>) -> StdResult<WhitelistResponse> {
    Ok(WhitelistResponse {
        whitelist: WHITELIST
            .keys(deps.storage, None, None, Order::Ascending)
            .filter_map(|w| w.ok())
            .collect(),
    })
}

// Adapter query functions
fn query_list_adapters(deps: &Deps<NeutronQuery>) -> StdResult<AdaptersListResponse> {
    let adapters = ADAPTERS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|entry| match entry {
            Ok((name, info)) => Some((name, info)),
            Err(_) => None,
        })
        .collect::<Vec<(String, AdapterInfo)>>();

    Ok(AdaptersListResponse { adapters })
}

fn query_adapter_info(deps: &Deps<NeutronQuery>, name: String) -> StdResult<AdapterInfoResponse> {
    let info = ADAPTERS
        .may_load(deps.storage, name.clone())?
        .ok_or_else(|| StdError::generic_err(format!("Adapter not found: {}", name)))?;

    Ok(AdapterInfoResponse { info })
}

/// Calculates the value of `shares` relative to the `total_pool_value` based on `total_shares_supply`.
/// Returned value is denominated in the deposit tokens.
/// Returns an error if the `shares` exceed supply. Returns zero if supply is zero.
/// Formula: (user_shares * total_pool_value) / total_shares_supply
fn calculate_shares_value(
    deps: &Deps<NeutronQuery>,
    config: &Config,
    shares: Uint128,
    total_shares_supply: Uint128,
    total_pool_value_base_tokens: Uint128,
) -> StdResult<Uint128> {
    if total_shares_supply.is_zero() {
        return Ok(Uint128::zero());
    }

    if shares > total_shares_supply {
        return Err(StdError::generic_err(format!("invalid shares amount; shares sent: {shares}, total shares supply: {total_shares_supply}")));
    }

    let shares_value_base_tokens = shares
        .checked_multiply_ratio(total_pool_value_base_tokens, total_shares_supply)
        .map_err(|e| StdError::generic_err(format!("overflow error: {e}")))?;

    convert_base_token_into_deposit_token(deps, config, shares_value_base_tokens)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    msg: Reply,
) -> Result<Response<NeutronMsg>, ContractError> {
    let reply_paylod = from_json::<ReplyPayload>(&msg.payload)?;

    match reply_paylod {
        ReplyPayload::CreateDenom {
            subdenom,
            metadata,
            initial_shares_recipient,
            initial_shares_amount,
        } => {
            // Full denom name, e.g. "factory/{inflow_contract_address}/hydro_inflow_atom"
            let full_denom = query_full_denom(deps.as_ref(), &env.contract.address, subdenom)?;

            CONFIG.update(deps.storage, |mut config| -> Result<_, ContractError> {
                config.vault_shares_denom = full_denom.denom.clone();

                Ok(config)
            })?;

            let metadata_msg = create_set_denom_metadata_msg(
                env.contract.address.into_string(),
                full_denom.denom.clone(),
                metadata,
            );

            // Mint initial shares to the recipient
            let mint_msg = NeutronMsg::submit_mint_tokens(
                full_denom.denom.clone(),
                initial_shares_amount,
                initial_shares_recipient.clone(),
            );

            Ok(Response::new()
                .add_message(metadata_msg)
                .add_message(mint_msg)
                .add_attribute("action", "reply_create_denom")
                .add_attribute("full_denom", full_denom.denom)
                .add_attribute("initial_shares_minted", initial_shares_amount)
                .add_attribute("initial_shares_recipient", initial_shares_recipient))
        }
    }
}

/// Creates MsgSetDenomMetadata that will set the metadata for the previously created `full_denom` token.
fn create_set_denom_metadata_msg(
    contract_address: String,
    full_denom: String,
    token_metadata: DenomMetadata,
) -> CosmosMsg<NeutronMsg> {
    CosmosMsg::Any(AnyMsg {
        type_url: "/osmosis.tokenfactory.v1beta1.MsgSetDenomMetadata".to_owned(),
        value: Binary::from(
            MsgSetDenomMetadata {
                sender: contract_address,
                metadata: Some(Metadata {
                    denom_units: vec![
                        DenomUnit {
                            denom: full_denom.clone(),
                            exponent: 0,
                            aliases: vec![],
                        },
                        DenomUnit {
                            denom: token_metadata.display.clone(),
                            exponent: token_metadata.exponent,
                            aliases: vec![],
                        },
                    ],
                    base: full_denom,
                    display: token_metadata.display,
                    name: token_metadata.name,
                    description: token_metadata.description,
                    symbol: token_metadata.symbol,
                    uri: token_metadata.uri.unwrap_or_default(),
                    uri_hash: token_metadata.uri_hash.unwrap_or_default(),
                }),
            }
            .encode_to_vec(),
        ),
    })
}

/// Queries all registered adapters to get the total amount deposited by this vault contract.
/// Returns the sum of all positions across all adapters for the given denom.
fn query_total_adapter_positions(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    deposit_denom: String,
) -> StdResult<Uint128> {
    let inflow_address = env.contract.address.to_string();
    let mut total_positions = Uint128::zero();

    // Iterate through all adapters
    let adapters: Vec<(String, AdapterInfo)> = ADAPTERS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|entry| entry.ok())
        .collect();

    for (_name, adapter_info) in adapters {
        // Skip adapters tracked in deployed amount to avoid double counting
        // These adapters are already counted in Control Center's DEPLOYED_AMOUNT
        if matches!(
            adapter_info.deployment_tracking,
            DeploymentTracking::Tracked
        ) {
            continue;
        }

        // Query each adapter for positions by this vault contract
        let query_msg = AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address: inflow_address.clone(),
            denom: deposit_denom.clone(),
        };

        // Query the adapter - if it fails, skip this adapter
        let result: Result<DepositorPositionResponse, _> = deps.querier.query_wasm_smart(
            adapter_info.address.to_string(),
            &AdapterInterfaceQuery {
                standard_query: &query_msg,
            },
        );

        if let Ok(response) = result {
            total_positions = total_positions.checked_add(response.amount)?;
        }
        // If query fails, we skip this adapter and continue
    }

    Ok(total_positions)
}

/// Returns the total value of the vault as well as the total number of shares issued by querying
/// the Control Center contract. The total pool value is denominated in base tokens (e.g. ATOM).
fn get_control_center_pool_info(
    deps: &Deps<NeutronQuery>,
    control_center_contract: &Addr,
) -> StdResult<ControlCenterPoolInfoResponse> {
    deps.querier.query_wasm_smart(
        control_center_contract.to_string(),
        &ControlCenterQueryMsg::PoolInfo {},
    )
}

/// Returns the deposit cap of the vault by querying the Control Center contract.
fn get_deposit_cap(
    deps: &Deps<NeutronQuery>,
    control_center_contract: &Addr,
) -> StdResult<Uint128> {
    Ok(deps
        .querier
        .query_wasm_smart::<ControlCenterConfigResponse>(
            control_center_contract.to_string(),
            &ControlCenterQueryMsg::Config {},
        )?
        .config
        .deposit_cap)
}

/// Returns information about this contract's pool including:
///     1. balance,
///     2. adapter deposits amount,
///     3. withdrawal queue amount and
///     4. total shares issued.
/// Balance, adapter deposits, and withdrawal queue amount values returned are denominated in base tokens (e.g. ATOM).
/// Intended to be used by the Control Center contract to query the pool values of all its sub-vaults.
fn get_pool_info(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    config: Config,
) -> StdResult<PoolInfoResponse> {
    let deposit_token_balance = deps
        .querier
        .query_balance(env.contract.address.clone(), config.deposit_denom.clone())?
        .amount;

    // Get the total amount held in adapters. This amount should be added to the
    // total pool value, since it is available but not counted in the deployed amount
    let adapter_deposits = query_total_adapter_positions(deps, env, config.deposit_denom.clone())?;

    // Get the total amount already requested for withdrawal. This amount should be
    // subtracted from the total pool value, since we cannot count on it anymore.
    let withdrawal_queue_amount = load_withdrawal_queue_info(deps.storage)?.total_withdrawal_amount;

    let shares_issued = query_shares_issued(deps)?;

    // Convert the values from deposit tokens into base tokens
    let ratio_to_base_token = get_token_ratio_to_base_token(deps, &config)?.atomics();
    let denominator = Decimal::one().atomics();

    let balance_base_tokens = deposit_token_balance
        .checked_multiply_ratio(ratio_to_base_token, denominator)
        .map_err(|e| {
            StdError::generic_err(format!(
                "failed to convert deposit token into base token representation: {e}"
            ))
        })?;

    let adapter_deposits_base_tokens = adapter_deposits
        .checked_multiply_ratio(ratio_to_base_token, denominator)
        .map_err(|e| {
            StdError::generic_err(format!(
                "failed to convert deposit token into base token representation: {e}"
            ))
        })?;

    let withdrawal_queue_base_tokens = withdrawal_queue_amount
        .checked_multiply_ratio(ratio_to_base_token, denominator)
        .map_err(|e| {
            StdError::generic_err(format!(
                "failed to convert deposit token into base token representation: {e}"
            ))
        })?;

    Ok(PoolInfoResponse {
        balance_base_tokens,
        adapter_deposits_base_tokens,
        withdrawal_queue_base_tokens,
        shares_issued,
    })
}

/// Updates the list of user's pending withdrawal requests by adding and/or removing specified withdrawal IDs.
fn update_user_withdrawal_requests_info(
    storage: &mut dyn Storage,
    user: Addr,
    config: &Config,
    withdrawals_to_add: Option<Vec<u64>>,
    withdrawals_to_remove: Option<Vec<u64>>,
    should_check_withdrawal_limit: bool,
) -> Result<(), ContractError> {
    USER_WITHDRAWAL_REQUESTS.update(
        storage,
        user.clone(),
        |current_withdrawals| -> Result<_, ContractError> {
            let mut current_withdrawals = current_withdrawals.unwrap_or_default();

            if let Some(withdrawals_to_add) = withdrawals_to_add {
                current_withdrawals.extend(withdrawals_to_add);
            }

            if let Some(withdrawals_to_remove) = withdrawals_to_remove {
                let withdrawals_to_remove: HashSet<u64> = HashSet::from_iter(withdrawals_to_remove);

                current_withdrawals.retain(|id| !withdrawals_to_remove.contains(id));
            }

            if should_check_withdrawal_limit
                && (current_withdrawals.len() as u64) > config.max_withdrawals_per_user
            {
                return Err(new_generic_error(format!(
                    "user {} has reached the maximum number of pending withdrawals: {}",
                    user, config.max_withdrawals_per_user
                )));
            }

            Ok(current_withdrawals)
        },
    )?;
    Ok(())
}

/// Updates the withdrawal queue info by applying the provided updates to its fields.
fn update_withdrawal_queue_info(
    storage: &mut dyn Storage,
    shares_burned_update: Option<Int128>,
    total_withdrawal_amount_update: Option<Int128>,
    non_funded_withdrawal_amount_update: Option<Int128>,
) -> Result<(), ContractError> {
    fn get_resulting_value(
        initial_value: Uint128,
        update: Int128,
    ) -> Result<Uint128, ContractError> {
        let current_value: Int128 = initial_value.try_into()?;

        current_value
            .checked_add(update)
            .map_err(|e| new_generic_error(format!("overflow error: {e}")))?
            .try_into()
            .map_err(|e: ConversionOverflowError| {
                new_generic_error(format!("conversion into Uint128 type failed; error: {e}"))
            })
    }

    let mut withdrawal_queue_info = load_withdrawal_queue_info(storage)?;

    if let Some(shares_burned_update) = shares_burned_update {
        withdrawal_queue_info.total_shares_burned = get_resulting_value(
            withdrawal_queue_info.total_shares_burned,
            shares_burned_update,
        )?;
    }

    if let Some(total_withdrawal_amount_update) = total_withdrawal_amount_update {
        withdrawal_queue_info.total_withdrawal_amount = get_resulting_value(
            withdrawal_queue_info.total_withdrawal_amount,
            total_withdrawal_amount_update,
        )?
    }

    if let Some(non_funded_withdrawal_amount_update) = non_funded_withdrawal_amount_update {
        withdrawal_queue_info.non_funded_withdrawal_amount = get_resulting_value(
            withdrawal_queue_info.non_funded_withdrawal_amount,
            non_funded_withdrawal_amount_update,
        )?
    }

    WITHDRAWAL_QUEUE_INFO.save(storage, &withdrawal_queue_info)?;

    Ok(())
}

/// Adds a new entry to the payout history for a user.
fn add_payout_history_entry(
    storage: &mut dyn Storage,
    env: &Env,
    recipient: &Addr,
    vault_shares_burned: Uint128,
    amount_received: Uint128,
) -> Result<(), ContractError> {
    let payout_id = get_next_payout_id(storage)?;

    let payout_entry = PayoutEntry {
        id: payout_id,
        executed_at: env.block.time,
        recipient: recipient.clone(),
        vault_shares_burned,
        amount_received,
    };

    PAYOUTS_HISTORY.save(storage, (recipient.clone(), payout_id), &payout_entry)?;

    Ok(())
}

/// Calculates the balance available for funding pending withdrawals by subtracting the amount
/// already allocated for earlier funded withdrawal requests from the current contract balance.
fn get_balance_available_for_pending_withdrawals(
    deps: &Deps<NeutronQuery>,
    contract_address: &str,
    deposit_denom: &str,
    withdrawal_queue_info: &WithdrawalQueueInfo,
) -> StdResult<Uint128> {
    let contract_balance = deps
        .querier
        .query_balance(contract_address, deposit_denom)?
        .amount;

    // We cannot count on the tokens that were provided earlier for withdrawals but haven't been paid out
    // to the users yet, so we only consider the portion of the contract balance that exceeds this amount.
    let earlier_funded_withdrawal_amount = withdrawal_queue_info
        .total_withdrawal_amount
        .checked_sub(withdrawal_queue_info.non_funded_withdrawal_amount)?;

    // Return the difference, or zero if the earlier funded amount exceeds the contract balance.
    Ok(contract_balance
        .checked_sub(earlier_funded_withdrawal_amount)
        .unwrap_or_default())
}

/// Converts a slice of items into a comma-separated string of their string representations.
pub fn get_slice_as_attribute<T: ToString>(input: &[T]) -> String {
    input
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<String>>()
        .join(",")
}
