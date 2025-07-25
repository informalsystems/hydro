use std::collections::{HashMap, HashSet};

use cosmwasm_std::{
    from_json, to_json_vec, Attribute, Decimal256, SubMsg, SubMsgResult, Uint256, WasmMsg,
};
// entry_point is being used but for some reason clippy doesn't see that, hence the allow attribute here
#[allow(unused_imports)]
use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Decimal, Deps, DepsMut, Env,
    MessageInfo, Order, Reply, Response, StdError, StdResult, Storage, Timestamp, Uint128,
};
use cw2::set_contract_version;
use cw_utils::must_pay;
use interface::drop_puppeteer::{DelegationsResponse, PuppeteerQueryMsg, QueryExtMsg};
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;
use neutron_sdk::interchain_queries::v047::register_queries::new_register_staking_validators_query_msg;
use neutron_sdk::sudo::msg::SudoMsg;

use crate::cw721;
use crate::error::{new_generic_error, ContractError};
use crate::gatekeeper::{
    build_gatekeeper_lock_tokens_msg, build_init_gatekeeper_msg, gatekeeper_handle_submsg_reply,
};
use crate::governance::{query_total_power_at_height, query_voting_power_at_height};
use crate::lsm_integration::{query_ibc_denom_trace, COSMOS_VALIDATOR_PREFIX, TRANSFER_PORT};
use crate::msg::{
    CollectionInfo, ConvertLockupPayload, ExecuteMsg, InstantiateMsg, LiquidityDeployment,
    LockTokensProof, ProposalToLockups, ReplyPayload, TokenInfoProviderInstantiateMsg, TrancheInfo,
};
use crate::query::{
    AllUserLockupsResponse, AllUserLockupsWithTrancheInfosResponse, AllVotesResponse,
    CanLockDenomResponse, ConstantsResponse, CurrentRoundResponse, DtokenAmountResponse,
    DtokenAmountsResponse, ExpiredUserLockupsResponse, GatekeeperResponse, ICQManagersResponse,
    LiquidityDeploymentResponse, LockEntryWithPower, LockVotesHistoryEntry,
    LockVotesHistoryResponse, LockupWithPerTrancheInfo, ProposalResponse, QueryMsg,
    RegisteredValidatorQueriesResponse, RoundEndResponse, RoundProposalsResponse,
    RoundTotalVotingPowerResponse, RoundTrancheLiquidityDeploymentsResponse,
    SpecificUserLockupsResponse, SpecificUserLockupsWithTrancheInfosResponse,
    TokenInfoProvidersResponse, TopNProposalsResponse, TotalLockedTokensResponse, TranchesResponse,
    UserVotedLocksResponse, UserVotesResponse, UserVotingPowerResponse, VoteEntry, VotedLockInfo,
    WhitelistAdminsResponse, WhitelistResponse,
};
use crate::score_keeper::{
    add_token_group_shares_to_proposal, add_token_group_shares_to_round_total,
    apply_proposal_changes, apply_token_groups_ratio_changes, combine_proposal_power_updates,
    get_total_power_for_proposal, get_total_power_for_round,
    remove_token_group_shares_from_proposal, TokenGroupRatioChange,
};
use crate::state::{
    Constants, DropTokenInfo, LockEntryV2, Proposal, RoundLockPowerSchedule, Tranche,
    ValidatorInfo, Vote, VoteWithPower, CONSTANTS, DROP_TOKEN_INFO, GATEKEEPER, ICQ_MANAGERS,
    LIQUIDITY_DEPLOYMENTS_MAP, LOCKED_TOKENS, LOCKS_MAP_V2, LOCK_ID, LOCK_ID_EXPIRY,
    LOCK_ID_TRACKING, PROPOSAL_MAP, PROPS_BY_SCORE, PROP_ID, REVERSE_LOCK_ID_TRACKING,
    SNAPSHOTS_ACTIVATION_HEIGHT, TOKEN_INFO_PROVIDERS, TRANCHE_ID, TRANCHE_MAP, USER_LOCKS,
    USER_LOCKS_FOR_CLAIM, VALIDATORS_INFO, VALIDATORS_PER_ROUND, VALIDATORS_STORE_INITIALIZED,
    VALIDATOR_TO_QUERY_ID, VOTE_MAP_V1, VOTE_MAP_V2, VOTING_ALLOWED_ROUND, WHITELIST,
    WHITELIST_ADMINS,
};
use crate::token_manager::{
    add_token_info_providers, handle_token_info_provider_add_remove,
    token_manager_handle_submsg_reply, TokenManager,
};
use crate::utils::{
    calculate_vote_power, get_current_user_voting_power, get_higest_voting_allowed_round,
    get_highest_known_height_for_round_id, get_lock_time_weighted_shares, get_lock_vote,
    get_next_lock_id, get_owned_lock_entry, get_proposal, get_user_claimable_locks,
    load_constants_active_at_timestamp, load_current_constants, run_on_each_transaction,
    scale_lockup_power, to_lockup_with_power, to_lockup_with_tranche_infos,
    update_locked_tokens_info, validate_locked_tokens_caps, verify_historical_data_availability,
};
use crate::validators_icqs::{
    build_create_interchain_query_submsg, handle_delivered_interchain_query_result,
    handle_submsg_reply, query_min_interchain_query_deposit,
};
use crate::vote::{
    process_unvotes, process_votes, validate_proposals_and_locks_for_voting, ProcessUnvotesResult,
    ProcessVotesResult, VoteProcessingContext,
};
use interface::drop_core::ExecuteMsg as DropExecuteMsg;
use interface::drop_core::QueryMsg as DropQueryMsg;

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const MAX_LOCK_ENTRIES: usize = 100;

pub const NATIVE_TOKEN_DENOM: &str = "untrn";

pub const MIN_DEPLOYMENT_DURATION: u64 = 1;

pub const MIN_SPLIT_LOCK_SIZE: Uint128 = Uint128::new(10_000);

const UNUSED_MSG_ID: u64 = 0;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // validate that the lock epoch length is not shorter than the round length
    if msg.lock_epoch_length < msg.round_length {
        return Err(ContractError::Std(StdError::generic_err(
            "Lock epoch length must not be shorter than the round length",
        )));
    }

    let cw721_collection_info = msg.cw721_collection_info.unwrap_or(CollectionInfo {
        name: "Hydro Lockups".to_string(),
        symbol: "hydro-lockups".to_string(),
    });

    let state = Constants {
        round_length: msg.round_length,
        lock_epoch_length: msg.lock_epoch_length,
        first_round_start: msg.first_round_start,
        max_locked_tokens: msg.max_locked_tokens.u128(),
        known_users_cap: 0,
        max_deployment_duration: msg.max_deployment_duration,
        paused: false,
        round_lock_power_schedule: RoundLockPowerSchedule::new(msg.round_lock_power_schedule),
        cw721_collection_info,
        lock_expiry_duration_seconds: msg.lock_expiry_duration_seconds,
        lock_depth_limit: msg.lock_depth_limit,
    };

    CONSTANTS.save(deps.storage, env.block.time.nanos(), &state)?;
    LOCKED_TOKENS.save(deps.storage, &0)?;
    LOCK_ID.save(deps.storage, &0)?;
    PROP_ID.save(deps.storage, &0)?;

    let mut whitelist_admins: Vec<Addr> = vec![];
    let mut whitelist: Vec<Addr> = vec![];
    for admin in msg.whitelist_admins {
        let admin_addr = deps.api.addr_validate(&admin)?;
        if !whitelist_admins.contains(&admin_addr) {
            whitelist_admins.push(admin_addr.clone());
        }
    }
    for whitelist_account in msg.initial_whitelist {
        let whitelist_account_addr = deps.api.addr_validate(&whitelist_account)?;
        if !whitelist.contains(&whitelist_account_addr) {
            whitelist.push(whitelist_account_addr.clone());
        }
    }

    for manager in msg.icq_managers {
        let manager_addr = deps.api.addr_validate(&manager)?;
        ICQ_MANAGERS.save(deps.storage, manager_addr, &true)?;
    }

    WHITELIST_ADMINS.save(deps.storage, &whitelist_admins)?;
    WHITELIST.save(deps.storage, &whitelist)?;

    // For each tranche, create a tranche in the TRANCHE_MAP
    let mut tranches = std::collections::HashSet::new();
    let mut tranche_id = 1;

    for tranche_info in msg.tranches {
        let tranche_name = tranche_info.name.trim().to_string();

        if !tranches.insert(tranche_name.clone()) {
            return Err(ContractError::Std(StdError::generic_err(
                "Duplicate tranche name found in provided tranches, but tranche names must be unique.",
            )));
        }

        let tranche = Tranche {
            id: tranche_id,
            name: tranche_name,
            metadata: tranche_info.metadata,
        };
        TRANCHE_MAP.save(deps.storage, tranche_id, &tranche)?;
        tranche_id += 1;
    }

    // Store ID to be used for the next tranche
    TRANCHE_ID.save(deps.storage, &tranche_id)?;

    let mut submsgs = vec![];

    // Save token info providers into the store and build SubMsgs to instantiate contracts, if there are any needed
    let (token_info_provider_init_msgs, _) =
        add_token_info_providers(&mut deps, msg.token_info_providers)?;
    submsgs.extend(token_info_provider_init_msgs);

    // Prepare Gatekeeper instantiation SubMsg
    if let Some(init_gatekeeper_msg) = build_init_gatekeeper_msg(&msg.gatekeeper)? {
        submsgs.push(init_gatekeeper_msg);
    }

    // the store for the first round is already initialized, since there is no previous round to copy information over from.
    VALIDATORS_STORE_INITIALIZED.save(deps.storage, 0, &true)?;

    SNAPSHOTS_ACTIVATION_HEIGHT.save(deps.storage, &env.block.height)?;

    Ok(Response::new()
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender.clone())
        .add_submessages(submsgs))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    let constants = load_current_constants(&deps.as_ref(), &env)?;

    // Since un-pausing can only be done through contract migration,
    // we can check that the contract is not paused within execute.
    if constants.paused {
        return Err(ContractError::Paused);
    }

    let current_round = compute_current_round_id(&env, &constants)?;
    run_on_each_transaction(deps.storage, &env, current_round)?;

    match msg {
        ExecuteMsg::LockTokens {
            lock_duration,
            proof,
        } => lock_tokens(deps, env, info, &constants, lock_duration, proof),
        ExecuteMsg::RefreshLockDuration {
            lock_ids,
            lock_duration,
        } => refresh_lock_duration(deps, env, info, &constants, lock_ids, lock_duration),
        ExecuteMsg::SplitLock { lock_id, amount } => {
            split_lock(deps, env, info, &constants, lock_id, amount)
        }
        ExecuteMsg::MergeLocks { lock_ids } => merge_locks(deps, env, info, &constants, lock_ids),
        ExecuteMsg::UnlockTokens { lock_ids } => unlock_tokens(deps, env, info, lock_ids),
        ExecuteMsg::CreateProposal {
            round_id,
            tranche_id,
            title,
            description,
            deployment_duration,
            minimum_atom_liquidity_request,
        } => create_proposal(
            deps,
            env,
            info,
            &constants,
            round_id,
            tranche_id,
            title,
            description,
            deployment_duration,
            minimum_atom_liquidity_request,
        ),
        ExecuteMsg::Vote {
            tranche_id,
            proposals_votes,
        } => vote(deps, env, info, &constants, tranche_id, proposals_votes),
        ExecuteMsg::Unvote {
            tranche_id,
            lock_ids,
        } => unvote(deps, env, info, &constants, tranche_id, lock_ids),
        ExecuteMsg::AddAccountToWhitelist { address } => add_to_whitelist(deps, env, info, address),
        ExecuteMsg::RemoveAccountFromWhitelist { address } => {
            remove_from_whitelist(deps, env, info, address)
        }
        ExecuteMsg::UpdateConfig {
            activate_at,
            max_locked_tokens,
            known_users_cap,
            max_deployment_duration,
            cw721_collection_info,
            lock_depth_limit,
            lock_expiry_duration_seconds,
        } => update_config(
            deps,
            env,
            info,
            activate_at,
            max_locked_tokens,
            known_users_cap,
            max_deployment_duration,
            cw721_collection_info,
            lock_depth_limit,
            lock_expiry_duration_seconds,
        ),
        ExecuteMsg::DeleteConfigs { timestamps } => delete_configs(deps, &env, info, timestamps),
        ExecuteMsg::Pause {} => pause_contract(deps, &env, info),
        ExecuteMsg::AddTranche { tranche } => add_tranche(deps, env, info, tranche),
        ExecuteMsg::EditTranche {
            tranche_id,
            tranche_name,
            tranche_metadata,
        } => edit_tranche(deps, env, info, tranche_id, tranche_name, tranche_metadata),
        ExecuteMsg::CreateICQsForValidators { validators } => {
            create_icqs_for_validators(deps, env, info, &constants, validators)
        }
        ExecuteMsg::AddICQManager { address } => add_icq_manager(deps, env, info, address),
        ExecuteMsg::RemoveICQManager { address } => remove_icq_manager(deps, env, info, address),
        ExecuteMsg::WithdrawICQFunds { amount } => withdraw_icq_funds(deps, env, info, amount),
        ExecuteMsg::AddLiquidityDeployment {
            round_id,
            tranche_id,
            proposal_id,
            destinations,
            deployed_funds,
            funds_before_deployment,
            total_rounds,
            remaining_rounds,
        } => {
            let deployment = LiquidityDeployment {
                round_id,
                tranche_id,
                proposal_id,
                destinations,
                deployed_funds,
                funds_before_deployment,
                total_rounds,
                remaining_rounds,
            };
            add_liquidity_deployment(deps, env, info, &constants, deployment)
        }
        ExecuteMsg::RemoveLiquidityDeployment {
            round_id,
            tranche_id,
            proposal_id,
        } => remove_liquidity_deployment(deps, env, info, round_id, tranche_id, proposal_id),
        ExecuteMsg::UpdateTokenGroupRatio {
            token_group_id,
            old_ratio,
            new_ratio,
        } => update_token_group_ratio(
            deps,
            env,
            info,
            &constants,
            token_group_id,
            old_ratio,
            new_ratio,
        ),
        ExecuteMsg::AddTokenInfoProvider {
            token_info_provider,
        } => add_token_info_provider(deps, env, info, &constants, token_info_provider),
        ExecuteMsg::RemoveTokenInfoProvider { provider_id } => {
            remove_token_info_provider(deps, env, info, &constants, provider_id)
        }
        ExecuteMsg::SetGatekeeper { gatekeeper_addr } => {
            set_gatekeeper(deps, env, info, gatekeeper_addr)
        }
        ExecuteMsg::TransferNft {
            recipient,
            token_id,
        } => cw721::handle_execute_transfer(deps, env, info, recipient, token_id),
        ExecuteMsg::SendNft {
            contract,
            token_id,
            msg,
        } => cw721::handle_execute_send_nft(deps, env, info, contract, token_id, msg),
        ExecuteMsg::Approve {
            spender,
            expires,
            token_id,
        } => cw721::handle_execute_approve(deps, env, info, spender, expires, token_id),
        ExecuteMsg::Revoke { spender, token_id } => {
            cw721::handle_execute_revoke(deps, env, info, spender, token_id)
        }
        ExecuteMsg::ApproveAll { operator, expires } => {
            cw721::handle_execute_approve_all(deps, env, info, operator, expires)
        }
        ExecuteMsg::RevokeAll { operator } => {
            cw721::handle_execute_revoke_all(deps, env, info, operator)
        }
        ExecuteMsg::SetDropTokenInfo {
            core_address,
            d_token_denom,
            puppeteer_address,
        } => set_drop_token_info(
            deps,
            env,
            info,
            core_address,
            d_token_denom,
            puppeteer_address,
        ),
        ExecuteMsg::ConvertLockupToDtoken { lock_ids } => {
            convert_lockup_to_dtoken(deps, env, info, lock_ids)
        }
    }
}

// SetGatekeeper(gatekeeper_addr):
// Validate that the sender is a whitelist admin
// Changes the address of the Gatekeeper contract to the provided one
// If the provided address is None, the reference to the Gatekeeper contract is removed from this contract
fn set_gatekeeper(
    deps: DepsMut<'_, NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    gatekeeper_addr: Option<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let whitelist_admins = WHITELIST_ADMINS.load(deps.storage)?;

    if !whitelist_admins.contains(&info.sender) {
        return Err(ContractError::Unauthorized);
    }

    match &gatekeeper_addr {
        Some(addr) => {
            if addr.is_empty() {
                return Err(ContractError::Std(StdError::generic_err(
                    "Gatekeeper address cannot be empty",
                )));
            }
            GATEKEEPER.save(deps.storage, addr)?;
        }
        None => {
            GATEKEEPER.remove(deps.storage);
        }
    }

    Ok(Response::new()
        .add_attribute("action", "set_gatekeeper")
        .add_attribute("sender", info.sender)
        .add_attribute(
            "gatekeeper_addr",
            gatekeeper_addr.unwrap_or("None".to_string()),
        ))
}

// LockTokens(lock_duration):
//     Receive tokens
//     Validate against the accepted denom
//     Update voting power on proposals if user already voted for any
//     Update total round power
//     Create entry in LocksMap
fn lock_tokens(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    constants: &Constants,
    lock_duration: u64,
    proof: Option<LockTokensProof>,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_lock_duration(
        &constants.round_lock_power_schedule,
        constants.lock_epoch_length,
        lock_duration,
    )?;

    let current_round = compute_current_round_id(&env, constants)?;

    if info.funds.len() != 1 {
        return Err(ContractError::Std(StdError::generic_err(
            "Must provide exactly one coin to lock",
        )));
    }

    let funds = info.funds[0].clone();

    let mut token_manager = TokenManager::new(&deps.as_ref());
    let token_group_id = token_manager
        .validate_denom(&deps.as_ref(), current_round, funds.denom)
        .map_err(|err| new_generic_error(format!("validating denom: {err}")))?;

    let total_locked_tokens = LOCKED_TOKENS.load(deps.storage)?;
    let amount_to_lock = info.funds[0].amount.u128();
    let locking_info = validate_locked_tokens_caps(
        &deps,
        constants,
        current_round,
        &info.sender,
        total_locked_tokens,
        amount_to_lock,
    )?;

    // validate that the user does not have too many locks
    if get_lock_count(&deps.as_ref(), info.sender.clone()) >= MAX_LOCK_ENTRIES {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "User has too many locks, only {MAX_LOCK_ENTRIES} locks allowed"
        ))));
    }

    let lock_id = get_next_lock_id(deps.storage)?;
    let lock_entry = LockEntryV2 {
        lock_id,
        owner: info.sender.clone(),
        funds: info.funds[0].clone(),
        lock_start: env.block.time,
        lock_end: env.block.time.plus_nanos(lock_duration),
    };
    let lock_end = lock_entry.lock_end.nanos();
    LOCKS_MAP_V2.save(deps.storage, lock_id, &lock_entry, env.block.height)?;

    USER_LOCKS.update(
        deps.storage,
        info.sender.clone(),
        env.block.height,
        |current_locks| -> Result<Vec<u64>, StdError> {
            match current_locks {
                None => Ok(vec![lock_id]),
                Some(mut current_locks) => {
                    current_locks.push(lock_id);
                    Ok(current_locks)
                }
            }
        },
    )?;

    // Update USER_LOCKS_FOR_CLAIM to include this new lock for tribute claiming
    USER_LOCKS_FOR_CLAIM.update(
        deps.storage,
        info.sender.clone(),
        |current_claim_locks| -> Result<Vec<u64>, StdError> {
            match current_claim_locks {
                None => Ok(vec![lock_id]),
                Some(mut current_claim_locks) => {
                    current_claim_locks.push(lock_id);
                    Ok(current_claim_locks)
                }
            }
        },
    )?;

    update_locked_tokens_info(
        &mut deps,
        current_round,
        &info.sender,
        total_locked_tokens,
        &locking_info,
    )?;

    // Prepare a message that will be sent to the Gatekeeper to validate if the user has
    // the right to lock the specifed number of tokens, per currently active criteria.
    // The ReplyOn is set to Never, so if this message fails then the changes we made
    // during this lock_tokens processing will be reverted as well. We also don't need to
    // wait for the result of execution, since the Gatekeeper will accept this SubMsg only
    // if user is eligible to lock the entire amount provided, and provides valid proofs.
    let mut submsgs = vec![];
    if let Some(gatekeeper_msg) =
        build_gatekeeper_lock_tokens_msg(&deps, &info.sender, &locking_info, &proof)?
    {
        submsgs.push(gatekeeper_msg);
    }

    // If user already voted for some proposals in the current round, update the voting power on those proposals.
    update_voting_power_on_proposals(
        &mut deps,
        constants,
        &mut token_manager,
        current_round,
        None,
        lock_entry.clone(),
        token_group_id.clone(),
    )?;

    // Calculate and update the total voting power info for current and all
    // future rounds in which the user will have voting power greater than 0
    let last_round_with_power = compute_round_id_for_timestamp(constants, lock_end)? - 1;

    update_total_time_weighted_shares(
        &mut deps,
        env.block.height,
        constants,
        &mut token_manager,
        current_round,
        current_round,
        last_round_with_power,
        lock_end,
        token_group_id,
        lock_entry.funds.amount,
        |_, _, _| Uint128::zero(),
    )?;

    Ok(Response::new()
        .add_submessages(submsgs)
        .add_attribute("action", "lock_tokens")
        .add_attribute("sender", info.sender)
        .add_attribute("lock_id", lock_entry.lock_id.to_string())
        .add_attribute("locked_tokens", info.funds[0].clone().to_string())
        .add_attribute("lock_start", lock_entry.lock_start.to_string())
        .add_attribute("lock_end", lock_entry.lock_end.to_string()))
}

// Extends the lock duration of the guiven lock entries to be current_block_time + lock_duration,
// assuming that this would actually increase the lock_end_time (so this *should not* be a way to make the lock time shorter).
// Thus, for each lock entry the lock_end_time afterwards *must* be later than the lock_end_time before.
// If this doesn't hold for any of the input lock entries, the function will return an error, and no
// lock entries will be updated.
// This should essentially have the same effect as removing the old locks and immediately re-locking all
// the same funds for the new lock duration.
fn refresh_lock_duration(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    constants: &Constants,
    lock_ids: Vec<u64>,
    lock_duration: u64,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_lock_duration(
        &constants.round_lock_power_schedule,
        constants.lock_epoch_length,
        lock_duration,
    )?;

    // if there are no lock_ids, return an error
    if lock_ids.is_empty() {
        return Err(ContractError::Std(StdError::generic_err(
            "No lock_ids provided",
        )));
    }

    let current_round_id = compute_current_round_id(&env, constants)?;

    let mut response = Response::new()
        .add_attribute("action", "refresh_lock_duration")
        .add_attribute("sender", info.clone().sender)
        .add_attribute("lock_count", lock_ids.len().to_string());

    let mut token_manager = TokenManager::new(&deps.as_ref());
    for lock_id in lock_ids {
        let (new_lock_end, old_lock_end) = refresh_single_lock(
            &mut deps,
            &info,
            &env,
            constants,
            &mut token_manager,
            current_round_id,
            lock_id,
            lock_duration,
        )?;

        response = response.add_attribute(
            format!("lock_id_{lock_id}_old_end"),
            old_lock_end.to_string(),
        );
        response = response.add_attribute(
            format!("lock_id_{lock_id}_new_end"),
            new_lock_end.to_string(),
        );
    }

    Ok(response)
}

#[allow(clippy::too_many_arguments)]
fn refresh_single_lock(
    deps: &mut DepsMut<NeutronQuery>,
    info: &MessageInfo,
    env: &Env,
    constants: &Constants,
    token_manager: &mut TokenManager,
    current_round_id: u64,
    lock_id: u64,
    new_lock_duration: u64,
) -> Result<(u64, u64), ContractError> {
    let mut lock_entry = get_owned_lock_entry(deps.storage, &info.sender, lock_id)?;

    let old_lock_entry = lock_entry.clone();

    let new_lock_end = env.block.time.plus_nanos(new_lock_duration).nanos();
    let old_lock_end = lock_entry.lock_end.nanos();
    if new_lock_end <= old_lock_end {
        return Err(ContractError::Std(StdError::generic_err(
            "Shortening locks is not allowed, new lock end time must be after the old lock end",
        )));
    }
    lock_entry.lock_end = Timestamp::from_nanos(new_lock_end);
    LOCKS_MAP_V2.save(deps.storage, lock_id, &lock_entry, env.block.height)?;
    let validate_denom_result = token_manager.validate_denom(
        &deps.as_ref(),
        current_round_id,
        lock_entry.funds.denom.clone(),
    );

    let token_group_id = match validate_denom_result {
        Ok(token_group_id) => token_group_id,
        Err(err) => return Err(new_generic_error(format!("validating denom: {err}"))),
    };

    update_voting_power_on_proposals(
        deps,
        constants,
        token_manager,
        current_round_id,
        Some(old_lock_entry),
        lock_entry.clone(),
        token_group_id.clone(),
    )?;
    let old_last_round_with_power = compute_round_id_for_timestamp(constants, old_lock_end)? - 1;
    let new_last_round_with_power = compute_round_id_for_timestamp(constants, new_lock_end)? - 1;
    update_total_time_weighted_shares(
        deps,
        env.block.height,
        constants,
        token_manager,
        current_round_id,
        current_round_id,
        new_last_round_with_power,
        new_lock_end,
        token_group_id,
        lock_entry.funds.amount,
        |round, round_end, locked_amount| {
            if round > old_last_round_with_power {
                return Uint128::zero();
            }

            let old_lockup_length = old_lock_end - round_end.nanos();
            scale_lockup_power(
                &constants.round_lock_power_schedule,
                constants.lock_epoch_length,
                old_lockup_length,
                locked_amount,
            )
        },
    )?;
    Ok((new_lock_end, old_lock_end))
}

// SplitLock(lock_id_to_split, amount):
//     Check that the lock_id_to_split is owned by the sender
//     Check the amount to split out (e.g. existing lock amount, new lock amount, remaining lock amount)
//     Check if user has reached the maximum number of locks it can own
//     Create two new lock entres, one with the specified amount and another one with the remaining amount
//     Check if the splitted lock voted in previous rounds and insert 0-power votes for the same proposals for the new lock entries
//     If the splitted lock was used for voting in current round, remove the existing vote and add two new votes
fn split_lock(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    constants: &Constants,
    lock_id_to_split: u64,
    amount: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    let starting_lock_entry = get_owned_lock_entry(deps.storage, &info.sender, lock_id_to_split)?;

    let resulting_lock_amount = starting_lock_entry
        .funds
        .amount
        .checked_sub(amount)
        .map_err(|_| {
            new_generic_error(format!(
                "Lock id: {} with size {} cannot be used to split out into new lock with size {}.",
                amount, starting_lock_entry.funds.amount, amount,
            ))
        })?;

    if resulting_lock_amount < MIN_SPLIT_LOCK_SIZE || amount < MIN_SPLIT_LOCK_SIZE {
        return Err(new_generic_error(format!(
            "Cannot split lock with amount: {}. Both resulting lockups must be at least of size: {}.",
            starting_lock_entry.funds.amount, MIN_SPLIT_LOCK_SIZE,
        )));
    }

    if get_lock_count(&deps.as_ref(), info.sender.clone()) >= MAX_LOCK_ENTRIES {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "Cannot split lock. User has too many locks, only {MAX_LOCK_ENTRIES} locks allowed"
        ))));
    }

    // All other fields except lock_id and amount should remain the same as with original lock entry
    let new_lock_id_1 = get_next_lock_id(deps.storage)?;
    let new_lock_entry_1 = LockEntryV2 {
        lock_id: new_lock_id_1,
        funds: Coin {
            denom: starting_lock_entry.funds.denom.clone(),
            amount: resulting_lock_amount,
        },
        ..starting_lock_entry.clone()
    };

    let new_lock_id_2 = get_next_lock_id(deps.storage)?;
    let new_lock_entry_2 = LockEntryV2 {
        lock_id: new_lock_id_2,
        owner: info.sender.clone(),
        funds: Coin {
            denom: starting_lock_entry.funds.denom.clone(),
            amount,
        },
        lock_start: env.block.time,
        lock_end: starting_lock_entry.lock_end,
    };

    let depth = get_lock_ancestor_depth(
        &deps.as_ref(),
        env.clone(),
        starting_lock_entry.lock_id,
        constants.lock_expiry_duration_seconds,
    )?;
    if depth >= constants.lock_depth_limit {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "Cannot split lock. Depth limit {} for lock id {} reached",
            constants.lock_depth_limit, starting_lock_entry.lock_id
        ))));
    }

    // Forward lock tracking
    let total = resulting_lock_amount + amount;
    let frac_1 = Decimal::from_ratio(resulting_lock_amount, total);
    let frac_2 = Decimal::one() - frac_1;
    LOCK_ID_TRACKING.save(
        deps.storage,
        starting_lock_entry.lock_id,
        &vec![(new_lock_id_1, frac_1), (new_lock_id_2, frac_2)],
    )?;

    // Reverse lock tracking
    REVERSE_LOCK_ID_TRACKING.save(
        deps.storage,
        new_lock_id_1,
        &vec![starting_lock_entry.lock_id],
    )?;
    REVERSE_LOCK_ID_TRACKING.save(
        deps.storage,
        new_lock_id_2,
        &vec![starting_lock_entry.lock_id],
    )?;

    // Expiry
    LOCK_ID_EXPIRY.save(deps.storage, starting_lock_entry.lock_id, &env.block.time)?;

    // Remove starting lock entry
    LOCKS_MAP_V2.remove(deps.storage, starting_lock_entry.lock_id, env.block.height)?;

    // Insert new lock entries
    for lock_entry in [&new_lock_entry_1, &new_lock_entry_2] {
        LOCKS_MAP_V2.save(
            deps.storage,
            lock_entry.lock_id,
            lock_entry,
            env.block.height,
        )?;
    }

    // Update information about locks owned by the user
    USER_LOCKS.update(
        deps.storage,
        info.sender.clone(),
        env.block.height,
        |current_locks| -> Result<Vec<u64>, StdError> {
            let mut current_locks = current_locks.expect("User locks must exist");

            current_locks.extend_from_slice(&[new_lock_entry_1.lock_id, new_lock_entry_2.lock_id]);
            current_locks.retain(|&lock_id| lock_id != lock_id_to_split);

            Ok(current_locks)
        },
    )?;

    // Update information about locks for which the user is eligible to claim the tributes.
    // Keep the lock_id_to_split in the list so that the user can claim past round tributes.
    USER_LOCKS_FOR_CLAIM.update(
        deps.storage,
        info.sender.clone(),
        |current_locks| -> Result<Vec<u64>, StdError> {
            let mut current_locks = current_locks.expect("User locks must exist");

            current_locks.extend_from_slice(&[new_lock_entry_1.lock_id, new_lock_entry_2.lock_id]);

            Ok(current_locks)
        },
    )?;

    let current_round_id = compute_current_round_id(&env, constants)?;

    // Use both lock entries for revoting, if there was an existing vote
    let lock_entries = HashMap::from_iter([
        (new_lock_entry_1.lock_id, new_lock_entry_1.clone()),
        (new_lock_entry_2.lock_id, new_lock_entry_2.clone()),
    ]);

    // Prepare target_votes for process_unvotes() function to remove the existing lock vote, if there is any.
    let target_votes = HashMap::from_iter([(lock_id_to_split, None)]);

    let mut token_manager = TokenManager::new(&deps.as_ref());

    let tranche_ids = TRANCHE_MAP
        .keys(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<u64>>>()?;

    for tranche_id in tranche_ids {
        // Go back through previous rounds, find if the splitted lock was used for voting,
        // and insert 0-power votes for the new lock entries. This is needed since we will
        // (in some cases) populate VOTING_ALLOWED_ROUND for the new lock entries, and the
        // query_all/specific_user_lockups_with_tranche_infos() is relaying on vote existance
        // in that case.
        for round_id in 0..current_round_id {
            if let Some(vote) = get_lock_vote(deps.storage, round_id, tranche_id, lock_id_to_split)?
            {
                let new_vote = Vote {
                    prop_id: vote.prop_id,
                    time_weighted_shares: (vote.time_weighted_shares.0, Decimal::zero()),
                };

                for lock_id in [new_lock_entry_1.lock_id, new_lock_entry_2.lock_id] {
                    VOTE_MAP_V2.save(deps.storage, ((round_id, tranche_id), lock_id), &new_vote)?;
                }
            }
        }

        let unvotes_result =
            process_unvotes(deps.storage, current_round_id, tranche_id, &target_votes)?;

        // If no vote was removed, there is no need to re-add it
        let removed_votes = unvotes_result
            .removed_votes
            .iter()
            .take(1)
            .collect::<Vec<_>>();
        let old_vote = match removed_votes.len() {
            1 => removed_votes[0],
            _ => {
                // New locks inherit the voting allowed round from the original lock.
                // If there was no vote in this round that got removed, then information
                // comes from the previous rounds vote, if there were any.
                // If the original lock was used for voting in the current round, then process_votes()
                // will update the VOTING_ALLOWED_ROUND for new locks as well.
                if let Some(voting_allowed_round) =
                    VOTING_ALLOWED_ROUND.may_load(deps.storage, (tranche_id, lock_id_to_split))?
                {
                    for lock_id in [new_lock_entry_1.lock_id, new_lock_entry_2.lock_id] {
                        VOTING_ALLOWED_ROUND.save(
                            deps.storage,
                            (tranche_id, lock_id),
                            &voting_allowed_round,
                        )?;
                    }
                }

                continue;
            }
        };

        let votes = ProposalToLockups {
            proposal_id: old_vote.1.prop_id,
            lock_ids: vec![new_lock_entry_1.lock_id, new_lock_entry_2.lock_id],
        };

        process_votes_and_apply_proposal_changes(
            &mut deps,
            &env,
            &mut token_manager,
            constants,
            current_round_id,
            tranche_id,
            &[votes],
            &lock_entries,
            unvotes_result,
        )?;
    }

    Ok(Response::new()
        .add_attribute("action", "split_lock")
        .add_attribute("sender", info.sender)
        .add_attribute("lock_id_to_split", lock_id_to_split.to_string())
        .add_attribute("new_lock_1_id", new_lock_entry_1.lock_id.to_string())
        .add_attribute(
            "new_lock_1_amount",
            new_lock_entry_1.funds.amount.to_string(),
        )
        .add_attribute("new_lock_2_id", new_lock_entry_2.lock_id.to_string())
        .add_attribute(
            "new_lock_2_amount",
            new_lock_entry_2.funds.amount.to_string(),
        ))
}

// MergeLocks(lock_ids):
//     Validate lock_ids to merge: eliminate duplicates, check ownership, check if there are at least two locks, and all locks hold the same denom
//     Sum up the amounts of all input locks, and save a new lock entry into the store; Remove the input locks from the store
//     Remove any votes in the current round that were cast by the input locks and, if the conditions are met, insert a new vote for the new lock entry
fn merge_locks(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    constants: &Constants,
    lock_ids: Vec<u64>,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Eliminate duplicates from the lock_ids
    let lock_ids: HashSet<u64> = HashSet::from_iter(lock_ids);
    let mut input_locks = vec![];

    // Verify that the user owns all the locks with provided IDs
    for lock_id in &lock_ids {
        input_locks.push(get_owned_lock_entry(deps.storage, &info.sender, *lock_id)?);
    }

    if input_locks.len() < 2 {
        return Err(ContractError::Std(StdError::generic_err(
            "Must specify at least two lock IDs to merge.",
        )));
    }

    let mut lock_denoms: HashSet<String> = HashSet::new();
    let mut resulting_lock_amount = Uint128::zero();

    // Iterate over all input locks and:
    //      1. calulate the resulting lock amount
    //      2. collect all the denoms to ensure they are the same
    //      3. find the latest lock end time
    let new_lock_end = input_locks
        .iter()
        .map(|lock_entry| {
            lock_denoms.insert(lock_entry.funds.denom.clone());
            resulting_lock_amount += lock_entry.funds.amount;

            lock_entry.lock_end.nanos()
        })
        .max()
        .expect("there must be at least two locks");

    if lock_denoms.len() != 1 {
        return Err(ContractError::Std(StdError::generic_err(
            "Cannot merge locks with different denoms.",
        )));
    }

    let resulting_lock_id = get_next_lock_id(deps.storage)?;
    let resulting_lock_entry = LockEntryV2 {
        lock_id: resulting_lock_id,
        owner: info.sender.clone(),
        funds: Coin {
            denom: input_locks[0].funds.denom.clone(),
            amount: resulting_lock_amount,
        },
        lock_start: env.block.time,
        lock_end: Timestamp::from_nanos(new_lock_end),
    };

    LOCKS_MAP_V2.save(
        deps.storage,
        resulting_lock_entry.lock_id,
        &resulting_lock_entry,
        env.block.height,
    )?;

    let mut parents = vec![];

    for lock_id in &lock_ids {
        let depth = get_lock_ancestor_depth(
            &deps.as_ref(),
            env.clone(),
            *lock_id,
            constants.lock_expiry_duration_seconds,
        )?;

        if depth >= constants.lock_depth_limit {
            let msg = format!(
                "Cannot merge locks. Depth limit {} for lock id {} reached",
                constants.lock_depth_limit, lock_id
            );
            return Err(ContractError::Std(StdError::generic_err(msg)));
        }
        LOCK_ID_TRACKING.save(
            deps.storage,
            *lock_id,
            &vec![(resulting_lock_id, Decimal::one())],
        )?;

        assert!(
            !parents.contains(lock_id),
            "Parent list unexpectedly contains lock_id {lock_id}"
        );

        parents.push(*lock_id);

        LOCK_ID_EXPIRY.save(deps.storage, *lock_id, &env.block.time)?;
        // Remove merged lock from locks map
        LOCKS_MAP_V2.remove(deps.storage, *lock_id, env.block.height)?;
    }
    REVERSE_LOCK_ID_TRACKING.save(deps.storage, resulting_lock_id, &parents)?;

    // Remove merged locks and add new lock id to USER_LOCKS
    USER_LOCKS.update(
        deps.storage,
        info.sender.clone(),
        env.block.height,
        |current_locks| -> Result<Vec<u64>, StdError> {
            let mut current_locks = current_locks.expect("User locks must exist");

            current_locks.push(resulting_lock_id);
            current_locks.retain(|lock_id| !lock_ids.contains(lock_id));

            Ok(current_locks)
        },
    )?;

    // Add new lock id to USER_LOCKS_FOR_CLAIM (merged ones are retained)
    USER_LOCKS_FOR_CLAIM.update(
        deps.storage,
        info.sender.clone(),
        |current_locks| -> Result<Vec<u64>, StdError> {
            let mut current_locks = current_locks.expect("User locks for claim must exist");
            current_locks.push(resulting_lock_id);

            Ok(current_locks)
        },
    )?;

    let current_round_id = compute_current_round_id(&env, constants)?;

    let unvoting_target_votes = HashMap::from_iter(
        input_locks
            .iter()
            .map(|lock_entry| (lock_entry.lock_id, None)),
    );

    let voting_lock_entries =
        HashMap::from_iter([(resulting_lock_entry.lock_id, resulting_lock_entry.clone())]);

    let tranche_ids = TRANCHE_MAP
        .keys(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<u64>>>()?;

    let mut token_manager = TokenManager::new(&deps.as_ref());

    for tranche_id in tranche_ids {
        // First run process_unvotes() in order to remove votes and voting allowed round
        // information originating from the current round votes.
        let unvotes_result = process_unvotes(
            deps.storage,
            current_round_id,
            tranche_id,
            &unvoting_target_votes,
        )?;

        // After removing the current round votes, check if any of the input lockups shouldn't be allowed to
        // vote in the current round. If so, the resulting lockup will inherit the *highest* voting allowed round
        // and we also insert 0-power vote for the new lock entry in the same round the original lock has voted.
        // Note that there could be multiple such lockups, so we will take one of them.
        if let Some(lock_voting_allowed) =
            get_higest_voting_allowed_round(&deps.as_ref(), tranche_id, &lock_ids)?
        {
            if lock_voting_allowed.voting_allowed_round > current_round_id {
                VOTING_ALLOWED_ROUND.save(
                    deps.storage,
                    (tranche_id, resulting_lock_id),
                    &lock_voting_allowed.voting_allowed_round,
                )?;

                // Find the vote that implies next voting_allowed_round and insert a 0-power vote for the new lock entry
                for round_id in (0..current_round_id).rev() {
                    if let Some(vote) = VOTE_MAP_V2.may_load(
                        deps.storage,
                        ((round_id, tranche_id), lock_voting_allowed.lock_id),
                    )? {
                        VOTE_MAP_V2.save(
                            deps.storage,
                            ((round_id, tranche_id), resulting_lock_id),
                            &Vote {
                                prop_id: vote.prop_id,
                                time_weighted_shares: (
                                    vote.time_weighted_shares.0,
                                    Decimal::zero(),
                                ),
                            },
                        )?;

                        // Break out of the inner loop as soon as we find a vote in round closest to the current one
                        break;
                    }

                    if round_id == 0 {
                        return Err(ContractError::Std(StdError::generic_err(format!(
                        "Could not find vote for lock {} in tranche {} for any of previous rounds",
                        lock_voting_allowed.lock_id, tranche_id
                    ))));
                    }
                }

                // If the resulting lock isn't allowed to vote in the current round
                // and tranche, then move to the next tranche.
                continue;
            }
        }

        // If input lock entries didn't vote in current round, or they voted for multiple proposals,
        // we will not add the vote for the resulting lock entry.
        if unvotes_result.removed_votes.len() != 1 {
            continue;
        }

        let old_vote = unvotes_result
            .removed_votes
            .iter()
            .take(1)
            .collect::<Vec<_>>()[0] // There must be exactly one removed vote
            .1;

        let votes = ProposalToLockups {
            proposal_id: old_vote.prop_id,
            lock_ids: vec![resulting_lock_entry.lock_id],
        };

        process_votes_and_apply_proposal_changes(
            &mut deps,
            &env,
            &mut token_manager,
            constants,
            current_round_id,
            tranche_id,
            &[votes],
            &voting_lock_entries,
            unvotes_result,
        )?;
    }

    let lock_ids_attr = lock_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<String>>()
        .join(",");

    Ok(Response::new()
        .add_attribute("action", "merge_locks")
        .add_attribute("sender", info.sender)
        .add_attribute("lock_ids", lock_ids_attr)
        .add_attribute("resulting_lock_id", resulting_lock_id.to_string()))
}

/// Recursively resolves the final (leaf-level) composition of a given `lock_id`.
///
/// This function traces the ancestry tree from the given lock down to its leaves,
/// applying all split and merge fractions, and produces a weighted composition
/// of how much each final leaf lock contributes to the given lock.
///
/// A "leaf" lock is one that has no further children in `LOCK_ID_TRACKING`.
///
/// ## How it works:
/// - If the given `lock_id` is not found in `LOCK_ID_TRACKING`, it is treated as a leaf
///   and assigned 100% (`Decimal::one()`) weight.
/// - Otherwise, the function recursively traverses the child locks and accumulates
///   their weighted fractions, multiplying fractions at each level.
/// - The final result is a vector of `(leaf_lock_id, fraction)` pairs that sum to 1.0.
///
/// ## Example
/// Consider the following actions and resulting tracking:
/// ```text
/// LOCK_ID(s)           ACTION          RESULT
/// 1                    split 50-50     lock_id_tracking[1] = [(2, 0.5), (3, 0.5)]
/// 2                    split 70-30     lock_id_tracking[2] = [(4, 0.7), (5, 0.3)]
/// 3, 4                 merge           lock_id_tracking[3] = [(6, 1.0)]
///                                      lock_id_tracking[4] = [(6, 1.0)]
///
/// Here, lock 1 was used for voting before being split.
///
/// Then calling:
///
/// get_current_lock_composition(..., 1)
///
/// returns:
///
/// [(5, 0.15), (6, 0.85)]
///
/// Explanation:
/// - Lock 1 splits into 2 (0.5) and 3 (0.5)
/// - Lock 2 splits into 4 (0.7) and 5 (0.3)
/// - Locks 3 and 4 merge into 6 (1.0 each)
/// - So final composition for 1 is:
///   - Lock 5: 0.5 * 0.3 = 0.15
///   - Lock 6: 0.5 * (from lock 3) 1.0 + 0.5 * (from lock 4) 0.7 = 0.5 + 0.35 = 0.85
///
pub fn get_current_lock_composition(
    deps: &Deps<NeutronQuery>,
    lock_id: u64,
) -> StdResult<Vec<(u64, Decimal)>> {
    fn resolve(deps: &Deps<NeutronQuery>, lock_id: u64) -> StdResult<HashMap<u64, Decimal>> {
        let maybe_children = LOCK_ID_TRACKING.may_load(deps.storage, lock_id)?;

        // If not in tracking, it's a leaf
        if maybe_children.is_none() {
            let mut map = HashMap::new();
            map.insert(lock_id, Decimal::one());
            return Ok(map);
        }

        let mut result = HashMap::new();
        for (child_id, fraction) in maybe_children.unwrap() {
            let child_composition = resolve(deps, child_id)?;
            for (leaf_id, weight) in child_composition {
                let entry = result.entry(leaf_id).or_insert(Decimal::zero());
                *entry += weight * fraction;
            }
        }

        Ok(result)
    }

    let map = resolve(deps, lock_id)?;
    let mut out = map.into_iter().collect::<Vec<_>>();
    out.sort_by_key(|(id, _)| *id);
    Ok(out)
}

/// Calculates the depth of non-expired ancestor locks for a given `lock_id`.
///
/// Depth is defined as the length of the longest chain of parent locks, starting from `lock_id`
/// and following its ancestors recursively, **excluding any locks that have expired**.
///
/// - The depth includes the input `lock_id` itself (depth 1 for a valid leaf).
/// - If the input lock is expired, the function returns `0`.
/// - If an ancestor lock is expired, the recursion **stops along that path**, but may continue along others.
/// - Expired parents are **skipped**, not counted, and not recursed into.
/// - Cycle prevention is enforced using a `visited` list (cloned per path).
///
/// Example cases:
/// - Root node with no parents: returns `1`
/// - Lock with 3 non-expired ancestors: returns `4`
/// - Lock with expired ancestors: returns the depth up to the first expired ancestor
///  
/// Given the following reverse tracking and all locks unexpired:
///
/// REVERSE_LOCK_ID_TRACKING:
/// Lock ID 1 => [0]
/// Lock ID 2 => [0]
/// Lock ID 3 => [1]
/// Lock ID 4 => [1]
/// Lock ID 5 => [2, 3]
///
/// Then:
///
/// get_lock_ancestor_depth(..., 5) == Ok(4)
///
/// Explanation: Longest valid path is: 5 → 3 → 1 → 0 (4 hops).
///
/// If lock `0` is expired:
///
/// get_lock_ancestor_depth(..., 5) == Ok(3)  // path 5 → 3 → 1 still valid
///
///
/// If lock `1` and `2` are expired:
///
/// get_lock_ancestor_depth(..., 5) == Ok(2)  // path 5 → 3
///
///
/// If lock `3` is expired:
///
/// get_lock_ancestor_depth(..., 5) == Ok(1)  // only 5 itself counts
///
///
/// If lock `5` is expired:
///
/// get_lock_ancestor_depth(..., 5) == Ok(0)  // root is expired
///
pub fn get_lock_ancestor_depth(
    deps: &Deps<NeutronQuery>,
    env: Env,
    lock_id: u64,
    lock_expiry_duration_seconds: u64,
) -> StdResult<u64> {
    if let Some(expiry_time) = LOCK_ID_EXPIRY.may_load(deps.storage, lock_id)? {
        let cutoff = expiry_time.plus_seconds(lock_expiry_duration_seconds);
        if env.block.time > cutoff {
            return Ok(0); // lock itself is expired
        }
    }

    fn recurse(
        deps: &Deps<NeutronQuery>,
        env: Env,
        current_id: u64,
        lock_expiry_duration_seconds: u64,
        visited: &mut [u64],
    ) -> StdResult<u64> {
        let parents = REVERSE_LOCK_ID_TRACKING
            .may_load(deps.storage, current_id)?
            .unwrap_or_default();

        if parents.is_empty() {
            return Ok(1);
        }

        let mut max_depth = 0;
        for parent_id in parents {
            if let Some(expiry_time) = LOCK_ID_EXPIRY.may_load(deps.storage, parent_id)? {
                let expiry_cutoff = expiry_time.plus_seconds(lock_expiry_duration_seconds);
                if env.block.time > expiry_cutoff {
                    continue;
                }
            }

            let mut local_visited = visited.to_owned();
            if local_visited.contains(&parent_id) {
                continue;
            }
            local_visited.push(parent_id);

            let depth = recurse(
                deps,
                env.clone(),
                parent_id,
                lock_expiry_duration_seconds,
                &mut local_visited,
            )?;
            if depth > max_depth {
                max_depth = depth;
            }
        }

        Ok(max_depth + 1)
    }

    recurse(deps, env, lock_id, lock_expiry_duration_seconds, &mut [])
}

// Validate that the lock duration (given in nanos) is either 1, 2, 3, 6, or 12 epochs
fn validate_lock_duration(
    round_lock_power_schedule: &RoundLockPowerSchedule,
    lock_epoch_length: u64,
    lock_duration: u64,
) -> Result<(), ContractError> {
    let lock_times = round_lock_power_schedule
        .round_lock_power_schedule
        .iter()
        .map(|entry| entry.locked_rounds * lock_epoch_length)
        .collect::<Vec<u64>>();

    if !lock_times.contains(&lock_duration) {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "Lock duration must be one of: {lock_times:?}; but was: {lock_duration}"
        ))));
    }

    Ok(())
}

// UnlockTokens():
//     Validate that the caller didn't vote in previous round
//     Validate caller
//     Validate `lock_end` < now
//     Send `amount` tokens back to caller
//     Delete entry from LocksMap
fn unlock_tokens(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    lock_ids: Option<Vec<u64>>,
) -> Result<Response<NeutronMsg>, ContractError> {
    // TODO: reenable this when we implement slashing
    // validate_previous_round_vote(&deps, &env, info.sender.clone())?;

    let locks: Vec<_> = USER_LOCKS
        .may_load(deps.storage, info.sender.clone())?
        .into_iter()
        .flatten()
        .filter_map(|id| {
            if lock_ids.as_ref().is_some_and(|ids| !ids.contains(&id)) {
                return None;
            }

            LOCKS_MAP_V2
                .load(deps.storage, id)
                .map(|lock| Some((id, lock)))
                .transpose()
        })
        .collect::<Result<_, _>>()?;

    let mut total_unlocked_amount = Uint128::zero();

    let mut response = Response::new()
        .add_attribute("action", "unlock_tokens")
        .add_attribute("sender", info.sender.to_string());

    let mut removed_lock_ids = HashSet::new();
    let mut unlocked_tokens = vec![];

    for (lock_id, lock_entry) in locks {
        if lock_entry.lock_end < env.block.time {
            // Send tokens back to caller
            let send = Coin {
                denom: lock_entry.funds.denom,
                amount: lock_entry.funds.amount,
            };

            response = response.add_message(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: vec![send.clone()],
            });

            total_unlocked_amount += send.amount;

            // Delete unlocked lock
            LOCKS_MAP_V2.remove(deps.storage, lock_id, env.block.height)?;

            // Clear any CW721 Approval on the lock
            cw721::clear_nft_approvals(deps.storage, lock_id)?;

            removed_lock_ids.insert(lock_id);
            unlocked_tokens.push(send.to_string());
        }
    }

    USER_LOCKS.update(
        deps.storage,
        info.sender.clone(),
        env.block.height,
        |current_locks| -> Result<Vec<u64>, StdError> {
            match current_locks {
                None => Ok(vec![]),
                Some(mut current_locks) => {
                    current_locks.retain(|lock_id| !removed_lock_ids.contains(lock_id));
                    Ok(current_locks)
                }
            }
        },
    )?;

    if !total_unlocked_amount.is_zero() {
        LOCKED_TOKENS.update(
            deps.storage,
            |locked_tokens| -> Result<u128, ContractError> {
                Ok(locked_tokens - total_unlocked_amount.u128())
            },
        )?;
    }

    // Convert removed_lock_ids to strings for the response attributes
    let unlocked_lock_ids = removed_lock_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<String>>();

    Ok(response
        .add_attribute("unlocked_lock_ids", unlocked_lock_ids.join(", "))
        .add_attribute("unlocked_tokens", unlocked_tokens.join(", ")))
}

// prevent clippy from warning for unused function
// TODO: reenable this when we enable slashing
// Note: this function is outdated and would need to be fixed when reinstated
// When we want to reinstate the function, the process should probably be:
// 1. Receive list of lock_ids (already confirmed that they belong to the user) to unlock
// 2. For each lock_id, check that the last vote's bid duration does not prevent the unlock to happen at this round
// 3. Return the list of lock_ids that are allowed to be unlocked.
#[allow(dead_code)]
fn validate_previous_round_vote(
    deps: &DepsMut<NeutronQuery>,
    env: &Env,
    _sender: &Addr,
) -> Result<(), ContractError> {
    let constants = load_current_constants(&deps.as_ref(), env)?;
    let current_round_id = compute_current_round_id(env, &constants)?;
    if current_round_id > 0 {
        let previous_round_id = current_round_id - 1;
        for tranche_id in TRANCHE_MAP.keys(deps.storage, None, None, Order::Ascending) {
            if VOTE_MAP_V2
                .prefix((previous_round_id, tranche_id?))
                .range(deps.storage, None, None, Order::Ascending)
                .count()
                > 0
            {
                return Err(ContractError::Std(StdError::generic_err(
                    "Tokens can not be unlocked, user voted for at least one proposal in previous round",
                )));
            }
        }
    }

    Ok(())
}

// Creates a new proposal in the store.
// It will:
// * validate that the contract is not paused
// * validate that the creator of the proposal is on the whitelist
// Then, it will create the proposal in the specified tranche and in the specified round.
// If no round_id is specified, the function will use the current round id.
#[allow(clippy::too_many_arguments)]
fn create_proposal(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    constants: &Constants,
    round_id: Option<u64>,
    tranche_id: u64,
    title: String,
    description: String,
    deployment_duration: u64,
    minimum_atom_liquidity_request: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    let current_round_id = compute_current_round_id(&env, constants)?;

    // if no round_id is provided, use the current round
    let round_id = round_id.unwrap_or(current_round_id);

    if current_round_id > round_id {
        return Err(ContractError::Std(StdError::generic_err(
            "cannot create a proposal in a round that ended in the past",
        )));
    }

    // validate that the sender is on the whitelist
    let whitelist = WHITELIST.load(deps.storage)?;

    if !whitelist.contains(&info.sender) {
        return Err(ContractError::Unauthorized);
    }

    // check that the tranche with the given id exists
    TRANCHE_MAP.load(deps.storage, tranche_id)?;

    // check that the deployment duration is within the allowed range
    if deployment_duration < MIN_DEPLOYMENT_DURATION
        || deployment_duration > constants.max_deployment_duration
    {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "Invalid deployment duration: {}. Must be between {} and {} rounds.",
            deployment_duration, MIN_DEPLOYMENT_DURATION, constants.max_deployment_duration,
        ))));
    }

    let proposal_id = PROP_ID.load(deps.storage)?;

    let proposal = Proposal {
        round_id,
        tranche_id,
        proposal_id,
        power: Uint128::zero(),
        percentage: Uint128::zero(),
        title: title.trim().to_string(),
        description: description.trim().to_string(),
        deployment_duration,
        minimum_atom_liquidity_request,
    };

    PROP_ID.save(deps.storage, &(proposal_id + 1))?;
    PROPOSAL_MAP.save(deps.storage, (round_id, tranche_id, proposal_id), &proposal)?;

    Ok(Response::new()
        .add_attribute("action", "create_proposal")
        .add_attribute("sender", info.sender)
        .add_attribute("round_id", round_id.to_string())
        .add_attribute("tranche_id", tranche_id.to_string())
        .add_attribute("proposal_id", proposal_id.to_string())
        .add_attribute("proposal_title", proposal.title)
        .add_attribute("proposal_description", proposal.description)
        .add_attribute(
            "deployment_duration",
            proposal.deployment_duration.to_string(),
        )
        .add_attribute(
            "minimum_atom_liquidity_request",
            proposal.minimum_atom_liquidity_request.to_string(),
        ))
}

fn vote(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    constants: &Constants,
    tranche_id: u64,
    proposals_votes: Vec<ProposalToLockups>,
) -> Result<Response<NeutronMsg>, ContractError> {
    // This voting system is designed to allow for an unlimited number of proposals and an unlimited number of votes
    // to be created, without being vulnerable to DOS. A naive implementation, where all votes or all proposals were iterated
    // at the end of the round could be DOSed by creating a large number of votes or proposals. This is not a problem
    // for this implementation, but this leads to some subtlety in the implementation.
    // I will explain the overall principle here:
    // - The information on which proposal is winning is updated each time someone votes, instead of being calculated at the end of the round.
    // - This information is stored in a map called PROPS_BY_SCORE, which maps the score of a proposal to the proposal id.
    // - At the end of the round, a single access to PROPS_BY_SCORE is made to get the winning proposal.
    // - To enable switching votes (and for other stuff too), we store the vote in VOTE_MAP.
    // - When a user votes the second time in a round, the information about their previous vote from VOTE_MAP is used to reverse the effect of their previous vote.
    // - This leads to slightly higher gas costs for each vote, in exchange for a much lower gas cost at the end of the round.
    let round_id = compute_current_round_id(&env, constants)?;

    // check that the tranche with the given id exists
    TRANCHE_MAP.load(deps.storage, tranche_id)?;

    // Validate input proposals and locks, and receive:
    // - target votes map with lock_id -> proposal_id,
    // - lock entries map with lock_id -> lock_entry
    let (target_votes, lock_entries) =
        validate_proposals_and_locks_for_voting(deps.storage, &info.sender, &proposals_votes)?;

    // Process unvotes first
    let unvotes_result = process_unvotes(deps.storage, round_id, tranche_id, &target_votes)?;
    let removed_votes = unvotes_result.removed_votes.clone();

    let mut token_manager = TokenManager::new(&deps.as_ref());
    let votes_result = process_votes_and_apply_proposal_changes(
        &mut deps,
        &env,
        &mut token_manager,
        constants,
        round_id,
        tranche_id,
        &proposals_votes,
        &lock_entries,
        unvotes_result,
    )?;

    // Build response
    let mut response = Response::new()
        .add_attribute("action", "vote")
        .add_attribute("sender", info.sender.to_string());

    // Add attributes for old votes that were removed
    for (lock_id, vote) in removed_votes {
        response = response.add_attribute(
            format!("lock_id_{lock_id}_old_proposal_id"),
            vote.prop_id.to_string(),
        );
    }

    let to_string = |input: &Vec<u64>| {
        input
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<String>>()
            .join(",")
    };

    Ok(response
        .add_attribute("proposal_id", to_string(&votes_result.voted_proposals))
        .add_attribute("locks_voted", to_string(&votes_result.locks_voted))
        .add_attribute("locks_skipped", to_string(&votes_result.locks_skipped)))
}

// Function to unvote specific locks
pub fn unvote(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    constants: &Constants,
    tranche_id: u64,
    lock_ids: Vec<u64>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let round_id = compute_current_round_id(&env, constants)?;

    // Check that the tranche exists
    TRANCHE_MAP.load(deps.storage, tranche_id)?;

    let user_lock_ids = USER_LOCKS
        .may_load(deps.storage, info.sender.clone())?
        .ok_or_else(|| StdError::generic_err("User has no locks"))?;

    for &lock_id in &lock_ids {
        if !user_lock_ids.contains(&lock_id) {
            return Err(
                StdError::generic_err("Lock ID not found or does not belong to user").into(),
            );
        }
    }

    // Create target votes map for unvoting - None means we're just unvoting
    let target_votes: HashMap<u64, Option<u64>> = lock_ids
        .into_iter()
        .map(|lock_id| (lock_id, None))
        .collect();

    // Process unvotes
    let unvotes_result = process_unvotes(deps.storage, round_id, tranche_id, &target_votes)?;

    let unique_proposals_to_update: HashSet<u64> =
        unvotes_result.power_changes.keys().copied().collect();

    let mut token_manager = TokenManager::new(&deps.as_ref());
    // Apply proposal power changes from unvotes
    apply_proposal_changes(
        &mut deps,
        &mut token_manager,
        round_id,
        unvotes_result.power_changes,
    )?;

    // Update the proposal in the proposal map, as well as the props by score map, after all changes
    // We can use update_proposal_and_props_by_score_maps as we already applied the proposal power changes
    for proposal_id in unique_proposals_to_update {
        let proposal = get_proposal(deps.storage, round_id, tranche_id, proposal_id)?;
        update_proposal_and_props_by_score_maps(deps.storage, round_id, tranche_id, &proposal)?;
    }

    // Build response
    let mut response = Response::new()
        .add_attribute("action", "unvote")
        .add_attribute("sender", info.sender.to_string());

    // Add attributes for removed votes
    for (lock_id, vote) in unvotes_result.removed_votes {
        response = response.add_attribute(
            format!("lock_id_{lock_id}_old_proposal_id"),
            vote.prop_id.to_string(),
        );
    }

    Ok(response)
}

// Adds a new account address to the whitelist.
fn add_to_whitelist(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_whitelist_admin(&deps, &info)?;

    // Add address to whitelist
    let mut whitelist = WHITELIST.load(deps.storage)?;
    let whitelist_account_addr = deps.api.addr_validate(&address)?;

    // return an error if the account address is already in the whitelist
    if whitelist.contains(&whitelist_account_addr) {
        return Err(ContractError::Std(StdError::generic_err(
            "Address already in whitelist",
        )));
    }

    whitelist.push(whitelist_account_addr.clone());
    WHITELIST.save(deps.storage, &whitelist)?;

    Ok(Response::new()
        .add_attribute("action", "add_to_whitelist")
        .add_attribute("sender", info.sender)
        .add_attribute("added_whitelist_address", whitelist_account_addr))
}

// Removes an account address from the whitelist.
fn remove_from_whitelist(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_whitelist_admin(&deps, &info)?;

    // Remove the account address from the whitelist
    let mut whitelist = WHITELIST.load(deps.storage)?;

    let whitelist_account_addr = deps.api.addr_validate(&address)?;

    whitelist.retain(|cp| cp != whitelist_account_addr);
    WHITELIST.save(deps.storage, &whitelist)?;

    Ok(Response::new()
        .add_attribute("action", "remove_from_whitelist")
        .add_attribute("sender", info.sender)
        .add_attribute("removed_whitelist_address", whitelist_account_addr))
}

#[allow(clippy::too_many_arguments)]
fn update_config(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    activate_at: Timestamp,
    max_locked_tokens: Option<u128>,
    known_users_cap: Option<u128>,
    max_deployment_duration: Option<u64>,
    cw721_collection_info: Option<CollectionInfo>,
    lock_depth_limit: Option<u64>,
    lock_expiry_duration_seconds: Option<u64>,
) -> Result<Response<NeutronMsg>, ContractError> {
    if env.block.time > activate_at {
        return Err(ContractError::Std(StdError::generic_err(
            "Can not update config in the past.",
        )));
    }

    // Load the Constants active at the given timestamp and base the updates on them.
    // This allows us to update the Constants in arbitrary order. E.g. at the similar block
    // height we can schedule multiple updates for the future, where each new Constants will
    // have the changes introduced by earlier ones.
    let mut constants = load_constants_active_at_timestamp(&deps.as_ref(), activate_at)?.1;

    validate_sender_is_whitelist_admin(&deps, &info)?;

    let mut response = Response::new()
        .add_attribute("action", "update_config")
        .add_attribute("sender", info.sender);

    if let Some(max_locked_tokens) = max_locked_tokens {
        constants.max_locked_tokens = max_locked_tokens;
        response = response.add_attribute("new_max_locked_tokens", max_locked_tokens.to_string());
    }

    if let Some(known_users_cap) = known_users_cap {
        constants.known_users_cap = known_users_cap;
        response = response.add_attribute("new_known_users_cap", known_users_cap.to_string());
    }

    if let Some(max_deployment_duration) = max_deployment_duration {
        constants.max_deployment_duration = max_deployment_duration;
        response = response.add_attribute(
            "new_max_deployment_duration",
            max_deployment_duration.to_string(),
        );
    }

    if let Some(cw721_collection_info) = cw721_collection_info {
        constants.cw721_collection_info = cw721_collection_info.clone();
        response = response
            .add_attribute(
                "new_cw721_collection_info_name",
                &cw721_collection_info.name,
            )
            .add_attribute(
                "new_cw721_collection_info_symbol",
                &cw721_collection_info.symbol,
            );
    }

    if let Some(lock_depth_limit) = lock_depth_limit {
        constants.lock_depth_limit = lock_depth_limit;
        response = response.add_attribute("new_lock_depth_limit", lock_depth_limit.to_string());
    }

    if let Some(lock_expiry_duration_seconds) = lock_expiry_duration_seconds {
        constants.lock_expiry_duration_seconds = lock_expiry_duration_seconds;
        response = response.add_attribute(
            "new_lock_expiry_duration_seconds",
            lock_expiry_duration_seconds.to_string(),
        );
    }

    CONSTANTS.save(deps.storage, activate_at.nanos(), &constants)?;

    Ok(response)
}

fn delete_configs(
    deps: DepsMut<NeutronQuery>,
    _env: &Env,
    info: MessageInfo,
    timestamps: Vec<Timestamp>,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_whitelist_admin(&deps, &info)?;

    let timestamps_to_delete: Vec<u64> = timestamps
        .into_iter()
        .filter_map(|timestamp| {
            if CONSTANTS.has(deps.storage, timestamp.nanos()) {
                Some(timestamp.nanos())
            } else {
                None
            }
        })
        .collect();

    for timestamp in &timestamps_to_delete {
        CONSTANTS.remove(deps.storage, *timestamp);
    }

    Ok(Response::new()
        .add_attribute("action", "delete_configs")
        .add_attribute("sender", info.sender)
        .add_attribute(
            "deleted_configs_at_timestamps",
            timestamps_to_delete
                .into_iter()
                .map(|timestamp| timestamp.to_string())
                .collect::<Vec<String>>()
                .join(", "),
        ))
}

// Pause:
//     Validate that the contract isn't already paused
//     Validate sender is whitelist admin
//     Set paused to true and save the changes
fn pause_contract(
    deps: DepsMut<NeutronQuery>,
    env: &Env,
    info: MessageInfo,
) -> Result<Response<NeutronMsg>, ContractError> {
    let (timestamp, mut constants) =
        load_constants_active_at_timestamp(&deps.as_ref(), env.block.time)?;

    validate_sender_is_whitelist_admin(&deps, &info)?;

    constants.paused = true;
    CONSTANTS.save(deps.storage, timestamp, &constants)?;

    Ok(Response::new()
        .add_attribute("action", "pause_contract")
        .add_attribute("sender", info.sender)
        .add_attribute("paused", "true"))
}

// AddTranche:
//     Validate that the contract isn't paused
//     Validate sender is whitelist admin
//     Validate that the tranche with the same name doesn't already exist
//     Add new tranche to the store
fn add_tranche(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    tranche: TrancheInfo,
) -> Result<Response<NeutronMsg>, ContractError> {
    let tranche_name = tranche.name.trim().to_string();

    validate_sender_is_whitelist_admin(&deps, &info)?;
    validate_tranche_name_uniqueness(&deps, &tranche_name)?;

    let tranche_id = TRANCHE_ID.load(deps.storage)?;
    let tranche = Tranche {
        id: tranche_id,
        name: tranche_name,
        metadata: tranche.metadata,
    };

    TRANCHE_MAP.save(deps.storage, tranche_id, &tranche)?;
    TRANCHE_ID.save(deps.storage, &(tranche_id + 1))?;

    Ok(Response::new()
        .add_attribute("action", "add_tranche")
        .add_attribute("sender", info.sender)
        .add_attribute("tranche id", tranche.id.to_string())
        .add_attribute("tranche name", tranche.name)
        .add_attribute("tranche metadata", tranche.metadata))
}

// EditTranche:
//     Validate that the contract isn't paused
//     Validate sender is whitelist admin
//     Validate that the tranche with the given id exists
//     Validate that the tranche with the same name doesn't already exist
//     Update the tranche in the store
fn edit_tranche(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    tranche_id: u64,
    tranche_name: Option<String>,
    tranche_metadata: Option<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_whitelist_admin(&deps, &info)?;

    let mut tranche = TRANCHE_MAP.load(deps.storage, tranche_id)?;
    let old_tranche_name = tranche.name.clone();
    let old_tranche_metadata = tranche.metadata.clone();

    if let Some(new_tranche_name) = tranche_name {
        let new_tranche_name = new_tranche_name.trim().to_string();
        // If a new name is provided, we don't allow for it to be equal with
        // any of existing tranche names, including the one being updated.
        // If user wants to update only metadata they should provide None for tranche_name.
        validate_tranche_name_uniqueness(&deps, &new_tranche_name)?;

        tranche.name = new_tranche_name
    };

    tranche.metadata = tranche_metadata.unwrap_or(tranche.metadata);
    TRANCHE_MAP.save(deps.storage, tranche.id, &tranche)?;

    Ok(Response::new()
        .add_attribute("action", "edit_tranche")
        .add_attribute("sender", info.sender)
        .add_attribute("tranche id", tranche.id.to_string())
        .add_attribute("old tranche name", old_tranche_name)
        .add_attribute("old tranche metadata", old_tranche_metadata)
        .add_attribute("new tranche name", tranche.name)
        .add_attribute("new tranche metadata", tranche.metadata))
}

// CreateICQsForValidators:
//     Validate that the contract isn't paused
//     Validate that the first round has started
//     Validate received validator addresses
//     Validate that the sender paid enough deposit for ICQs creation
//     Create ICQ for each of the valid addresses
fn create_icqs_for_validators(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    constants: &Constants,
    validators: Vec<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let lsm_token_info_provider =
        match TokenManager::new(&deps.as_ref()).get_lsm_token_info_provider() {
            None => {
                return Err(new_generic_error(
                    "Cannot create validator ICQs: contract doesn't support locking of LSM tokens.",
                ))
            }
            Some(provider) => provider,
        };
    // This function will return error if the first round hasn't started yet. It is necessarry
    // that it has started, since handling the results of the interchain queries relies on this.
    compute_current_round_id(&env, constants)?;

    let mut valid_addresses = HashSet::new();
    for validator in validators
        .iter()
        .map(|validator| validator.trim().to_owned())
    {
        if !valid_addresses.contains(&validator)
            && validator.starts_with(COSMOS_VALIDATOR_PREFIX)
            && bech32::decode(&validator).is_ok()
            && !VALIDATOR_TO_QUERY_ID.has(deps.storage, validator.clone())
        {
            valid_addresses.insert(validator);
        }
    }

    let is_icq_manager = validate_address_is_icq_manager(&deps, info.sender.clone()).is_ok();

    // icq_manager can create ICQs without paying for them; in this case, the
    // funds are implicitly provided by the contract, and these can thus either be funds
    // sent to the contract beforehand, or they could be escrowed funds
    // that were returned to the contract when previous Interchain Queries were removed
    // amd the escrowed funds were removed
    if !is_icq_manager {
        validate_icq_deposit_funds_sent(deps, &info, valid_addresses.len() as u64)?;
    }

    let mut register_icqs_submsgs = vec![];
    for validator_address in valid_addresses.clone() {
        let msg = new_register_staking_validators_query_msg(
            lsm_token_info_provider.hub_connection_id.clone(),
            vec![validator_address.clone()],
            lsm_token_info_provider.icq_update_period,
        )
        .map_err(|err| {
            StdError::generic_err(format!(
                "Failed to create staking validators interchain query. Error: {err}"
            ))
        })?;

        register_icqs_submsgs.push(build_create_interchain_query_submsg(
            msg,
            validator_address,
        )?);
    }

    Ok(Response::new()
        .add_attribute("action", "create_icqs_for_validators")
        .add_attribute("sender", info.sender)
        .add_attribute(
            "validator_addresses",
            valid_addresses
                .into_iter()
                .collect::<Vec<String>>()
                .join(", "),
        )
        .add_submessages(register_icqs_submsgs))
}

// Validates that enough funds were sent to create ICQs for the given validator addresses.
fn validate_icq_deposit_funds_sent(
    deps: DepsMut<NeutronQuery>,
    info: &MessageInfo,
    num_created_icqs: u64,
) -> Result<(), ContractError> {
    let min_icq_deposit = query_min_interchain_query_deposit(&deps.as_ref())?;
    let sent_token = must_pay(info, &min_icq_deposit.denom)?;
    let min_icqs_deposit = min_icq_deposit.amount.u128() * (num_created_icqs as u128);
    if min_icqs_deposit > sent_token.u128() {
        return Err(ContractError::Std(
            StdError::generic_err(format!("Insufficient tokens sent to pay for {} interchain queries deposits. Sent: {}, Required: {}", num_created_icqs, Coin::new(sent_token, NATIVE_TOKEN_DENOM), Coin::new(min_icqs_deposit, NATIVE_TOKEN_DENOM)))));
    }

    Ok(())
}

fn add_icq_manager(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_whitelist_admin(&deps, &info)?;

    let user_addr = deps.api.addr_validate(&address)?;

    let is_icq_manager = validate_address_is_icq_manager(&deps, user_addr.clone());
    if is_icq_manager.is_ok() {
        return Err(ContractError::Std(StdError::generic_err(
            "Address is already an ICQ manager",
        )));
    }

    ICQ_MANAGERS.save(deps.storage, user_addr.clone(), &true)?;

    Ok(Response::new()
        .add_attribute("action", "add_icq_manager")
        .add_attribute("address", user_addr)
        .add_attribute("sender", info.sender))
}

fn remove_icq_manager(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_whitelist_admin(&deps, &info)?;

    let user_addr = deps.api.addr_validate(&address)?;

    let free_icq_creators = validate_address_is_icq_manager(&deps, user_addr.clone());
    if free_icq_creators.is_err() {
        return Err(ContractError::Std(StdError::generic_err(
            "Address is not an ICQ manager",
        )));
    }

    ICQ_MANAGERS.remove(deps.storage, user_addr.clone());

    Ok(Response::new()
        .add_attribute("action", "remove_icq_manager")
        .add_attribute("address", user_addr)
        .add_attribute("sender", info.sender))
}

// Tries to withdraw the given amount of the NATIVE_TOKEN_DENOM from
// the contract. These will in practice be funds that
// were returned to the contract when Interchain Queries
// were removed because a validator fell out of the
// top validators.
fn withdraw_icq_funds(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_icq_manager(&deps, info.sender.clone())?;

    // send the amount of native tokens to the sender
    let send = Coin {
        denom: NATIVE_TOKEN_DENOM.to_string(),
        amount,
    };

    Ok(Response::new()
        .add_attribute("action", "withdraw_icq_escrows")
        .add_attribute("sender", info.sender.clone())
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![send],
        }))
}

// This function will add a given liquidity deployment to the deployments that were performed.
// This will not actually perform any movement of funds; it is assumed to be called when
// a trusted party, e.g. a multisig, has performed some deployment,
// and the contract should be updated to reflect this.
// This will return an error if:
// * the given round has not started yet
// * the given tranche does not exist
// * the given proposal does not exist
// * there already is a deployment for the given round, tranche, and proposal
pub fn add_liquidity_deployment(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    constants: &Constants,
    deployment: LiquidityDeployment,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_whitelist_admin(&deps, &info)?;

    let round_id = deployment.round_id;
    let tranche_id = deployment.tranche_id;
    let proposal_id = deployment.proposal_id;

    // check that the round has started
    let current_round_id = compute_current_round_id(&env, constants)?;
    if round_id > current_round_id {
        return Err(ContractError::Std(StdError::generic_err(
            "Cannot add liquidity deployment for a round that has not started yet",
        )));
    }

    // check that the tranche with the given id exists
    TRANCHE_MAP.load(deps.storage, tranche_id)?;

    // check that the proposal with the given id exists
    PROPOSAL_MAP
        .load(deps.storage, (round_id, tranche_id, proposal_id))
        .map_err(|_| {
            ContractError::Std(StdError::generic_err(format!(
                "Proposal for round {round_id}, tranche {tranche_id}, and id {proposal_id} does not exist"
            )))
        })?;

    // check that there is no deployment for the given round, tranche, and proposal
    if LIQUIDITY_DEPLOYMENTS_MAP
        .may_load(deps.storage, (round_id, tranche_id, proposal_id))?
        .is_some()
    {
        return Err(ContractError::Std(StdError::generic_err(
            "There already is a deployment for the given round, tranche, and proposal",
        )));
    }

    let response = Response::new()
        .add_attribute("action", "add_liquidity_deployment")
        .add_attribute("sender", info.sender)
        .add_attribute("round_id", round_id.to_string())
        .add_attribute("tranche_id", tranche_id.to_string())
        .add_attribute("proposal_id", proposal_id.to_string())
        .add_attribute(
            "deployment",
            serde_json_wasm::to_string(&deployment).map_err(|_| {
                ContractError::Std(StdError::generic_err("Failed to serialize deployment"))
            })?,
        );

    LIQUIDITY_DEPLOYMENTS_MAP.save(
        deps.storage,
        (round_id, tranche_id, proposal_id),
        &deployment,
    )?;

    Ok(response)
}

// This function will remove a given liquidity deployment from the deployments that were performed.
// The main purpose is to correct a faulty entry added via add_liquidity_deployment.
// This will return an error if the deployment does not exist.
pub fn remove_liquidity_deployment(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_whitelist_admin(&deps, &info)?;

    // check that the deployment exists
    LIQUIDITY_DEPLOYMENTS_MAP.load(deps.storage, (round_id, tranche_id, proposal_id))?;

    let response = Response::new()
        .add_attribute("action", "remove_liquidity_deployment")
        .add_attribute("sender", info.sender)
        .add_attribute("round_id", round_id.to_string())
        .add_attribute("tranche_id", tranche_id.to_string())
        .add_attribute("proposal_id", proposal_id.to_string());

    LIQUIDITY_DEPLOYMENTS_MAP.remove(deps.storage, (round_id, tranche_id, proposal_id));

    Ok(response)
}

pub fn update_token_group_ratio(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    constants: &Constants,
    token_group_id: String,
    old_ratio: Decimal,
    new_ratio: Decimal,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_token_info_provider(&deps, &info)?;

    let current_round_id = compute_current_round_id(&env, constants)?;

    let tokens_ratio_changes = vec![TokenGroupRatioChange {
        token_group_id: token_group_id.clone(),
        old_ratio,
        new_ratio,
    }];

    apply_token_groups_ratio_changes(
        deps.storage,
        env.block.height,
        current_round_id,
        &tokens_ratio_changes,
    )?;

    let response = Response::new()
        .add_attribute("action", "update_token_group_ratio")
        .add_attribute("sender", info.sender)
        .add_attribute("current_round_id", current_round_id.to_string())
        .add_attribute("token_group_id", token_group_id.clone())
        .add_attribute("old_ratio", old_ratio.to_string())
        .add_attribute("new_ratio", new_ratio.to_string());

    Ok(response)
}

pub fn add_token_info_provider(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    constants: &Constants,
    provider_info: TokenInfoProviderInstantiateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_whitelist_admin(&deps, &info)?;

    let (token_info_provider_init_msgs, lsm_token_info_provider) =
        add_token_info_providers(&mut deps, vec![provider_info.clone()])?;

    // If LSM token info provider was added, apply proposal and round power changes immediately.
    if let Some(mut lsm_token_info_provider) = lsm_token_info_provider {
        handle_token_info_provider_add_remove(
            &mut deps,
            &env,
            constants,
            &mut lsm_token_info_provider,
            |token_group| TokenGroupRatioChange {
                token_group_id: token_group.0.clone(),
                old_ratio: Decimal::zero(),
                new_ratio: *token_group.1,
            },
        )?;
    }

    let response = Response::new()
        .add_attribute("action", "add_token_info_provider")
        .add_attribute("sender", info.sender)
        .add_attribute("token_info_provider", provider_info.to_string())
        .add_submessages(token_info_provider_init_msgs);

    Ok(response)
}

pub fn remove_token_info_provider(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    constants: &Constants,
    provider_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_whitelist_admin(&deps, &info)?;

    let mut token_info_provider =
        match TOKEN_INFO_PROVIDERS.may_load(deps.storage, provider_id.clone())? {
            Some(provider) => provider,
            None => {
                return Err(new_generic_error(format!(
                    "Token info provider with ID: {} doesn't exist.",
                    provider_id.clone()
                )))
            }
        };

    // Remove any voting power on proposals and rounds that comes from tokens of the given token info provider.
    handle_token_info_provider_add_remove(
        &mut deps,
        &env,
        constants,
        &mut token_info_provider,
        |token_group| TokenGroupRatioChange {
            token_group_id: token_group.0.clone(),
            old_ratio: *token_group.1,
            new_ratio: Decimal::zero(),
        },
    )?;

    TOKEN_INFO_PROVIDERS.remove(deps.storage, provider_id.clone());

    Ok(Response::new()
        .add_attribute("action", "remove_token_info_provider")
        .add_attribute("sender", info.sender)
        .add_attribute("provider_id", provider_id))
}

// Inserts or updates the drop info needed for conversion of lockups to dTokens
// Validate that the sender is a whitelist admin
// Sets the address of the drop core contract, the dToken denom, and the puppeteer address.
fn set_drop_token_info(
    deps: DepsMut<'_, NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    core_address: String,
    d_token_denom: String,
    puppeteer_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let whitelist_admins = WHITELIST_ADMINS.load(deps.storage)?;

    if !whitelist_admins.contains(&info.sender) {
        return Err(ContractError::Unauthorized);
    }

    let drop_token_info = DropTokenInfo {
        address: deps.api.addr_validate(&core_address)?,
        d_token_denom,
        puppeteer_address: deps.api.addr_validate(&puppeteer_address)?,
    };

    DROP_TOKEN_INFO.save(deps.storage, &drop_token_info)?;

    Ok(Response::new()
        .add_attribute("action", "set_drop_token_info")
        .add_attribute("sender", info.sender))
}

// Converts existing lockup (of lsm token) to dToken
// The user specifies one or several lockups in order to convert it on drop
// Hydro contract exectutes Bond message/s to drop core contract
// https://github.com/hadronlabs-org/drop-contracts/blob/bdbb1a7986b4448aff10db3baaa150d71527e815/contracts/core/src/contract.rs#L1002-L1002
// Drop core contract mints appropriate amount of dtoken given lsm shares sent
pub fn convert_lockup_to_dtoken(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    lock_ids: Vec<u64>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let lock_ids_set: HashSet<u64> = lock_ids.into_iter().collect();

    let lockups = query_user_lockups(
        &deps.as_ref(),
        info.sender.clone(),
        |lock| lock_ids_set.contains(&lock.lock_id),
        0,
        lock_ids_set.len() as u32,
    );

    let drop_info = DROP_TOKEN_INFO.load(deps.storage)?;

    let mut submsgs = vec![];

    for lockup in lockups {
        // Sender needs to be the owner of the lockups
        if lockup.owner != info.sender {
            return Err(ContractError::Unauthorized);
        }
        // Check if the lockup is already converted to dToken
        if lockup.funds.denom == drop_info.d_token_denom {
            return Err(ContractError::Std(StdError::generic_err(format!(
                "Lockup {lock_id} is already converted to dToken",
                lock_id = lockup.lock_id
            ))));
        }

        let convert_token_msg = DropExecuteMsg::Bond {
            receiver: None,
            r#ref: None,
        };

        let wasm_execute_msg = WasmMsg::Execute {
            contract_addr: drop_info.address.to_string(),
            msg: to_json_binary(&convert_token_msg)?,
            funds: vec![lockup.funds.clone()],
        };

        let reply_payload = ConvertLockupPayload {
            lock_id: lockup.lock_id,
            amount: lockup.funds.amount,
            sender: info.sender.clone(),
        };

        submsgs.push(
            SubMsg::reply_on_success(wasm_execute_msg, UNUSED_MSG_ID)
                .with_payload(to_json_vec(&ReplyPayload::ConvertLockup(reply_payload))?),
        );
    }

    Ok(Response::new()
        .add_submessages(submsgs)
        .add_attribute("action", "convert_lockup_to_dtoken"))
}

// For each reply the following happens: (reply_id is lock_id)
// 1. If there are any votes with lock_id - unvote
// 2. Apply proposal power changes
// 3. Update lock entry with converted denom and amount
// 4. Re-vote with the new lock entry if there were previous votes
// 5. Apply proposal power changes
pub fn convert_lockup_to_dtoken_reply(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    convert_lockup_payload: ConvertLockupPayload,
    msg: Reply,
) -> Result<Response<NeutronMsg>, ContractError> {
    let lock_id = convert_lockup_payload.lock_id;

    let sender: Addr = convert_lockup_payload.sender;

    let constants = load_current_constants(&deps.as_ref(), &env)?;

    let current_round_id = compute_current_round_id(&env, &constants)?;

    let mut total_locked_tokens = LOCKED_TOKENS.load(deps.storage)?;

    let issue_amount = match msg.result {
        SubMsgResult::Ok(res) => {
            // Collect all attributes from events
            let attrs: Vec<&Attribute> = res
                .events
                .iter()
                .flat_map(|e| e.attributes.iter())
                .collect();

            let issue_amount_attr = attrs
                .iter()
                .find(|attr| attr.key == "issue_amount")
                .ok_or_else(|| {
                    ContractError::Std(cosmwasm_std::StdError::generic_err(
                        "issue_amount attribute not found",
                    ))
                })?;

            issue_amount_attr
                .value
                .parse::<u128>()
                .map(Uint128::from)
                .map_err(|_| {
                    ContractError::Std(cosmwasm_std::StdError::generic_err(
                        "Invalid issue_amount attribute",
                    ))
                })?
        }
        SubMsgResult::Err { .. } => {
            return Err(ContractError::Std(cosmwasm_std::StdError::generic_err(
                "Submessage execution failed",
            )))
        }
    };

    total_locked_tokens = total_locked_tokens
        .checked_sub(convert_lockup_payload.amount.into())
        .ok_or_else(|| new_generic_error("Locked tokens underflow in reply"))?
        .checked_add(issue_amount.into())
        .ok_or_else(|| new_generic_error("Locked tokens overflow in reply"))?;

    LOCKED_TOKENS.save(deps.storage, &total_locked_tokens)?;

    let tranches: Vec<Tranche> = TRANCHE_MAP
        .range(deps.storage, None, None, Order::Ascending)
        .map(|t| t.unwrap().1)
        .collect();

    let mut tranches_with_votes: Vec<(u64, Vote)> = Vec::new();

    let mut unvotes_results_map: HashMap<u64, ProcessUnvotesResult> = HashMap::new();

    // Collect tranches with existing votes
    for tranche in tranches {
        let tranche_id = tranche.id;
        if let Some(vote) =
            VOTE_MAP_V2.may_load(deps.storage, ((current_round_id, tranche_id), lock_id))?
        {
            tranches_with_votes.push((tranche_id, vote));
        }
    }

    let mut token_manager = TokenManager::new(&deps.as_ref());

    // If there were existing votes, unvote them using the old lock entry
    for (tranche_id, _vote) in &tranches_with_votes {
        let mut target_votes = HashMap::new();
        target_votes.insert(lock_id, None);

        let unvotes_result: ProcessUnvotesResult =
            match process_unvotes(deps.storage, current_round_id, *tranche_id, &target_votes) {
                Ok(result) => result,
                Err(_) => continue,
            };
        unvotes_results_map.insert(*tranche_id, unvotes_result.clone());
        let unique_proposals_to_update: HashSet<u64> =
            unvotes_result.power_changes.keys().copied().collect();

        // Apply proposal power changes from unvotes
        apply_proposal_changes(
            &mut deps,
            &mut token_manager,
            current_round_id,
            unvotes_result.power_changes,
        )?;

        // Update the proposal in the proposal map, as well as the props by score map, after all changes
        // We can use update_proposal_and_props_by_score_maps as we already applied the proposal power changes
        for proposal_id in unique_proposals_to_update {
            let proposal =
                PROPOSAL_MAP.load(deps.storage, (current_round_id, *tranche_id, proposal_id))?;
            update_proposal_and_props_by_score_maps(
                deps.storage,
                current_round_id,
                *tranche_id,
                &proposal,
            )?;
        }
    }

    // update lock entry with converted denom and amount
    let drop_info = DROP_TOKEN_INFO.load(deps.storage)?;
    let mut lock_entry = LOCKS_MAP_V2.load(deps.storage, lock_id)?;
    let new_funds = Coin {
        denom: drop_info.d_token_denom.to_string(),
        amount: issue_amount,
    };
    lock_entry.funds = new_funds;
    LOCKS_MAP_V2.save(deps.storage, lock_id, &lock_entry, env.block.height)?;

    // Re-vote only if there were previous votes
    if !tranches_with_votes.is_empty() {
        for (tranche_id, vote) in &tranches_with_votes {
            let unvotes_result = unvotes_results_map
                .get(tranche_id)
                .expect("unvotes_result must exist for tranche");
            //vote with updated lock
            // Prepare context for voting
            let context = VoteProcessingContext {
                env: &env,
                constants: &constants,
                round_id: current_round_id,
                tranche_id: *tranche_id,
            };

            let proposals_votes = vec![ProposalToLockups {
                proposal_id: vote.prop_id,
                lock_ids: vec![lock_id],
            }];

            let mut lock_entries: HashMap<u64, LockEntryV2> = HashMap::new();
            lock_entries.insert(lock_id, lock_entry.clone());

            // Process new votes
            let votes_result = process_votes(
                &mut deps,
                &mut token_manager,
                context,
                &proposals_votes,
                &lock_entries,
                unvotes_result.locks_to_skip.clone(),
            )?;

            let unique_proposals_to_update: HashSet<u64> =
                votes_result.power_changes.keys().copied().collect();

            // Apply power changes from votes
            apply_proposal_changes(
                &mut deps,
                &mut token_manager,
                current_round_id,
                votes_result.power_changes,
            )?;

            // Update the proposal in the proposal map, as well as the props by score map, after all changes
            // We can use update_proposal_and_props_by_score_maps as we already applied the proposal power changes
            for proposal_id in unique_proposals_to_update {
                let proposal = PROPOSAL_MAP
                    .load(deps.storage, (current_round_id, *tranche_id, proposal_id))?;
                update_proposal_and_props_by_score_maps(
                    deps.storage,
                    current_round_id,
                    *tranche_id,
                    &proposal,
                )?;
            }
        }
    }
    Ok(Response::new()
        .add_attribute("action", "convert_lockup_success")
        .add_attribute("lock_id", lock_id.to_string())
        .add_attribute("sender", sender.to_string())
        .add_attribute("issue_amount", issue_amount.to_string()))
}

fn validate_sender_is_whitelist_admin(
    deps: &DepsMut<NeutronQuery>,
    info: &MessageInfo,
) -> Result<(), ContractError> {
    let whitelist_admins = WHITELIST_ADMINS.load(deps.storage)?;
    if !whitelist_admins.contains(&info.sender) {
        return Err(ContractError::Unauthorized);
    }

    Ok(())
}

fn validate_address_is_icq_manager(
    deps: &DepsMut<NeutronQuery>,
    address: Addr,
) -> Result<(), ContractError> {
    let is_manager = ICQ_MANAGERS.may_load(deps.storage, address)?;
    if is_manager.is_none() {
        return Err(ContractError::Unauthorized);
    }

    Ok(())
}

fn validate_sender_is_token_info_provider(
    deps: &DepsMut<NeutronQuery>,
    info: &MessageInfo,
) -> Result<(), ContractError> {
    let token_info_provider =
        TOKEN_INFO_PROVIDERS.may_load(deps.storage, info.sender.to_string())?;

    match token_info_provider {
        Some(_) => Ok(()),
        None => Err(ContractError::Unauthorized),
    }
}

fn validate_tranche_name_uniqueness(
    deps: &DepsMut<NeutronQuery>,
    tranche_name: &String,
) -> Result<(), ContractError> {
    for tranche_entry in TRANCHE_MAP.range(deps.storage, None, None, Order::Ascending) {
        let (_, tranche) = tranche_entry?;
        if tranche.name == *tranche_name {
            return Err(ContractError::Std(StdError::generic_err(
                "Tranche with the given name already exists. Duplicate tranche names are not allowed.",
            )));
        }
    }

    Ok(())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<NeutronQuery>, env: Env, msg: QueryMsg) -> Result<Binary, ContractError> {
    let binary = match msg {
        QueryMsg::Constants {} => to_json_binary(&query_constants(deps, env)?),
        QueryMsg::Tranches {} => to_json_binary(&query_tranches(deps)?),
        QueryMsg::AllUserLockups {
            address,
            start_from,
            limit,
        } => to_json_binary(&query_all_user_lockups(
            &deps, &env, address, start_from, limit,
        )?),
        QueryMsg::SpecificUserLockups { address, lock_ids } => to_json_binary(
            &query_specific_user_lockups(&deps, &env, address, lock_ids)?,
        ),
        QueryMsg::AllUserLockupsWithTrancheInfos {
            address,
            start_from,
            limit,
        } => to_json_binary(&query_all_user_lockups_with_tranche_infos(
            &deps, &env, address, start_from, limit,
        )?),
        QueryMsg::SpecificUserLockupsWithTrancheInfos { address, lock_ids } => to_json_binary(
            &query_specific_user_lockups_with_tranche_infos(&deps, &env, address, lock_ids)?,
        ),
        QueryMsg::ExpiredUserLockups {
            address,
            start_from,
            limit,
        } => to_json_binary(&query_expired_user_lockups(
            &deps, &env, address, start_from, limit,
        )?),
        QueryMsg::UserVotingPower { address } => {
            to_json_binary(&query_user_voting_power(deps, env, address)?)
        }
        QueryMsg::UserVotes {
            round_id,
            tranche_id,
            address,
        } => to_json_binary(&query_user_votes(deps, round_id, tranche_id, address)?),
        QueryMsg::UserVotedLocks {
            user_address,
            round_id,
            tranche_id,
            proposal_id,
        } => to_json_binary(&query_user_voted_locks(
            deps,
            user_address,
            round_id,
            tranche_id,
            proposal_id,
        )?),
        QueryMsg::LockVotesHistory {
            lock_id,
            start_from_round_id,
            stop_at_round_id,
            tranche_id,
        } => to_json_binary(&query_lock_votes_history(
            deps,
            env,
            lock_id,
            start_from_round_id,
            stop_at_round_id,
            tranche_id,
        )?),
        QueryMsg::AllVotes { start_from, limit } => {
            to_json_binary(&query_all_votes(deps, start_from, limit)?)
        }
        QueryMsg::AllVotesRoundTranche {
            round_id,
            tranche_id,
            start_from,
            limit,
        } => to_json_binary(&query_all_votes_round_tranche(
            deps, round_id, tranche_id, start_from, limit,
        )?),
        QueryMsg::Proposal {
            round_id,
            tranche_id,
            proposal_id,
        } => to_json_binary(&query_proposal(deps, round_id, tranche_id, proposal_id)?),
        QueryMsg::RoundTotalVotingPower { round_id } => {
            to_json_binary(&query_round_total_power(deps, round_id)?)
        }
        QueryMsg::RoundProposals {
            round_id,
            tranche_id,
            start_from,
            limit,
        } => to_json_binary(&query_round_tranche_proposals(
            deps, round_id, tranche_id, start_from, limit,
        )?),
        QueryMsg::CurrentRound {} => to_json_binary(&query_current_round_id(deps, env)?),
        QueryMsg::RoundEnd { round_id } => to_json_binary(&query_round_end(deps, env, round_id)?),
        QueryMsg::TopNProposals {
            round_id,
            tranche_id,
            number_of_proposals,
        } => to_json_binary(&query_top_n_proposals(
            deps,
            round_id,
            tranche_id,
            number_of_proposals,
        )?),
        QueryMsg::Whitelist {} => to_json_binary(&query_whitelist(deps)?),
        QueryMsg::WhitelistAdmins {} => to_json_binary(&query_whitelist_admins(deps)?),
        QueryMsg::TotalLockedTokens {} => to_json_binary(&query_total_locked_tokens(deps)?),
        QueryMsg::RegisteredValidatorQueries {} => {
            to_json_binary(&query_registered_validator_queries(deps)?)
        }
        QueryMsg::CanLockDenom { token_denom } => {
            to_json_binary(&query_can_lock_denom(&deps, &env, token_denom)?)
        }
        QueryMsg::ICQManagers {} => to_json_binary(&query_icq_managers(deps)?),
        QueryMsg::LiquidityDeployment {
            round_id,
            tranche_id,
            proposal_id,
        } => to_json_binary(&query_liquidity_deployment(
            deps,
            round_id,
            tranche_id,
            proposal_id,
        )?),
        QueryMsg::RoundTrancheLiquidityDeployments {
            round_id,
            tranche_id,
            start_from,
            limit,
        } => to_json_binary(&query_round_tranche_liquidity_deployments(
            deps, round_id, tranche_id, start_from, limit,
        )?),
        QueryMsg::TotalPowerAtHeight { height } => {
            to_json_binary(&query_total_power_at_height(&deps, &env, height)?)
        }
        QueryMsg::VotingPowerAtHeight { address, height } => {
            to_json_binary(&query_voting_power_at_height(&deps, &env, address, height)?)
        }
        QueryMsg::TokenInfoProviders {} => to_json_binary(&query_token_info_providers(deps)?),
        QueryMsg::Gatekeeper {} => to_json_binary(&query_gatekeeper(deps)?),
        QueryMsg::OwnerOf {
            token_id,
            include_expired,
        } => to_json_binary(&cw721::query_owner_of(
            deps,
            env,
            token_id,
            include_expired,
        )?),
        QueryMsg::Approval {
            token_id,
            spender,
            include_expired,
        } => to_json_binary(&cw721::query_approval(
            deps,
            env,
            token_id,
            spender,
            include_expired,
        )?),
        QueryMsg::Approvals {
            token_id,
            include_expired,
        } => to_json_binary(&cw721::query_approvals(
            deps,
            env,
            token_id,
            include_expired,
        )?),
        QueryMsg::AllOperators {
            owner,
            include_expired,
            start_after,
            limit,
        } => to_json_binary(&cw721::query_all_operators(
            deps,
            env,
            owner,
            include_expired,
            start_after,
            limit,
        )?),
        QueryMsg::NumTokens {} => to_json_binary(&cw721::query_num_tokens(deps)?),
        QueryMsg::Tokens {
            owner,
            start_after,
            limit,
        } => to_json_binary(&cw721::query_tokens(deps, owner, start_after, limit)?),
        QueryMsg::AllTokens { start_after, limit } => {
            to_json_binary(&cw721::query_all_tokens(deps, env, start_after, limit)?)
        }
        QueryMsg::CollectionInfo {} => to_json_binary(&cw721::query_collection_info(deps, env)?),
        QueryMsg::NftInfo { token_id } => {
            to_json_binary(&cw721::query_nft_info(deps, env, token_id)?)
        }
        QueryMsg::AllNftInfo {
            token_id,
            include_expired,
        } => to_json_binary(&cw721::query_all_nft_info(
            deps,
            env,
            token_id,
            include_expired,
        )?),
        QueryMsg::SimulateDtokenAmounts { lock_ids, address } => {
            to_json_binary(&query_simulate_dtoken_amounts(&deps, lock_ids, address)?)
        }
    }?;

    Ok(binary)
}

fn query_liquidity_deployment(
    deps: Deps<NeutronQuery>,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> StdResult<LiquidityDeploymentResponse> {
    let deployment =
        LIQUIDITY_DEPLOYMENTS_MAP.load(deps.storage, (round_id, tranche_id, proposal_id))?;
    Ok(LiquidityDeploymentResponse {
        liquidity_deployment: deployment,
    })
}

pub fn query_round_tranche_liquidity_deployments(
    deps: Deps<NeutronQuery>,
    round_id: u64,
    tranche_id: u64,
    start_from: u64,
    limit: u64,
) -> StdResult<RoundTrancheLiquidityDeploymentsResponse> {
    let mut deployments = vec![];
    for deployment in LIQUIDITY_DEPLOYMENTS_MAP
        .prefix((round_id, tranche_id))
        .range(deps.storage, None, None, Order::Ascending)
        .skip(start_from as usize)
        .take(limit as usize)
    {
        let (_, deployment) = deployment?;
        deployments.push(deployment);
    }

    Ok(RoundTrancheLiquidityDeploymentsResponse {
        liquidity_deployments: deployments,
    })
}

pub fn query_round_total_power(
    deps: Deps<NeutronQuery>,
    round_id: u64,
) -> StdResult<RoundTotalVotingPowerResponse> {
    let total_round_power = get_total_power_for_round(&deps, round_id)?;
    Ok(RoundTotalVotingPowerResponse {
        total_voting_power: total_round_power.to_uint_ceil(),
    })
}

pub fn query_constants(deps: Deps<NeutronQuery>, env: Env) -> StdResult<ConstantsResponse> {
    Ok(ConstantsResponse {
        constants: load_current_constants(&deps, &env)?,
    })
}

fn get_user_lockups_with_predicate(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    address: String,
    predicate: impl FnMut(&LockEntryV2) -> bool,
    start_from: u32,
    limit: u32,
) -> StdResult<Vec<LockEntryWithPower>> {
    let addr = deps.api.addr_validate(&address)?;
    let raw_lockups = query_user_lockups(deps, addr, predicate, start_from, limit);
    enrich_lockups_with_power(deps, env, raw_lockups)
}

pub fn enrich_lockups_with_power(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    lockups: Vec<LockEntryV2>,
) -> StdResult<Vec<LockEntryWithPower>> {
    let constants = load_current_constants(deps, env)?;
    let current_round_id = compute_current_round_id(env, &constants)?;
    let round_end = compute_round_end(&constants, current_round_id)?;
    let mut token_manager = TokenManager::new(deps);

    // enrich the lockups by computing the voting power for each lockup
    let enriched_lockups = lockups
        .iter()
        .map(|lock| {
            to_lockup_with_power(
                deps,
                &constants,
                &mut token_manager,
                current_round_id,
                round_end,
                lock.clone(),
            )
        })
        .collect();

    Ok(enriched_lockups)
}

pub fn query_all_user_lockups(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    address: String,
    start_from: u32,
    limit: u32,
) -> StdResult<AllUserLockupsResponse> {
    let lockups = get_user_lockups_with_predicate(deps, env, address, |_| true, start_from, limit)?;
    Ok(AllUserLockupsResponse { lockups })
}

pub fn query_specific_user_lockups(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    address: String,
    lock_ids: Vec<u64>,
) -> StdResult<SpecificUserLockupsResponse> {
    let lock_ids_set: HashSet<u64> = lock_ids.into_iter().collect();
    let lockups = get_user_lockups_with_predicate(
        deps,
        env,
        address,
        |lock| lock_ids_set.contains(&lock.lock_id),
        0,
        lock_ids_set.len() as u32,
    )?;

    Ok(SpecificUserLockupsResponse { lockups })
}

// Helper function to handle the common logic for both query functions
pub fn enrich_lockups_with_tranche_infos(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    locks: Vec<LockEntryWithPower>,
) -> Result<Vec<LockupWithPerTrancheInfo>, ContractError> {
    let tranche_ids = TRANCHE_MAP
        .keys(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    let constants = load_current_constants(deps, env)?;
    let current_round_id = compute_current_round_id(env, &constants)?;

    let mut enriched_lockups = Vec::with_capacity(locks.len());

    for lock_with_power in locks {
        let lockup_with_tranche_info = to_lockup_with_tranche_infos(
            deps,
            &constants,
            &tranche_ids,
            lock_with_power,
            current_round_id,
        )?;

        enriched_lockups.push(lockup_with_tranche_info);
    }

    Ok(enriched_lockups)
}

pub fn query_all_user_lockups_with_tranche_infos(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    address: String,
    start_from: u32,
    limit: u32,
) -> Result<AllUserLockupsWithTrancheInfosResponse, ContractError> {
    let lockups = query_all_user_lockups(deps, env, address.clone(), start_from, limit)?;
    let enriched_lockups = enrich_lockups_with_tranche_infos(deps, env, lockups.lockups)?;
    Ok(AllUserLockupsWithTrancheInfosResponse {
        lockups_with_per_tranche_infos: enriched_lockups,
    })
}

pub fn query_specific_user_lockups_with_tranche_infos(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    address: String,
    lock_ids: Vec<u64>,
) -> Result<SpecificUserLockupsWithTrancheInfosResponse, ContractError> {
    let lockups = query_specific_user_lockups(deps, env, address.clone(), lock_ids)?;
    let enriched_lockups = enrich_lockups_with_tranche_infos(deps, env, lockups.lockups)?;

    Ok(SpecificUserLockupsWithTrancheInfosResponse {
        lockups_with_per_tranche_infos: enriched_lockups,
    })
}

pub fn query_expired_user_lockups(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    address: String,
    start_from: u32,
    limit: u32,
) -> StdResult<ExpiredUserLockupsResponse> {
    let user_address = deps.api.addr_validate(&address)?;
    let expired_lockup_predicate = |l: &LockEntryV2| l.lock_end < env.block.time;

    Ok(ExpiredUserLockupsResponse {
        lockups: query_user_lockups(
            deps,
            user_address,
            expired_lockup_predicate,
            start_from,
            limit,
        ),
    })
}

pub fn query_proposal(
    deps: Deps<NeutronQuery>,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> StdResult<ProposalResponse> {
    Ok(ProposalResponse {
        proposal: get_proposal(deps.storage, round_id, tranche_id, proposal_id)?,
    })
}

pub fn query_user_voting_power(
    deps: Deps<NeutronQuery>,
    env: Env,
    address: String,
) -> StdResult<UserVotingPowerResponse> {
    Ok(UserVotingPowerResponse {
        voting_power: get_current_user_voting_power(
            &deps,
            &env,
            deps.api.addr_validate(&address)?,
        )?,
    })
}

// This function queries user votes for the given round and tranche.
// It goes through all user votes per lock_id and groups them by the
// proposal ID. The returned result will contain one VoteWithPower per
// each proposal ID, with the total power summed up from all lock IDs
// used to vote for that proposal. The votes that were cast with locks
// containing denoms whose ratio towards the base denom later dropped
// to zero will be filtered out.
pub fn query_user_votes(
    deps: Deps<NeutronQuery>,
    round_id: u64,
    tranche_id: u64,
    user_address: String,
) -> StdResult<UserVotesResponse> {
    let user_address = deps.api.addr_validate(&user_address)?;
    let mut voted_proposals_power_sum: HashMap<u64, Decimal> = HashMap::new();

    // The USER_LOCKS only has history starting from round_id: 3
    // Before that round, we cannot query USER_LOCKS, which is necessary to check votes from VOTE_MAP_V2
    // So, we need to query VOTE_MAP_V1 for those rounds
    let round_highest_height = get_highest_known_height_for_round_id(deps.storage, round_id)?;
    let is_historical_data_available =
        verify_historical_data_availability(deps.storage, round_highest_height);

    let mut votes = Vec::new();

    if is_historical_data_available.is_err() {
        // We still use the old VOTE_MAP_V1 store for where there's no historical data available
        votes = VOTE_MAP_V1
            .prefix(((round_id, tranche_id), user_address.clone()))
            .range(deps.storage, None, None, Order::Ascending)
            .filter_map(|vote| match vote {
                Err(_) => None,
                Ok(vote) => Some(vote.1),
            })
            .collect::<Vec<Vote>>();
    } else {
        // Get the user's locks from USER_LOCKS.
        // If the lock was created at round_highest_height, it should still be counted (hence the load_at_height + 1)
        let user_locks = USER_LOCKS
            .may_load_at_height(deps.storage, user_address.clone(), round_highest_height + 1)?
            .unwrap_or_default();

        // Collect all votes for user's locks
        for lock_id in user_locks {
            let vote = get_lock_vote(deps.storage, round_id, tranche_id, lock_id)?;
            if let Some(vote) = vote {
                votes.push(vote);
            }
        }
    }

    let mut token_manager = TokenManager::new(&deps);

    for vote in votes {
        let vote_power = calculate_vote_power(&mut token_manager, &deps, round_id, &vote)?;

        if vote_power == Decimal::zero() {
            continue;
        }

        let current_power = match voted_proposals_power_sum.get(&vote.prop_id) {
            None => Decimal::zero(),
            Some(current_power) => *current_power,
        };

        let new_power = current_power.checked_add(vote_power)?;
        voted_proposals_power_sum.insert(vote.prop_id, new_power);
    }

    if voted_proposals_power_sum.is_empty() {
        return Err(StdError::generic_err(
            "User didn't vote in the given round and tranche",
        ));
    }

    let votes: Vec<VoteWithPower> = voted_proposals_power_sum
        .into_iter()
        .map(|vote| VoteWithPower {
            prop_id: vote.0,
            power: vote.1,
        })
        .collect();

    Ok(UserVotesResponse { votes })
}

/// Helper function that collects all voted locks for a user in a specific round/tranche,
/// optionally filtered by a specific proposal ID.
fn collect_user_voted_locks(
    deps: Deps<NeutronQuery>,
    user_addr: Addr,
    round_id: u64,
    tranche_id: u64,
    filter_proposal_id: Option<u64>,
) -> StdResult<HashMap<u64, Vec<VotedLockInfo>>> {
    // Get lock IDs that the user can claim tributes for
    let user_lock_ids = get_user_claimable_locks(deps.storage, user_addr.clone())?;

    let mut proposal_locks: HashMap<u64, Vec<VotedLockInfo>> = HashMap::new();
    let mut token_manager = TokenManager::new(&deps);

    // Check each user lock to see if it voted in this round/tranche
    for lock_id in user_lock_ids {
        let Ok(Some(vote)) = get_lock_vote(deps.storage, round_id, tranche_id, lock_id) else {
            // Skip this lock if it did not vote during that round
            continue;
        };

        // If filtering by proposal ID, skip votes for other proposals
        if let Some(target_proposal_id) = filter_proposal_id {
            if vote.prop_id != target_proposal_id {
                continue;
            }
        }

        // Calculate vote power for this vote
        let vote_power = calculate_vote_power(&mut token_manager, &deps, round_id, &vote)?;

        proposal_locks
            .entry(vote.prop_id)
            .or_default()
            .push(VotedLockInfo {
                lock_id,
                vote_power,
            });
    }

    Ok(proposal_locks)
}

/// Returns all locks owned by a user that voted on proposals within a specific round and tranche.
/// It includes all locks the user can claim tributes for (current owner or last known owner).
///
/// For each proposal the user's locks voted on in this round/tranche, returns:
/// - Proposal ID
/// - List of the locks that voted, with each lock's ID and voting power
///
/// If proposal_id is provided, only returns locks that voted for that specific proposal.
///
/// Response format: Vec<(proposal_id, Vec<VotedLockInfo>)>
/// where VotedLockInfo contains { lock_id, vote_power }
///
/// Note: It is called by tribute contract's query_outstanding_tribute_claims
pub fn query_user_voted_locks(
    deps: Deps<NeutronQuery>,
    user_address: String,
    round_id: u64,
    tranche_id: u64,
    proposal_id: Option<u64>,
) -> StdResult<UserVotedLocksResponse> {
    let user_addr = deps.api.addr_validate(&user_address)?;

    // If proposal_id is provided, validate that the proposal exists
    if let Some(prop_id) = proposal_id {
        get_proposal(deps.storage, round_id, tranche_id, prop_id)?;
    }

    // Collect voted locks, optionally filtered by proposal
    let proposal_locks =
        collect_user_voted_locks(deps, user_addr, round_id, tranche_id, proposal_id)?;

    // Convert HashMap to Vec of tuples
    let voted_locks = proposal_locks.into_iter().collect();

    Ok(UserVotedLocksResponse { voted_locks })
}

/// Query to get the voting history for a specific lock with optional filters
/// - start_from_round_id: optional minimum round to start from (inclusive)
/// - stop_at_round_id: optional maximum round to stop at (inclusive)
/// - tranche_id: optional filter to only show votes for a specific tranche
///
/// Note: It is called by tribute contract's query_outstanding_lockup_claimable_coins and claim_tribute
pub fn query_lock_votes_history(
    deps: Deps<NeutronQuery>,
    env: Env,
    lock_id: u64,
    start_from_round_id: Option<u64>,
    stop_at_round_id: Option<u64>,
    tranche_id: Option<u64>,
) -> StdResult<LockVotesHistoryResponse> {
    let constants = load_current_constants(&deps, &env)?;
    let current_round_id = compute_current_round_id(&env, &constants)?;

    let mut vote_history = Vec::new();

    // Determine the round range to query
    let start_round = start_from_round_id.unwrap_or(0);
    let end_round = std::cmp::min(
        stop_at_round_id.unwrap_or(current_round_id),
        current_round_id,
    );

    // Validate round range
    if start_round > end_round {
        return Err(StdError::generic_err(format!(
            "start_round ({start_round}) must be less than or equal to end_round ({end_round})"
        )));
    }

    // Determine which tranches to check (computed once outside the loop)
    let tranches_to_check: Vec<u64> = if let Some(specific_tranche_id) = tranche_id {
        // check that the tranche with the given id exists
        TRANCHE_MAP.load(deps.storage, specific_tranche_id)?;

        // Only check the specified tranche
        vec![specific_tranche_id]
    } else {
        // Get all tranches
        TRANCHE_MAP
            .range(deps.storage, None, None, Order::Ascending)
            .collect::<StdResult<Vec<_>>>()?
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    };

    // Create token manager once outside the loops
    let mut token_manager = TokenManager::new(&deps);

    // Iterate through the specified round range
    for round_id in start_round..=end_round {
        for current_tranche_id in &tranches_to_check {
            // Check if this lock voted in this round/tranche
            if let Some(vote) = get_lock_vote(deps.storage, round_id, *current_tranche_id, lock_id)?
            {
                // Calculate the vote power for this vote
                let vote_power = calculate_vote_power(&mut token_manager, &deps, round_id, &vote)?;

                vote_history.push(LockVotesHistoryEntry {
                    round_id,
                    tranche_id: *current_tranche_id,
                    proposal_id: vote.prop_id,
                    vote_power,
                });
            }
        }
    }

    Ok(LockVotesHistoryResponse { vote_history })
}

pub fn query_all_votes(
    deps: Deps<NeutronQuery>,
    start_from: u32,
    limit: u32,
) -> StdResult<AllVotesResponse> {
    let vote_entries = VOTE_MAP_V2
        .range(deps.storage, None, None, Order::Ascending)
        .skip(start_from as usize)
        .take(limit as usize)
        .filter_map(|kv| {
            let ((round_id_tranche, lock_id), vote) = kv.ok()?;
            // For each vote, get the lock entry to determine the owner
            LOCKS_MAP_V2
                .load(deps.storage, lock_id)
                .ok() // Skip votes where we can't find the lock entry
                .map(|lock_entry| VoteEntry {
                    round_id: round_id_tranche.0,
                    tranche_id: round_id_tranche.1,
                    sender_addr: lock_entry.owner,
                    lock_id,
                    vote,
                })
        })
        .collect();

    Ok(AllVotesResponse {
        votes: vote_entries,
    })
}

pub fn query_all_votes_round_tranche(
    deps: Deps<NeutronQuery>,
    round_id: u64,
    tranche_id: u64,
    start_from: u32,
    limit: u32,
) -> StdResult<AllVotesResponse> {
    // Use prefix to filter by round_id and tranche_id directly
    let prefix = (round_id, tranche_id);

    let votes = VOTE_MAP_V2
        .prefix(prefix)
        .range(deps.storage, None, None, Order::Ascending)
        .skip(start_from as usize)
        .take(limit as usize)
        .filter_map(|kv| {
            let (lock_id, vote) = kv.ok()?;
            LOCKS_MAP_V2
                .load(deps.storage, lock_id)
                .ok() // Skip votes where we can't find the lock entry
                .map(|lock_entry| VoteEntry {
                    round_id,
                    tranche_id,
                    sender_addr: lock_entry.owner,
                    lock_id,
                    vote,
                })
        })
        .collect();

    Ok(AllVotesResponse { votes })
}

pub fn query_round_tranche_proposals(
    deps: Deps<NeutronQuery>,
    round_id: u64,
    tranche_id: u64,
    start_from: u32,
    limit: u32,
) -> StdResult<RoundProposalsResponse> {
    if TRANCHE_MAP.load(deps.storage, tranche_id).is_err() {
        return Err(StdError::generic_err("Tranche does not exist"));
    }

    let props = PROPOSAL_MAP
        .prefix((round_id, tranche_id))
        .range(deps.storage, None, None, Order::Ascending)
        .skip(start_from as usize)
        .take(limit as usize);

    let mut proposals = vec![];
    for proposal in props {
        let (_, proposal) = proposal?;
        proposals.push(proposal);
    }

    Ok(RoundProposalsResponse { proposals })
}

pub fn query_current_round_id(
    deps: Deps<NeutronQuery>,
    env: Env,
) -> StdResult<CurrentRoundResponse> {
    let constants = &load_current_constants(&deps, &env)?;
    let round_id = compute_round_id_for_timestamp(constants, env.block.time.nanos())?;

    let round_end = compute_round_end(constants, round_id)?;

    Ok(CurrentRoundResponse {
        round_id,
        round_end,
    })
}

pub fn query_round_end(
    deps: Deps<NeutronQuery>,
    env: Env,
    round_id: u64,
) -> StdResult<RoundEndResponse> {
    let constants = &load_current_constants(&deps, &env)?;
    let round_end = compute_round_end(constants, round_id)?;

    Ok(RoundEndResponse { round_end })
}

pub fn query_top_n_proposals(
    deps: Deps<NeutronQuery>,
    round_id: u64,
    tranche_id: u64,
    num: usize,
) -> StdResult<TopNProposalsResponse> {
    if TRANCHE_MAP.load(deps.storage, tranche_id).is_err() {
        return Err(StdError::generic_err("Tranche does not exist"));
    }

    // Iterate through PROPS_BY_SCORE to find the top num props
    let top_prop_ids: Vec<u64> = PROPS_BY_SCORE
        .sub_prefix((round_id, tranche_id))
        .range(deps.storage, None, None, Order::Descending)
        .take(num)
        .map(|x| match x {
            Ok((_, prop_id)) => prop_id,
            Err(_) => 0, // Handle the error case appropriately
        })
        .collect();

    let mut top_props = vec![];

    for prop_id in top_prop_ids {
        let prop = get_proposal(deps.storage, round_id, tranche_id, prop_id)?;
        top_props.push(prop);
    }

    // get total voting power for the round
    let total_voting_power = get_total_power_for_round(&deps, round_id)?.to_uint_ceil();

    let top_proposals = top_props
        .into_iter()
        .map(|mut prop| {
            prop.percentage = if total_voting_power.is_zero() {
                // if total voting power is zero, each proposal must necessarily have 0 score
                // avoid division by zero and set percentage to 0
                Uint128::zero()
            } else {
                (prop.power * Uint128::new(100)) / total_voting_power
            };
            prop
        })
        .collect();

    // return top props
    Ok(TopNProposalsResponse {
        proposals: top_proposals,
    })
}

pub fn query_tranches(deps: Deps<NeutronQuery>) -> StdResult<TranchesResponse> {
    let tranches = TRANCHE_MAP
        .range(deps.storage, None, None, Order::Ascending)
        .map(|t| t.unwrap().1)
        .collect();

    Ok(TranchesResponse { tranches })
}

fn query_user_lockups(
    deps: &Deps<NeutronQuery>,
    user_address: Addr,
    mut predicate: impl FnMut(&LockEntryV2) -> bool,
    start_from: u32,
    limit: u32,
) -> Vec<LockEntryV2> {
    let Ok(Some(lock_ids)) = USER_LOCKS.may_load(deps.storage, user_address.clone()) else {
        return vec![];
    };

    lock_ids
        .into_iter()
        .filter_map(|lock_id| LOCKS_MAP_V2.may_load(deps.storage, lock_id).ok().flatten())
        .filter(|lock| predicate(lock))
        .skip(start_from as usize)
        .take(limit as usize)
        .collect()
}

pub fn query_whitelist(deps: Deps<NeutronQuery>) -> StdResult<WhitelistResponse> {
    Ok(WhitelistResponse {
        whitelist: WHITELIST.load(deps.storage)?,
    })
}

pub fn query_whitelist_admins(deps: Deps<NeutronQuery>) -> StdResult<WhitelistAdminsResponse> {
    Ok(WhitelistAdminsResponse {
        admins: WHITELIST_ADMINS.load(deps.storage)?,
    })
}

pub fn query_total_locked_tokens(deps: Deps<NeutronQuery>) -> StdResult<TotalLockedTokensResponse> {
    Ok(TotalLockedTokensResponse {
        total_locked_tokens: LOCKED_TOKENS.load(deps.storage)?,
    })
}

// Returns all the validator queries that are registered
// by the contract right now, for each query returning the address of the validator it is
// associated with, plus its query id.
pub fn query_registered_validator_queries(
    deps: Deps<NeutronQuery>,
) -> StdResult<RegisteredValidatorQueriesResponse> {
    let query_ids = VALIDATOR_TO_QUERY_ID
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|l| {
            if l.is_err() {
                deps.api
                    .debug(&format!("Error when querying validator query id: {l:?}"));
            }
            l.ok()
        })
        .collect();

    Ok(RegisteredValidatorQueriesResponse { query_ids })
}

pub fn query_validators_info(
    deps: Deps<NeutronQuery>,
    round_id: u64,
) -> StdResult<Vec<ValidatorInfo>> {
    Ok(VALIDATORS_INFO
        .prefix(round_id)
        .range(deps.storage, None, None, Order::Ascending)
        .map(|l| l.unwrap().1)
        .collect())
}

pub fn query_validators_per_round(
    deps: Deps<NeutronQuery>,
    round_id: u64,
) -> StdResult<Vec<(u128, String)>> {
    Ok(VALIDATORS_PER_ROUND
        .sub_prefix(round_id)
        .range(deps.storage, None, None, Order::Descending)
        .map(|l| l.unwrap().0)
        .collect())
}

// Checks whether the token with the given denom can be locked in Hydro. Denom can be locked if it belongs to
// a known token group that has power ratio to base denom greater than zero.
pub fn query_can_lock_denom(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    token_denom: String,
) -> StdResult<CanLockDenomResponse> {
    let constants = load_current_constants(deps, env)?;
    let current_round = compute_current_round_id(env, &constants)?;

    let mut token_manager = TokenManager::new(deps);
    let token_group_id =
        match token_manager.validate_denom(deps, current_round, token_denom.clone()) {
            Err(_) => {
                return Ok(CanLockDenomResponse {
                    denom: token_denom.clone(),
                    can_be_locked: false,
                })
            }
            Ok(token_group_id) => token_group_id,
        };

    match token_manager.get_token_group_ratio(deps, current_round, token_group_id) {
        Err(_) => Ok(CanLockDenomResponse {
            denom: token_denom.clone(),
            can_be_locked: false,
        }),
        Ok(ratio) => Ok(CanLockDenomResponse {
            denom: token_denom.clone(),
            can_be_locked: ratio != Decimal::zero(),
        }),
    }
}

pub fn query_icq_managers(deps: Deps<NeutronQuery>) -> StdResult<ICQManagersResponse> {
    Ok(ICQManagersResponse {
        managers: ICQ_MANAGERS
            .range(deps.storage, None, None, Order::Ascending)
            .filter_map(|l| match l {
                Ok((k, _)) => Some(k),
                Err(_) => {
                    deps.api
                        .debug("Error parsing store when iterating ICQ managers!");
                    None
                }
            })
            .collect(),
    })
}

pub fn query_simulate_dtoken_amounts(
    deps: &Deps<NeutronQuery>,
    lock_ids: Vec<u64>,
    address: String,
) -> StdResult<DtokenAmountsResponse> {
    let lock_ids_set: HashSet<u64> = lock_ids.into_iter().collect();

    let lockups = query_user_lockups(
        deps,
        deps.api.addr_validate(&address)?,
        |lock| lock_ids_set.contains(&lock.lock_id),
        0,
        lock_ids_set.len() as u32,
    );

    let drop_info = DROP_TOKEN_INFO.load(deps.storage)?;

    let ratio: Decimal = deps
        .querier
        .query_wasm_smart(drop_info.address, &DropQueryMsg::ExchangeRate {})?;

    let delegations_response = deps.querier.query_wasm_smart::<DelegationsResponse>(
        drop_info.puppeteer_address,
        &PuppeteerQueryMsg::Extension {
            msg: QueryExtMsg::Delegations {},
        },
    )?;

    let mut result: Vec<DtokenAmountResponse> = Vec::new();

    for lockup in lockups {
        let denom_trace = query_ibc_denom_trace(deps, lockup.funds.denom)?;
        let path_parts: Vec<&str> = denom_trace.path.split("/").collect();
        if path_parts.len() != 2 || path_parts[0] != TRANSFER_PORT || path_parts[1] != "channel-1" {
            return Err(StdError::generic_err("Invalid IBC denom path".to_string()));
        }
        let base_denom_parts: Vec<&str> = denom_trace.base_denom.split("/").collect();
        let validator = base_denom_parts[0].to_string();

        let validator_info = delegations_response
            .delegations
            .delegations
            .iter()
            .find(|one| one.validator == validator)
            .ok_or_else(|| {
                StdError::generic_err(format!("validator info not found: {validator}"))
            })?;

        let input_amount = Decimal::from_atomics(lockup.funds.amount, 0)
            .map_err(|_| new_generic_error("Invalid fund amount".to_string()))
            .unwrap();

        let share = Decimal256::from_atomics(input_amount.atomics(), 0).unwrap();

        let real_amount = Uint128::try_from(
            share.checked_mul(validator_info.share_ratio)?.atomics()
                / Uint256::from(10u128.pow(18)),
        )?;

        let decimal_real = Decimal::from_atomics(real_amount, 18)
            .map_err(|_| new_generic_error("Invalid real_amount for Decimal".to_string()));

        let decimal_issue_amount = (Decimal::one() / ratio) * decimal_real.unwrap();

        let precision = Uint128::from(10u128.pow(18));
        let int_part = decimal_issue_amount.atomics() / precision;

        result.push(DtokenAmountResponse {
            lock_id: lockup.lock_id,
            dtoken_amount: int_part,
        });
    }

    Ok(DtokenAmountsResponse {
        dtokens_response: result,
    })
}

pub fn query_token_info_providers(
    deps: Deps<NeutronQuery>,
) -> StdResult<TokenInfoProvidersResponse> {
    Ok(TokenInfoProvidersResponse {
        providers: TokenManager::new(&deps).token_info_providers,
    })
}

pub fn query_gatekeeper(deps: Deps<NeutronQuery>) -> StdResult<GatekeeperResponse> {
    Ok(GatekeeperResponse {
        gatekeeper: GATEKEEPER.may_load(deps.storage)?.unwrap_or_default(),
    })
}

// Computes the current round_id by taking contract_start_time and dividing the time since
// by the round_length.
pub fn compute_current_round_id(env: &Env, constants: &Constants) -> StdResult<u64> {
    compute_round_id_for_timestamp(constants, env.block.time.nanos())
}

fn compute_round_id_for_timestamp(constants: &Constants, timestamp: u64) -> StdResult<u64> {
    // If the first round has not started yet, return an error
    if timestamp < constants.first_round_start.nanos() {
        return Err(StdError::generic_err("The first round has not started yet"));
    }
    let time_since_start = timestamp - constants.first_round_start.nanos();
    let round_id = time_since_start / constants.round_length;

    Ok(round_id)
}

pub fn compute_round_end(constants: &Constants, round_id: u64) -> StdResult<Timestamp> {
    let round_end = constants
        .first_round_start
        .plus_nanos(constants.round_length * (round_id + 1));

    Ok(round_end)
}

// When a user locks new tokens or extends an existing lock duration, this function checks if that user already
// voted on some proposals in the current round. This is done by looking up for user votes in all tranches.
// If there are such proposals, the function will update the voting power to reflect the voting power change
// caused by lock/extend_lock action. The following data structures will be updated:
//      PROPOSAL_MAP, PROPS_BY_SCORE, VOTE_MAP, SCALED_PROPOSAL_SHARES_MAP, PROPOSAL_TOTAL_MAP
#[allow(clippy::too_many_arguments)]
fn update_voting_power_on_proposals(
    deps: &mut DepsMut<NeutronQuery>,
    constants: &Constants,
    token_manager: &mut TokenManager,
    current_round: u64,
    old_lock_entry: Option<LockEntryV2>,
    new_lock_entry: LockEntryV2,
    token_group_id: String,
) -> Result<(), ContractError> {
    let round_end = compute_round_end(constants, current_round)?;
    let lock_epoch_length = constants.lock_epoch_length;

    let old_scaled_shares = match old_lock_entry.as_ref() {
        None => Uint128::zero(),
        Some(lock_entry) => get_lock_time_weighted_shares(
            &constants.round_lock_power_schedule,
            round_end,
            lock_entry,
            lock_epoch_length,
        ),
    };
    let new_scaled_shares = get_lock_time_weighted_shares(
        &constants.round_lock_power_schedule,
        round_end,
        &new_lock_entry,
        lock_epoch_length,
    );

    // With a future code changes, it could happen that the new power becomes less than
    // the old one. This calculation will cover both power increase and decrease scenarios.
    let power_change = if new_scaled_shares > old_scaled_shares {
        let power_change = Decimal::from_ratio(new_scaled_shares, Uint128::one())
            .checked_sub(Decimal::from_ratio(old_scaled_shares, Uint128::one()))?;

        VotingPowerChange::new(true, power_change)
    } else {
        let power_change = Decimal::from_ratio(old_scaled_shares, Uint128::one())
            .checked_sub(Decimal::from_ratio(new_scaled_shares, Uint128::one()))?;

        VotingPowerChange::new(false, power_change)
    };

    let tranche_ids = TRANCHE_MAP
        .keys(deps.storage, None, None, Order::Ascending)
        .collect::<Result<Vec<u64>, StdError>>()?;

    for tranche_id in tranche_ids {
        let vote = get_vote_for_update(
            deps,
            &new_lock_entry.owner,
            current_round,
            tranche_id,
            &old_lock_entry,
            &token_group_id,
        )?;

        if let Some(mut vote) = vote {
            let current_vote_shares = if vote.time_weighted_shares.0.eq(&token_group_id) {
                vote.time_weighted_shares.1
            } else {
                return Err(ContractError::Std(StdError::generic_err(
                    "Can't update the vote- it holds shares of a different token group",
                )));
            };

            let proposal = get_proposal(deps.storage, current_round, tranche_id, vote.prop_id)?;

            // Ensure that lock entry spans long enough to be allowed to vote for this proposal.
            // If not, we will not have this lock vote for the proposal, even if user voted for
            // only one proposal in the given round and tranche. Note that this condition will
            // always be satisfied when refreshing the lock entry, since we already checked this
            // condition when user voted with this lock entry, and refreshing the lock only allows
            // lock duration to be extended.
            if !can_lock_vote_for_proposal(current_round, constants, &new_lock_entry, &proposal)? {
                continue;
            }

            let new_vote_shares = if power_change.is_increased {
                current_vote_shares.checked_add(power_change.scaled_power_change)?
            } else {
                current_vote_shares.checked_sub(power_change.scaled_power_change)?
            };

            vote.time_weighted_shares.1 = new_vote_shares;

            VOTE_MAP_V2.save(
                deps.storage,
                ((current_round, tranche_id), new_lock_entry.lock_id),
                &vote,
            )?;

            // We are creating a new vote only if user creates a new lockup (i.e. locks more tokens) and
            // in this case we should insert voting allowed info as well. If user is refreshing a lockup
            // that was already used for voting, then this information is already saved in the store.
            if old_lock_entry.is_none() {
                let voting_allowed_round = current_round + proposal.deployment_duration;
                VOTING_ALLOWED_ROUND.save(
                    deps.storage,
                    (tranche_id, new_lock_entry.lock_id),
                    &voting_allowed_round,
                )?;
            }

            if power_change.is_increased {
                add_token_group_shares_to_proposal(
                    deps,
                    token_manager,
                    current_round,
                    vote.prop_id,
                    token_group_id.clone(),
                    power_change.scaled_power_change,
                )?;
            } else {
                remove_token_group_shares_from_proposal(
                    deps,
                    token_manager,
                    current_round,
                    vote.prop_id,
                    token_group_id.clone(),
                    power_change.scaled_power_change,
                )?;
            }

            update_proposal_and_props_by_score_maps(
                deps.storage,
                current_round,
                tranche_id,
                &proposal,
            )?;
        }
    }

    Ok(())
}

// This function will lookup the vote that needs to be updated when user locks
// more tokens or refreshes the existing lockup. Whether some vote should be
// updated or not is determined by the following logic:
//
// If user is refreshing a lock, we check if this lock was already voted with.
// If yes, this vote will be updated. If no, we will not add a vote for it. Instead,
// we leave user a choice to vote later.
// Is user is adding a new lock, we check if the user already voted for some proposals.
// If user voted for only one proposal, then the new lock power will be added to it.
// If user voted for multiple proposals, then we do not add a vote for new lock entry.
pub fn get_vote_for_update(
    deps: &mut DepsMut<NeutronQuery>,
    sender: &Addr,
    current_round: u64,
    tranche_id: u64,
    old_lock_entry: &Option<LockEntryV2>,
    token_group_id: &str,
) -> Result<Option<Vote>, ContractError> {
    if let Some(old_lock_entry) = old_lock_entry {
        let vote = get_lock_vote(
            deps.storage,
            current_round,
            tranche_id,
            old_lock_entry.lock_id,
        )?;
        return Ok(vote);
    }

    let voted_proposals: HashSet<u64> = USER_LOCKS
        .may_load(deps.storage, sender.clone())?
        .into_iter()
        .flatten()
        .flat_map(|lock_id| {
            VOTE_MAP_V2
                .load(deps.storage, ((current_round, tranche_id), lock_id))
                .map(|vote| vote.prop_id)
                .ok()
        })
        .collect();

    if voted_proposals.len() != 1 {
        return Ok(None);
    }

    let vote = voted_proposals
        .into_iter()
        .map(|prop_id| Vote {
            prop_id,
            time_weighted_shares: (token_group_id.to_string(), Decimal::zero()),
        })
        .next();

    Ok(vote)
}

// Ensure that the lock will have a power greater than 0 at the end of
// the round preceding the round in which the liquidity will be returned.
pub fn can_lock_vote_for_proposal(
    current_round: u64,
    constants: &Constants,
    lock_entry: &LockEntryV2,
    proposal: &Proposal,
) -> Result<bool, ContractError> {
    let power_required_round_id = current_round + proposal.deployment_duration - 1;
    let power_required_round_end = compute_round_end(constants, power_required_round_id)?;

    Ok(lock_entry.lock_end >= power_required_round_end)
}

#[allow(clippy::too_many_arguments)]
fn process_votes_and_apply_proposal_changes(
    deps: &mut DepsMut<NeutronQuery>,
    env: &Env,
    token_manager: &mut TokenManager,
    constants: &Constants,
    round_id: u64,
    tranche_id: u64,
    votes: &[ProposalToLockups],
    lock_entries: &HashMap<u64, LockEntryV2>,
    unvotes_result: ProcessUnvotesResult,
) -> Result<ProcessVotesResult, ContractError> {
    let context = VoteProcessingContext {
        env,
        constants,
        round_id,
        tranche_id,
    };

    let votes_result = process_votes(
        deps,
        token_manager,
        context,
        votes,
        lock_entries,
        unvotes_result.locks_to_skip,
    )?;

    let combined_power_changes = combine_proposal_power_updates(
        unvotes_result.power_changes,
        votes_result.power_changes.clone(),
    );

    let unique_proposals_to_update: HashSet<u64> = combined_power_changes.keys().copied().collect();

    // Apply combined proposal power changes from unvotes and votes
    apply_proposal_changes(deps, token_manager, round_id, combined_power_changes)?;

    // Update the proposal in the proposal map, as well as the props by score map, after all changes
    // We can use update_proposal_and_props_by_score_maps as we already applied the proposal power changes
    for proposal_id in unique_proposals_to_update {
        let proposal = PROPOSAL_MAP.load(deps.storage, (round_id, tranche_id, proposal_id))?;
        update_proposal_and_props_by_score_maps(deps.storage, round_id, tranche_id, &proposal)?;
    }

    Ok(votes_result)
}

/// This function relies on PROPOSAL_TOTAL_MAP and SCALED_PROPOSAL_SHARES_MAP being
/// already updated with the new proposal power.
fn update_proposal_and_props_by_score_maps(
    storage: &mut dyn Storage,
    round_id: u64,
    tranche_id: u64,
    proposal: &Proposal,
) -> Result<(), ContractError> {
    let mut proposal = proposal.clone();
    let proposal_id = proposal.proposal_id;

    // Delete the proposal's old power in PROPS_BY_SCORE
    PROPS_BY_SCORE.remove(
        storage,
        ((round_id, tranche_id), proposal.power.into(), proposal_id),
    );

    // Get the new total power of the proposal
    let total_power = get_total_power_for_proposal(storage, proposal_id)?;

    // Save the new power into the proposal
    proposal.power = total_power.to_uint_ceil();

    // Save the proposal
    PROPOSAL_MAP.save(storage, (round_id, tranche_id, proposal_id), &proposal)?;

    // Save the proposal's new power in PROPS_BY_SCORE
    PROPS_BY_SCORE.save(
        storage,
        ((round_id, tranche_id), proposal.power.into(), proposal_id),
        &proposal_id,
    )?;

    Ok(())
}

#[allow(clippy::too_many_arguments)] // complex function that needs a lot of arguments
fn update_total_time_weighted_shares<T>(
    deps: &mut DepsMut<NeutronQuery>,
    current_height: u64,
    constants: &Constants,
    token_manager: &mut TokenManager,
    current_round: u64,
    start_round_id: u64,
    end_round_id: u64,
    lock_end: u64,
    token_group_id: String,
    amount: Uint128,
    get_old_voting_power: T,
) -> StdResult<()>
where
    T: Fn(u64, Timestamp, Uint128) -> Uint128,
{
    // We need the token ratio to update the total voting power of current and possibly future rounds.
    // It is loaded outside of the loop to save some gas. We use the token ratio from the current round,
    // since it is not populated for future rounds yet.
    let token_ratio = token_manager.get_token_group_ratio(
        &deps.as_ref(),
        current_round,
        token_group_id.clone(),
    )?;

    for round in start_round_id..=end_round_id {
        let round_end = compute_round_end(constants, round)?;
        let lockup_length = lock_end - round_end.nanos();
        let scaled_amount = scale_lockup_power(
            &constants.round_lock_power_schedule,
            constants.lock_epoch_length,
            lockup_length,
            amount,
        );
        let old_voting_power = get_old_voting_power(round, round_end, amount);
        let scaled_shares = Decimal::from_ratio(scaled_amount, Uint128::one())
            - Decimal::from_ratio(old_voting_power, Uint128::one());

        // save some gas if there was no power change
        if scaled_shares.is_zero() {
            continue;
        }

        // add the shares to the total power in the round
        add_token_group_shares_to_round_total(
            deps.storage,
            current_height,
            round,
            token_group_id.clone(),
            token_ratio,
            scaled_shares,
        )?;
    }

    Ok(())
}

// Returns the number of locks for a given user
fn get_lock_count(deps: &Deps<NeutronQuery>, user_address: Addr) -> usize {
    match USER_LOCKS.may_load(deps.storage, user_address) {
        Ok(Some(lock_ids)) => lock_ids.len(),
        _ => 0,
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    msg: Reply,
) -> Result<Response<NeutronMsg>, ContractError> {
    let reply_paylod = from_json::<ReplyPayload>(&msg.payload);
    match reply_paylod {
        Ok(reply_payload) => match reply_payload {
            ReplyPayload::InstantiateTokenInfoProvider(token_info_provider) => {
                token_manager_handle_submsg_reply(deps, &env, token_info_provider, msg)
            }
            ReplyPayload::InstantiateGatekeeper => gatekeeper_handle_submsg_reply(deps, msg),
            ReplyPayload::ConvertLockup(convert_lockup_payload) => {
                convert_lockup_to_dtoken_reply(deps, env, convert_lockup_payload, msg)
            }
        },
        Err(_) => handle_submsg_reply(deps, msg),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    msg: SudoMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        SudoMsg::KVQueryResult { query_id } => {
            handle_delivered_interchain_query_result(deps, env, query_id)
        }
        _ => Err(ContractError::Std(StdError::generic_err(
            "Unexpected sudo message received",
        ))),
    }
}

struct VotingPowerChange {
    pub is_increased: bool,
    pub scaled_power_change: Decimal,
}

impl VotingPowerChange {
    pub fn new(is_increased: bool, scaled_power_change: Decimal) -> Self {
        Self {
            is_increased,
            scaled_power_change,
        }
    }
}
