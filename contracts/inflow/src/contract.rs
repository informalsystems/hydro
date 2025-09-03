use std::{
    collections::{HashMap, HashSet},
    env, vec,
};

use cosmos_sdk_proto::cosmos::bank::v1beta1::{DenomUnit, Metadata};
use cosmwasm_std::{
    entry_point, from_json, to_json_binary, to_json_vec, Addr, AnyMsg, BankMsg, Binary, Coin,
    ConversionOverflowError, CosmosMsg, Deps, DepsMut, Env, Int128, MessageInfo, Order, Reply,
    Response, StdError, StdResult, Storage, SubMsg, Uint128,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;
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
        UserWithdrawalRequestsResponse, WithdrawalQueueInfoResponse,
    },
    state::{
        get_next_payout_id, get_next_withdrawal_id, load_config, load_withdrawal_queue_info,
        Config, PayoutEntry, WithdrawalEntry, WithdrawalQueueInfo, CONFIG, DEPLOYED_AMOUNT,
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
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    CONFIG.save(
        deps.storage,
        &Config {
            deposit_denom: msg.deposit_denom.clone(),
            vault_shares_denom: String::new(),
            max_withdrawals_per_user: msg.max_withdrawals_per_user,
            deposit_cap: msg.deposit_cap,
        },
    )?;

    DEPLOYED_AMOUNT.save(deps.storage, &Uint128::zero(), env.block.height)?;
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
        ExecuteMsg::Deposit {} => deposit(deps, env, info, &config),
        ExecuteMsg::Withdraw {} => withdraw(deps, env, info, &config),
        ExecuteMsg::CancelWithdrawal { withdrawal_ids } => {
            cancel_withdrawal(deps, info, &config, withdrawal_ids)
        }
        ExecuteMsg::FulfillPendingWithdrawals { limit } => {
            fulfill_pending_withdrawals(deps, env, info, &config, limit)
        }
        ExecuteMsg::ClaimUnbondedWithdrawals { withdrawal_ids } => {
            claim_unbonded_withdrawals(deps, env, info, &config, withdrawal_ids)
        }
        ExecuteMsg::WithdrawForDeployment { amount } => {
            withdraw_for_deployment(deps, env, info, &config, amount)
        }
        ExecuteMsg::AddToWhitelist { address } => add_to_whitelist(deps, env, info, address),
        ExecuteMsg::RemoveFromWhitelist { address } => {
            remove_from_whitelist(deps, env, info, address)
        }
        ExecuteMsg::SubmitDeployedAmount { amount } => {
            submit_deployed_amount(deps, env, info, amount)
        }
        ExecuteMsg::UpdateConfig { config } => update_config(deps, info, config),
    }
}

// Deposits tokens accepted by the vault and issues certain amount of vault shares tokens in return.
fn deposit(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: &Config,
) -> Result<Response<NeutronMsg>, ContractError> {
    let deposit_amount = cw_utils::must_pay(&info, &config.deposit_denom)?;

    let total_pool_value =
        get_total_pool_value(&deps.as_ref(), &env, config.deposit_denom.clone())?;
    if total_pool_value > config.deposit_cap {
        return Err(new_generic_error("deposit cap has been reached"));
    }

    let vault_shares_to_mint =
        calculate_number_of_shares_to_mint(&deps.as_ref(), &env, config, deposit_amount)?;

    let mint_vault_shares_msg = NeutronMsg::submit_mint_tokens(
        &config.vault_shares_denom,
        vault_shares_to_mint,
        &info.sender,
    );

    Ok(Response::new()
        .add_message(mint_vault_shares_msg)
        .add_attribute("action", "deposit")
        .add_attribute("sender", info.sender)
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
        query_shares_equivalent_value(&deps.as_ref(), &env, vault_shares_sent)?;

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
// have been provided to the smart contract. Users will receive back the vault shares tokens they
// had previously sent to the contract when creating the withdrawal requests that are now being canceled.
fn cancel_withdrawal(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    config: &Config,
    withdrawal_ids: Vec<u64>,
) -> Result<Response<NeutronMsg>, ContractError> {
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
        config,
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

    // Mint back the vault shares tokens that were burned when the withdrawal requests were created
    let mint_vault_shares_msg =
        NeutronMsg::submit_mint_tokens(&config.vault_shares_denom, shares_burned, &info.sender);

    Ok(response
        .add_message(mint_vault_shares_msg)
        .add_attribute("canceled_withdrawal_amount", amount_to_withdraw)
        .add_attribute("shares_minted_back", shares_burned))
}

// Permissionless action that iterates over the withdrawal requests queue and marks as funded all
// those withdrawal requests that can be paid out with the funds held by the contract, but also
// considering the funds already allocated for earlier requests that have not been paid out yet.
fn fulfill_pending_withdrawals(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: &Config,
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

    if available_balance.is_zero() {
        return Ok(response
            .add_attribute("funded_withdrawal_ids", "")
            .add_attribute("total_amount_funded", Uint128::zero()));
    }

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
    config: &Config,
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
            config,
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
    config: &Config,
    amount: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    let available_for_deployment = query_available_for_deployment(&deps.as_ref(), &env)?;
    if available_for_deployment.is_zero() {
        return Err(new_generic_error("no funds are available for deployment"));
    }

    // If the requested amount exceeds the available amount, withdraw only what is available.
    let amount_to_withdraw = available_for_deployment.min(amount);

    // We can update the deployed amount immediately, since we know it is now transferred to the multisig.
    DEPLOYED_AMOUNT.update(deps.storage, env.block.height, |current_value| {
        current_value
            .unwrap_or_default()
            .checked_add(amount_to_withdraw)
            .map_err(|e| new_generic_error(format!("overflow error: {e}")))
    })?;

    let send_tokens_msg = BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![Coin {
            amount: amount_to_withdraw,
            denom: config.deposit_denom.clone(),
        }],
    };

    Ok(Response::new()
        .add_message(send_tokens_msg)
        .add_attribute("action", "withdraw_for_deployment")
        .add_attribute("sender", info.sender)
        .add_attribute("amount_requested", amount)
        .add_attribute("amount_withdrawn", amount_to_withdraw))
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

/// Given the `deposit_amount`, this function will calculate how many vault shares tokens should be minted in return.
pub fn calculate_number_of_shares_to_mint(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    config: &Config,
    deposit_amount: Uint128,
) -> Result<Uint128, ContractError> {
    let contract_deposit_token_balance = deps
        .querier
        .query_balance(
            env.contract.address.to_string(),
            config.deposit_denom.clone(),
        )?
        .amount;

    let deployed_amount = DEPLOYED_AMOUNT.load(deps.storage)?;
    let withdrawal_queue_amount = load_withdrawal_queue_info(deps.storage)?.total_withdrawal_amount;

    // `deposit_amount` has already been added to the smart contract balance even before `execute()` is called,
    // so we need to subtract it here in order to accurately calculate number of vault shares to mint.
    // We also need to add the amount already deployed and subtract the amount requested for withdrawal.
    let deposit_token_current_balance = contract_deposit_token_balance
        .checked_sub(deposit_amount)?
        .checked_add(deployed_amount)?
        .checked_sub(withdrawal_queue_amount)
        .unwrap_or_default();

    let total_shares_issued = query_total_shares_issued(deps)?;

    // If there are currently no vault shares minted, then vault shares have 1:1 ratio with the deposit token.
    if deposit_token_current_balance.is_zero() || total_shares_issued.is_zero() {
        return Ok(deposit_amount);
    }

    deposit_amount
        .checked_multiply_ratio(total_shares_issued, deposit_token_current_balance)
        .map_err(|e| new_generic_error(format!("overflow error: {e}")))
}

fn submit_deployed_amount(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Check if the sender is in the whitelist
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    // Save the deployed amount snapshot at current height
    DEPLOYED_AMOUNT.save(deps.storage, &amount, env.block.height)?;

    Ok(Response::new()
        .add_attribute("action", "submit_deployed_amount")
        .add_attribute("sender", info.sender)
        .add_attribute("amount", amount))
}

fn update_config(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    config_update: UpdateConfigData,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Check if the sender is in the whitelist
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    let mut config = load_config(deps.storage)?;

    let mut response = Response::new()
        .add_attribute("action", "update_config")
        .add_attribute("sender", info.sender);

    if let Some(max_withdrawals_per_user) = config_update.max_withdrawals_per_user {
        config.max_withdrawals_per_user = max_withdrawals_per_user;

        response = response.add_attribute(
            "max_withdrawals_per_user",
            max_withdrawals_per_user.to_string(),
        );
    }

    if let Some(deposit_cap) = config_update.deposit_cap {
        config.deposit_cap = deposit_cap;

        response = response.add_attribute("deposit_cap", deposit_cap.to_string());
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(response)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<NeutronQuery>, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(&deps)?),
        QueryMsg::TotalSharesIssued {} => to_json_binary(&query_total_shares_issued(&deps)?),
        QueryMsg::TotalPoolValue {} => to_json_binary(&query_total_pool_value(&deps, &env)?),
        QueryMsg::SharesEquivalentValue { shares } => {
            to_json_binary(&query_shares_equivalent_value(&deps, &env, shares)?)
        }
        QueryMsg::UserSharesEquivalentValue { address } => {
            to_json_binary(&query_user_shares_equivalent_value(&deps, &env, address)?)
        }
        QueryMsg::DeployedAmount {} => to_json_binary(&query_deployed_amount(&deps)?),
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
    }
}

fn query_config(deps: &Deps<NeutronQuery>) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: load_config(deps.storage)?,
    })
}

fn query_total_shares_issued(deps: &Deps<NeutronQuery>) -> StdResult<Uint128> {
    Ok(deps
        .querier
        .query_supply(load_config(deps.storage)?.vault_shares_denom)?
        .amount)
}

fn query_total_pool_value(deps: &Deps<NeutronQuery>, env: &Env) -> StdResult<Uint128> {
    get_total_pool_value(deps, env, load_config(deps.storage)?.deposit_denom)
}

/// Returns the value equivalent of a given amount of shares based on the current total shares and pool value.
fn query_shares_equivalent_value(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    shares: Uint128,
) -> StdResult<Uint128> {
    let total_shares_supply = query_total_shares_issued(deps)?;
    let total_pool_value = query_total_pool_value(deps, env)?;

    calculate_shares_value(shares, total_shares_supply, total_pool_value)
}

/// Returns the value equivalent of a user's shares by querying their balance and calculating its worth based on total shares and pool value.
fn query_user_shares_equivalent_value(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    address: String,
) -> StdResult<Uint128> {
    let shares_denom = load_config(deps.storage)?.vault_shares_denom.clone();

    // Get the current balance of this address in the shares denom
    let shares_balance: Uint128 = deps.querier.query_balance(address, shares_denom)?.amount;

    query_shares_equivalent_value(deps, env, shares_balance)
}

fn query_deployed_amount(deps: &Deps<NeutronQuery>) -> StdResult<Uint128> {
    Ok(DEPLOYED_AMOUNT
        .may_load(deps.storage)?
        .unwrap_or_else(Uint128::zero))
}

pub fn query_available_for_deployment(deps: &Deps<NeutronQuery>, env: &Env) -> StdResult<Uint128> {
    let config = load_config(deps.storage)?;
    let withdrawal_queue_info = load_withdrawal_queue_info(deps.storage)?;

    let contract_balance = deps
        .querier
        .query_balance(env.contract.address.as_str(), config.deposit_denom)?
        .amount;

    Ok(
        if contract_balance <= withdrawal_queue_info.total_withdrawal_amount {
            Uint128::zero()
        } else {
            contract_balance.checked_sub(withdrawal_queue_info.total_withdrawal_amount)?
        },
    )
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

/// Calculates the value of `shares` relative to the `total_pool_value` based on `total_shares_supply`.
/// Returns an error if the `shares` exceed supply. Returns zero if supply is zero.
/// Formula: (user_shares * total_pool_value) / total_shares_supply
fn calculate_shares_value(
    shares: Uint128,
    total_shares_supply: Uint128,
    total_pool_value: Uint128,
) -> StdResult<Uint128> {
    if total_shares_supply.is_zero() {
        return Ok(Uint128::zero());
    }

    if shares > total_shares_supply {
        return Err(StdError::generic_err(format!("invalid shares amount; shares sent: {shares}, total shares supply: {total_shares_supply}")));
    }

    shares
        .checked_multiply_ratio(total_pool_value, total_shares_supply)
        .map_err(|e| StdError::generic_err(format!("overflow error: {e}")))
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

fn get_total_pool_value(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    deposit_denom: String,
) -> StdResult<Uint128> {
    // Get the current balance of this contract in the deposit denom
    let balance: Coin = deps
        .querier
        .query_balance(env.contract.address.clone(), deposit_denom.clone())?;

    // Get the total deployed amount (from snapshot storage)
    let deployed_amount = DEPLOYED_AMOUNT.may_load(deps.storage)?;

    // Get the total amount already requested for withdrawal. This amount should be
    // subtracted from the total pool value, since we cannot count on it anymore.
    let withdrawal_queue_amount = load_withdrawal_queue_info(deps.storage)?.total_withdrawal_amount;

    Ok(balance
        .amount
        .checked_add(deployed_amount.unwrap_or_default())?
        .checked_sub(withdrawal_queue_amount)
        .unwrap_or_default())
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
            .map_err(|e| new_generic_error(format!("overflow error: {}", e)))?
            .try_into()
            .map_err(|e: ConversionOverflowError| {
                new_generic_error(format!("conversion into Uint128 type failed; error: {}", e))
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

    if contract_balance <= earlier_funded_withdrawal_amount {
        return Ok(Uint128::zero());
    }

    Ok(contract_balance.checked_sub(earlier_funded_withdrawal_amount)?)
}

/// Converts a slice of items into a comma-separated string of their string representations.
pub fn get_slice_as_attribute<T: ToString>(input: &[T]) -> String {
    input
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<String>>()
        .join(",")
}
