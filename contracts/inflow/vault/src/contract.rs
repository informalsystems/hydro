use std::{
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
    inflow::PoolInfoResponse,
    inflow_control_center::{
        ConfigResponse as ControlCenterConfigResponse, ExecuteMsg as ControlCenterExecuteMsg,
        PoolInfoResponse as ControlCenterPoolInfoResponse, QueryMsg as ControlCenterQueryMsg,
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
    msg::{DenomMetadata, ExecuteMsg, InstantiateMsg, ReplyPayload, UpdateConfigData},
    query::{
        ConfigResponse, FundedWithdrawalRequestsResponse, QueryMsg, UserPayoutsHistoryResponse,
        UserWithdrawalRequestsResponse, WhitelistResponse, WithdrawalQueueInfoResponse,
    },
    state::{
        get_next_payout_id, get_next_withdrawal_id, load_config, load_withdrawal_queue_info,
        Config, PayoutEntry, WithdrawalEntry, WithdrawalQueueInfo, CONFIG,
        LAST_FUNDED_WITHDRAWAL_ID, NEXT_PAYOUT_ID, NEXT_WITHDRAWAL_ID, PAYOUTS_HISTORY,
        USER_WITHDRAWAL_REQUESTS, WHITELIST, WITHDRAWAL_QUEUE_INFO, WITHDRAWAL_REQUESTS,
    },
};

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const UNUSED_MSG_ID: u64 = 0;

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
        ))
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
        ExecuteMsg::Withdraw {} => withdraw(deps, env, info, &config),
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
    }
}

// Deposits tokens accepted by the vault and issues certain amount of vault shares tokens in return.
fn deposit(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
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

    let mint_vault_shares_msg = NeutronMsg::submit_mint_tokens(
        &config.vault_shares_denom,
        vault_shares_to_mint,
        recipient.to_string(),
    );

    Ok(Response::new()
        .add_message(mint_vault_shares_msg)
        .add_attribute("action", "deposit")
        .add_attribute("sender", info.sender)
        .add_attribute("recipient", recipient.to_string())
        .add_attribute("deposit_amount", deposit_amount)
        .add_attribute("vault_shares_minted", vault_shares_to_mint))
}

// User initiates withdrawal request by sending a certain number of vault shares tokens to the contract
// in order to redeem them for the underlying deposit tokens. If the contract has enough balance
// to cover the entire amount from existing withdrawal queue plus the new request, user will
// receive deposit tokens immediately. Otherwise, the request will be added to the withdrawal queue.
// In both cases, the vault shares tokens sent by the user will be burned immediately.
fn withdraw(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: &Config,
) -> Result<Response<NeutronMsg>, ContractError> {
    let vault_shares_denom = config.vault_shares_denom.clone();
    let vault_shares_sent = cw_utils::must_pay(&info, &vault_shares_denom)?;

    // Calculate how many deposit tokens the sent vault shares are worth
    let amount_to_withdraw =
        query_shares_equivalent_value(&deps.as_ref(), config, vault_shares_sent)?;

    if amount_to_withdraw.is_zero() {
        return Err(new_generic_error("cannot withdraw zero amount"));
    }

    // Get the current contract balance of the deposit token
    let contract_balance = deps
        .querier
        .query_balance(
            env.contract.address.to_string(),
            config.deposit_denom.clone(),
        )?
        .amount;

    // Get the total amount already requested for withdrawal
    let withdrawal_queue_amount = load_withdrawal_queue_info(deps.storage)?.total_withdrawal_amount;

    let mut response = Response::new()
        .add_attribute("action", "withdraw")
        .add_attribute("sender", info.sender.clone())
        .add_attribute("vault_shares_sent", vault_shares_sent);

    // If the contract has enough balance to cover the entire withdrawal queue amount
    // plus the new request, send the tokens to the user immediately.
    if withdrawal_queue_amount.checked_add(amount_to_withdraw)? <= contract_balance {
        response = response
            .add_message(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: vec![Coin::new(amount_to_withdraw, config.deposit_denom.clone())],
            })
            .add_attribute("paid_out_amount", amount_to_withdraw);

        // Add entry to the payout history
        add_payout_history_entry(
            deps.storage,
            &env,
            &info.sender,
            vault_shares_sent,
            amount_to_withdraw,
        )?;
    } else {
        let withdrawal_id = get_next_withdrawal_id(deps.storage)?;

        let withdrawal_entry = WithdrawalEntry {
            id: withdrawal_id,
            initiated_at: env.block.time,
            withdrawer: info.sender.clone(),
            shares_burned: vault_shares_sent,
            amount_to_receive: amount_to_withdraw,
            is_funded: false,
        };

        // Add the new withdrawal entry to the queue
        WITHDRAWAL_REQUESTS.save(deps.storage, withdrawal_id, &withdrawal_entry)?;

        // Update the withdrawal queue info
        update_withdrawal_queue_info(
            deps.storage,
            Some(Int128::try_from(vault_shares_sent)?),
            Some(Int128::try_from(amount_to_withdraw)?),
            Some(Int128::try_from(amount_to_withdraw)?),
        )?;

        // Add the new withdrawal id to the list of user's withdrawal requests
        update_user_withdrawal_requests_info(
            deps.storage,
            info.sender.clone(),
            config,
            Some(vec![withdrawal_id]),
            None,
            true,
        )?;

        response = response
            .add_attribute("withdrawal_id", withdrawal_id.to_string())
            .add_attribute("amount_queued_for_withdrawal", amount_to_withdraw);
    }

    // Burn the vault shares tokens sent by the user
    let burn_shares_msg = NeutronMsg::submit_burn_tokens(&vault_shares_denom, vault_shares_sent);

    Ok(response.add_message(burn_shares_msg))
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
    let shares_to_mint = calculate_number_of_shares_to_mint(
        amount_to_withdraw_base_tokens,
        total_pool_value,
        total_shares_issued,
    )?;

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

    let update_deployed_amount_msg =
        build_update_deployed_amount_msg(amount_to_withdraw_in_base_tokens, &config)?;

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
    deployed_amount_in_base_tokens: Uint128,
    config: &Config,
) -> StdResult<CosmosMsg<NeutronMsg>> {
    let update_deployed_amount_msg = ControlCenterExecuteMsg::UpdateDeployedAmount {
        amount: deployed_amount_in_base_tokens,
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
        ReplyPayload::CreateDenom { subdenom, metadata } => {
            // Full denom name, e.g. "factory/{inflow_contract_address}/hydro_inflow_atom"
            let full_denom = query_full_denom(deps.as_ref(), &env.contract.address, subdenom)?;

            CONFIG.update(deps.storage, |mut config| -> Result<_, ContractError> {
                config.vault_shares_denom = full_denom.denom.clone();

                Ok(config)
            })?;

            let msg = create_set_denom_metadata_msg(
                env.contract.address.into_string(),
                full_denom.denom.clone(),
                metadata,
            );

            Ok(Response::new()
                .add_message(msg)
                .add_attribute("action", "reply_create_denom")
                .add_attribute("full_denom", full_denom.denom))
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
///     1. balance
///     2. withdrawal queue amount and
///     3. total shares issued.
/// Balance and withdrawal queue amount values returned are denominated in base tokens (e.g. ATOM).
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

    let withdrawal_queue_base_tokens = withdrawal_queue_amount
        .checked_multiply_ratio(ratio_to_base_token, denominator)
        .map_err(|e| {
            StdError::generic_err(format!(
                "failed to convert deposit token into base token representation: {e}"
            ))
        })?;

    Ok(PoolInfoResponse {
        balance_base_tokens,
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
