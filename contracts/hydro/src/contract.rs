use std::collections::{HashMap, HashSet};

use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Decimal, Deps, DepsMut, Env,
    MessageInfo, Order, Reply, Response, StdError, StdResult, Storage, Timestamp, Uint128,
};
use cw2::set_contract_version;
use cw_utils::must_pay;
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;
use neutron_sdk::interchain_queries::v047::register_queries::new_register_staking_validators_query_msg;
use neutron_sdk::sudo::msg::SudoMsg;

use crate::error::ContractError;
use crate::lsm_integration::{
    add_validator_shares_to_round_total, get_total_power_for_round,
    get_validator_power_ratio_for_round, initialize_validator_store, validate_denom,
    COSMOS_VALIDATOR_PREFIX,
};
use crate::msg::{ExecuteMsg, InstantiateMsg, TrancheInfo};
use crate::query::{
    AllUserLockupsResponse, ConstantsResponse, CurrentRoundResponse, ExpiredUserLockupsResponse,
    ICQManagersResponse, LockEntryWithPower, ProposalResponse, QueryMsg,
    RegisteredValidatorQueriesResponse, RoundEndResponse, RoundProposalsResponse,
    RoundTotalVotingPowerResponse, TopNProposalsResponse, TotalLockedTokensResponse,
    TranchesResponse, UserVoteResponse, UserVotingPowerResponse, ValidatorPowerRatioResponse,
    WhitelistAdminsResponse, WhitelistResponse,
};
use crate::score_keeper::{
    add_validator_shares_to_proposal, get_total_power_for_proposal,
    remove_many_validator_shares_from_proposal, remove_validator_shares_from_proposal,
};
use crate::state::{
    Constants, LockEntry, Proposal, Tranche, ValidatorInfo, Vote, VoteWithPower, CONSTANTS,
    ICQ_MANAGERS, LOCKED_TOKENS, LOCKS_MAP, LOCK_ID, PROPOSAL_MAP, PROPS_BY_SCORE, PROP_ID,
    TRANCHE_ID, TRANCHE_MAP, VALIDATORS_INFO, VALIDATORS_PER_ROUND, VALIDATORS_STORE_INITIALIZED,
    VALIDATOR_TO_QUERY_ID, VOTE_MAP, WHITELIST, WHITELIST_ADMINS,
};
use crate::validators_icqs::{
    build_create_interchain_query_submsg, handle_delivered_interchain_query_result,
    handle_submsg_reply, query_min_interchain_query_deposit,
};

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const MAX_LOCK_ENTRIES: usize = 100;

pub const NATIVE_TOKEN_DENOM: &str = "untrn";

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
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

    let state = Constants {
        round_length: msg.round_length,
        lock_epoch_length: msg.lock_epoch_length,
        first_round_start: msg.first_round_start,
        max_locked_tokens: msg.max_locked_tokens.u128(),
        max_validator_shares_participating: msg.max_validator_shares_participating,
        hub_connection_id: msg.hub_connection_id,
        hub_transfer_channel_id: msg.hub_transfer_channel_id,
        icq_update_period: msg.icq_update_period,
        paused: false,
        is_in_pilot_mode: msg.is_in_pilot_mode,
    };

    CONSTANTS.save(deps.storage, &state)?;
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

    // the store for the first round is already initialized, since there is no previous round to copy information over from.
    VALIDATORS_STORE_INITIALIZED.save(deps.storage, 0, &true)?;

    Ok(Response::new()
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender.clone()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        ExecuteMsg::LockTokens { lock_duration } => lock_tokens(deps, env, info, lock_duration),
        ExecuteMsg::RefreshLockDuration {
            lock_ids,
            lock_duration,
        } => refresh_lock_duration(deps, env, info, lock_ids, lock_duration),
        ExecuteMsg::UnlockTokens {} => unlock_tokens(deps, env, info),
        ExecuteMsg::CreateProposal {
            tranche_id,
            title,
            description,
        } => create_proposal(deps, env, info, tranche_id, title, description),
        ExecuteMsg::Vote {
            tranche_id,
            proposal_id,
        } => vote(deps, env, info, tranche_id, proposal_id),
        ExecuteMsg::AddAccountToWhitelist { address } => add_to_whitelist(deps, env, info, address),
        ExecuteMsg::RemoveAccountFromWhitelist { address } => {
            remove_from_whitelist(deps, env, info, address)
        }
        ExecuteMsg::UpdateMaxLockedTokens { max_locked_tokens } => {
            update_max_locked_tokens(deps, info, max_locked_tokens)
        }
        ExecuteMsg::Pause {} => pause_contract(deps, info),
        ExecuteMsg::AddTranche { tranche } => add_tranche(deps, info, tranche),
        ExecuteMsg::EditTranche {
            tranche_id,
            tranche_name,
            tranche_metadata,
        } => edit_tranche(deps, info, tranche_id, tranche_name, tranche_metadata),
        ExecuteMsg::CreateICQsForValidators { validators } => {
            create_icqs_for_validators(deps, env, info, validators)
        }
        ExecuteMsg::AddICQManager { address } => add_icq_manager(deps, info, address),
        ExecuteMsg::RemoveICQManager { address } => remove_icq_manager(deps, info, address),
        ExecuteMsg::WithdrawICQFunds { amount } => withdraw_icq_funds(deps, info, amount),
    }
}

// LockTokens(lock_duration):
//     Receive tokens
//     Validate against the accepted denom
//     Update voting power on proposals if user already voted for any
//     Update total round power
//     Create entry in LocksMap
fn lock_tokens(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    lock_duration: u64,
) -> Result<Response<NeutronMsg>, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
    // if we are in pilot mode, only allow lockups of a single epoch
    if constants.is_in_pilot_mode {
        pilot_round_validate_lock_duration(constants.lock_epoch_length, lock_duration)?;
    } else {
        validate_lock_duration(constants.lock_epoch_length, lock_duration)?;
    }

    let current_round = compute_current_round_id(&env, &constants)?;
    initialize_validator_store(deps.storage, current_round)?;

    if info.funds.len() != 1 {
        return Err(ContractError::Std(StdError::generic_err(
            "Must provide exactly one coin to lock",
        )));
    }

    let funds = info.funds[0].clone();

    let validator =
        validate_denom(deps.as_ref(), env.clone(), &constants, funds.denom).map_err(|err| {
            ContractError::Std(StdError::generic_err(format!("validating denom: {}", err)))
        })?;

    // validate that this wouldn't cause the contract to have more locked tokens than the limit
    let amount_to_lock = info.funds[0].amount.u128();
    let locked_tokens = LOCKED_TOKENS.load(deps.storage)?;

    if locked_tokens + amount_to_lock > constants.max_locked_tokens {
        return Err(ContractError::Std(StdError::generic_err(
            "The limit for locking tokens has been reached. No more tokens can be locked.",
        )));
    }

    // validate that the user does not have too many locks
    if get_lock_count(deps.as_ref(), info.sender.clone()) >= MAX_LOCK_ENTRIES {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "User has too many locks, only {} locks allowed",
            MAX_LOCK_ENTRIES
        ))));
    }

    let lock_id = LOCK_ID.load(deps.storage)?;
    LOCK_ID.save(deps.storage, &(lock_id + 1))?;
    let lock_entry = LockEntry {
        lock_id,
        funds: info.funds[0].clone(),
        lock_start: env.block.time,
        lock_end: env.block.time.plus_nanos(lock_duration),
    };
    let lock_end = lock_entry.lock_end.nanos();
    LOCKS_MAP.save(deps.storage, (info.sender.clone(), lock_id), &lock_entry)?;
    LOCKED_TOKENS.save(deps.storage, &(locked_tokens + amount_to_lock))?;

    // If user already voted for some proposals in the current round, update the voting power on those proposals.
    let mut deps = deps;
    update_voting_power_on_proposals(
        &mut deps,
        &info.sender,
        &constants,
        current_round,
        None,
        lock_entry.clone(),
        validator.clone(),
    )?;

    // Calculate and update the total voting power info for current and all
    // future rounds in which the user will have voting power greather than 0
    let last_round_with_power = compute_round_id_for_timestamp(&constants, lock_end)? - 1;

    update_total_time_weighted_shares(
        &mut deps,
        &constants,
        current_round,
        last_round_with_power,
        lock_end,
        validator,
        lock_entry.funds.amount,
        |_, _, _| Uint128::zero(),
    )?;

    Ok(Response::new()
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
    lock_ids: Vec<u64>,
    lock_duration: u64,
) -> Result<Response<NeutronMsg>, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;

    if constants.is_in_pilot_mode {
        pilot_round_validate_lock_duration(constants.lock_epoch_length, lock_duration)?;
    } else {
        validate_lock_duration(constants.lock_epoch_length, lock_duration)?;
    }

    // if there are no lock_ids, return an error
    if lock_ids.is_empty() {
        return Err(ContractError::Std(StdError::generic_err(
            "No lock_ids provided",
        )));
    }

    let current_round_id = compute_current_round_id(&env, &constants)?;
    initialize_validator_store(deps.storage, current_round_id)?;

    let mut response = Response::new()
        .add_attribute("action", "refresh_lock_duration")
        .add_attribute("sender", info.clone().sender)
        .add_attribute("lock_count", lock_ids.len().to_string());

    for lock_id in lock_ids {
        let (new_lock_end, old_lock_end) = refresh_single_lock(
            &mut deps,
            &info,
            &env,
            &constants,
            current_round_id,
            lock_id,
            lock_duration,
        )?;

        response = response.add_attribute(
            format!("lock_id_{}_old_end", lock_id),
            old_lock_end.to_string(),
        );
        response = response.add_attribute(
            format!("lock_id_{}_new_end", lock_id),
            new_lock_end.to_string(),
        );
    }

    Ok(response)
}

fn refresh_single_lock(
    deps: &mut DepsMut<'_, NeutronQuery>,
    info: &MessageInfo,
    env: &Env,
    constants: &Constants,
    current_round_id: u64,
    lock_id: u64,
    new_lock_duration: u64,
) -> Result<(u64, u64), ContractError> {
    let mut lock_entry = LOCKS_MAP.load(deps.storage, (info.sender.clone(), lock_id))?;
    let old_lock_entry = lock_entry.clone();
    deps.api.debug(&format!("lock_entry: {:?}", lock_entry));
    let new_lock_end = env.block.time.plus_nanos(new_lock_duration).nanos();
    let old_lock_end = lock_entry.lock_end.nanos();
    if new_lock_end <= old_lock_end {
        return Err(ContractError::Std(StdError::generic_err(
            "Shortening locks is not allowed, new lock end time must be after the old lock end",
        )));
    }
    lock_entry.lock_end = Timestamp::from_nanos(new_lock_end);
    LOCKS_MAP.save(deps.storage, (info.sender.clone(), lock_id), &lock_entry)?;
    let validator_result = validate_denom(
        deps.as_ref(),
        env.clone(),
        constants,
        lock_entry.funds.denom.clone(),
    );
    if validator_result.is_err() {
        return Err(ContractError::Std(StdError::generic_err(
            "Lock denom is for a validator who is currently not in the set, try refreshing when the validator has enoug delegation",
        )));
    }
    let validator = validator_result.unwrap();
    update_voting_power_on_proposals(
        deps,
        &info.sender,
        constants,
        current_round_id,
        Some(old_lock_entry),
        lock_entry.clone(),
        validator.clone(),
    )?;
    let old_last_round_with_power = compute_round_id_for_timestamp(constants, old_lock_end)? - 1;
    let new_last_round_with_power = compute_round_id_for_timestamp(constants, new_lock_end)? - 1;
    update_total_time_weighted_shares(
        deps,
        constants,
        current_round_id,
        new_last_round_with_power,
        new_lock_end,
        validator,
        lock_entry.funds.amount,
        |round, round_end, locked_amount| {
            if round > old_last_round_with_power {
                return Uint128::zero();
            }

            let old_lockup_length = old_lock_end - round_end.nanos();
            scale_lockup_power(
                constants.lock_epoch_length,
                old_lockup_length,
                locked_amount,
            )
        },
    )?;
    Ok((new_lock_end, old_lock_end))
}

// Validate that the lock duration (given in nanos) is either 1, 2, 3, 6, or 12 epochs
fn validate_lock_duration(lock_epoch_length: u64, lock_duration: u64) -> Result<(), ContractError> {
    if lock_duration != lock_epoch_length
        && lock_duration != lock_epoch_length * 2
        && lock_duration != lock_epoch_length * 3
        && lock_duration != lock_epoch_length * 6
        && lock_duration != lock_epoch_length * 12
    {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "Lock duration must be 1, 2, 3, 6, or 12 epochs: {}, {}, {}, {}, or {}; but was: {}",
            lock_epoch_length,
            lock_epoch_length * 2,
            lock_epoch_length * 3,
            lock_epoch_length * 6,
            lock_epoch_length * 12,
            lock_duration
        ))));
    }

    Ok(())
}
// This is a separate validation function which will be used in the pilot rounds
// of the contract, making sure that only lockups of a single epoch are allowed.
fn pilot_round_validate_lock_duration(
    lock_epoch_length: u64,
    lock_duration: u64,
) -> Result<(), ContractError> {
    if lock_duration != lock_epoch_length {
        return Err(ContractError::Std(StdError::generic_err(
            "Lock duration must be 1 epoch",
        )));
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
) -> Result<Response<NeutronMsg>, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
    // TODO: reenable this when we implement slashing
    // validate_previous_round_vote(&deps, &env, info.sender.clone())?;

    // Iterate all locks for the caller and unlock them if lock_end < now
    let locks =
        LOCKS_MAP
            .prefix(info.sender.clone())
            .range(deps.storage, None, None, Order::Ascending);

    let mut to_delete = vec![];
    let mut total_unlocked_amount = Uint128::zero();

    let mut response = Response::new()
        .add_attribute("action", "unlock_tokens")
        .add_attribute("sender", info.sender.to_string());

    let mut unlocked_lock_ids = vec![];
    let mut unlocked_tokens = vec![];
    for lock in locks {
        let (lock_id, lock_entry) = lock?;
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

            // Delete entry from LocksMap
            to_delete.push((info.sender.clone(), lock_id));

            unlocked_lock_ids.push(lock_id.to_string());
            unlocked_tokens.push(send.to_string());
        }
    }

    // Delete unlocked locks
    for (addr, lock_id) in to_delete {
        LOCKS_MAP.remove(deps.storage, (addr, lock_id));
    }

    if !total_unlocked_amount.is_zero() {
        LOCKED_TOKENS.update(
            deps.storage,
            |locked_tokens| -> Result<u128, ContractError> {
                Ok(locked_tokens - total_unlocked_amount.u128())
            },
        )?;
    }

    Ok(response
        .add_attribute("unlocked_lock_ids", unlocked_lock_ids.join(", "))
        .add_attribute("unlocked_tokens", unlocked_tokens.join(", ")))
}

// prevent clippy from warning for unused function
// TODO: reenable this when we enable slashing
#[allow(dead_code)]
fn validate_previous_round_vote(
    deps: &DepsMut<NeutronQuery>,
    env: &Env,
    sender: Addr,
) -> Result<(), ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;
    let current_round_id = compute_current_round_id(env, &constants)?;
    if current_round_id > 0 {
        let previous_round_id = current_round_id - 1;
        for tranche_id in TRANCHE_MAP.keys(deps.storage, None, None, Order::Ascending) {
            if VOTE_MAP
                .may_load(
                    deps.storage,
                    (previous_round_id, tranche_id?, sender.clone()),
                )?
                .is_some()
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
// Then, it will create the proposal in the specified tranche and in the current round.
fn create_proposal(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    tranche_id: u64,
    title: String,
    description: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;
    validate_contract_is_not_paused(&constants)?;

    let round_id = compute_current_round_id(&env, &constants)?;
    initialize_validator_store(deps.storage, round_id)?;

    // validate that the sender is on the whitelist
    let whitelist = WHITELIST.load(deps.storage)?;

    if !whitelist.contains(&info.sender) {
        return Err(ContractError::Unauthorized);
    }

    // check that the tranche with the given id exists
    TRANCHE_MAP.load(deps.storage, tranche_id)?;

    let proposal_id = PROP_ID.load(deps.storage)?;

    let proposal = Proposal {
        round_id,
        tranche_id,
        proposal_id,
        power: Uint128::zero(),
        percentage: Uint128::zero(),
        title: title.trim().to_string(),
        description: description.trim().to_string(),
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
        .add_attribute("proposal_description", proposal.description))
}

pub fn scale_lockup_power(lock_epoch_length: u64, lockup_time: u64, raw_power: Uint128) -> Uint128 {
    let two: Uint128 = 2u16.into();

    // Scale lockup power
    // 1x if lockup is between 0 and 1 epochs
    // 1.25x if lockup is between 1 and 2 epochs
    // 1.5x if lockup is between 2 and 3 epochs
    // 2x if lockup is between 3 and 6 epochs
    // 4x if lockup is between 6 and 12 epochs
    // TODO: is there a less funky way to do Uint128 math???
    match lockup_time {
        // 4x if lockup is over 6 epochs
        _ if lockup_time > lock_epoch_length * 6 => raw_power * two * two,
        // 2x if lockup is between 3 and 6 epochs
        _ if lockup_time > lock_epoch_length * 3 => raw_power * two,
        // 1.5x if lockup is between 2 and 3 epochs
        _ if lockup_time > lock_epoch_length * 2 => raw_power + (raw_power / two),
        // 1.25x if lockup is between 1 and 2 epochs
        _ if lockup_time > lock_epoch_length => raw_power + (raw_power / (two * two)),
        // Covers 0 and 1 epoch which have no scaling
        _ => raw_power,
    }
}

fn vote(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    tranche_id: u64,
    proposal_id: u64,
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
    let constants = CONSTANTS.load(deps.storage)?;
    validate_contract_is_not_paused(&constants)?;

    let round_id = compute_current_round_id(&env, &constants)?;
    // voting can never be the first action in a round (since one can only vote on proposals in the current round, and a proposal must be created first)
    // however, to be safe, we initialize the validator store here, since this is more robust in case we change something about voting later
    initialize_validator_store(deps.storage, round_id)?;

    // check that the tranche with the given id exists
    TRANCHE_MAP.load(deps.storage, tranche_id)?;

    // compute the round end
    let round_end = compute_round_end(&constants, round_id)?;

    let mut response = Response::new()
        .add_attribute("action", "vote")
        .add_attribute("sender", info.sender.to_string());

    // Get any existing vote for this sender and reverse it- this may be a vote for a different proposal (if they are switching their vote),
    // or it may be a vote for the same proposal (if they have increased their power by locking more and want to update their vote).
    // TODO: this could be made more gas-efficient by using a separate path with fewer writes if the vote is for the same proposal
    let vote = VOTE_MAP.load(deps.storage, (round_id, tranche_id, info.sender.clone()));
    if let Ok(vote) = vote {
        // Load the proposal in the vote
        let mut proposal = PROPOSAL_MAP.load(deps.storage, (round_id, tranche_id, vote.prop_id))?;

        // Remove proposal's old power in PROPS_BY_SCORE
        PROPS_BY_SCORE.remove(
            deps.storage,
            (
                (round_id, proposal.tranche_id),
                proposal.power.into(),
                vote.prop_id,
            ),
        );

        remove_many_validator_shares_from_proposal(
            deps.storage,
            round_id,
            vote.prop_id,
            vote.time_weighted_shares
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect::<Vec<_>>(),
        )?;

        // save the new power into the proposal
        let total_power = get_total_power_for_proposal(deps.as_ref().storage, vote.prop_id)?;
        proposal.power = total_power.to_uint_ceil(); // TODO: decide whether we need to round or represent as decimals

        // Save the proposal
        PROPOSAL_MAP.save(
            deps.storage,
            (round_id, tranche_id, vote.prop_id),
            &proposal,
        )?;

        // Add proposal's new power in PROPS_BY_SCORE
        if proposal.power > Uint128::zero() {
            PROPS_BY_SCORE.save(
                deps.storage,
                (
                    (round_id, proposal.tranche_id),
                    proposal.power.into(),
                    vote.prop_id,
                ),
                &vote.prop_id,
            )?;
        }

        // Delete vote
        VOTE_MAP.remove(deps.storage, (round_id, tranche_id, info.sender.clone()));

        response = response.add_attribute("old_proposal_id", vote.prop_id.to_string());
    }

    let lock_epoch_length = CONSTANTS.load(deps.storage)?.lock_epoch_length;

    // Get sender's locked shares for each validator
    let mut time_weighted_shares_map: HashMap<String, Decimal> = HashMap::new();
    let locks =
        LOCKS_MAP
            .prefix(info.sender.clone())
            .range(deps.storage, None, None, Order::Ascending);

    for lock in locks {
        let (_, lock_entry) = lock?;

        // get the validator from the denom
        let validator_result = validate_denom(
            deps.as_ref(),
            env.clone(),
            &constants,
            lock_entry.clone().funds.denom,
        );
        if validator_result.is_err() {
            deps.api.debug(&
                format!(
                "Denom {} is not a valid validator denom; validator might not be in the current set of top validators by delegation",
                lock_entry.funds.denom
            ));
            // skip this lock entry, since the locked shares do not belong to a validator that we want to take into account
            continue;
        }
        let validator = validator_result.unwrap();

        let scaled_shares = get_lock_time_weighted_shares(round_end, lock_entry, lock_epoch_length);

        // add the shares to the map
        let shares = match time_weighted_shares_map.get(&validator) {
            Some(shares) => *shares,
            None => Decimal::zero(),
        };
        let new_shares = shares.checked_add(Decimal::from_ratio(scaled_shares, Uint128::one()))?;

        // insert the shares into the time_weigted_shares_map
        time_weighted_shares_map.insert(validator.clone(), new_shares);
    }

    // if the user doesn't have any shares that give voting power, we don't need to do anything
    if time_weighted_shares_map.is_empty() {
        return Ok(response);
    }

    // update the proposal's power with the new shares
    for (validator, num_shares) in time_weighted_shares_map.iter() {
        // add the validator shares to the proposal
        add_validator_shares_to_proposal(
            deps.storage,
            round_id,
            proposal_id,
            validator.to_string(),
            *num_shares,
        )?;
    }

    // update the proposal in the proposal map, as well as the props by score map
    update_proposal_and_props_by_score_maps(deps.storage, round_id, tranche_id, proposal_id)?;

    // Create vote in Votemap
    let vote = Vote {
        prop_id: proposal_id,
        time_weighted_shares: time_weighted_shares_map,
    };
    VOTE_MAP.save(deps.storage, (round_id, tranche_id, info.sender), &vote)?;

    Ok(response.add_attribute("proposal_id", vote.prop_id.to_string()))
}

// Returns the time-weighted amount of shares locked in the given lock entry in a round with the given end time,
// and using the given lock epoch length.
fn get_lock_time_weighted_shares(
    round_end: Timestamp,
    lock_entry: LockEntry,
    lock_epoch_length: u64,
) -> Uint128 {
    if round_end.nanos() > lock_entry.lock_end.nanos() {
        return Uint128::zero();
    }
    let lockup_length = lock_entry.lock_end.nanos() - round_end.nanos();
    scale_lockup_power(lock_epoch_length, lockup_length, lock_entry.funds.amount)
}

// Adds a new account address to the whitelist.
fn add_to_whitelist(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
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
    let constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
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

fn update_max_locked_tokens(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    max_locked_tokens: u128,
) -> Result<Response<NeutronMsg>, ContractError> {
    let mut constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
    validate_sender_is_whitelist_admin(&deps, &info)?;

    constants.max_locked_tokens = max_locked_tokens;
    CONSTANTS.save(deps.storage, &constants)?;

    Ok(Response::new()
        .add_attribute("action", "update_max_locked_tokens")
        .add_attribute("sender", info.sender)
        .add_attribute("max_locked_tokens", max_locked_tokens.to_string()))
}

// Pause:
//     Validate that the contract isn't already paused
//     Validate sender is whitelist admin
//     Set paused to true and save the changes
fn pause_contract(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
) -> Result<Response<NeutronMsg>, ContractError> {
    let mut constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
    validate_sender_is_whitelist_admin(&deps, &info)?;

    constants.paused = true;
    CONSTANTS.save(deps.storage, &constants)?;

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
    info: MessageInfo,
    tranche: TrancheInfo,
) -> Result<Response<NeutronMsg>, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;
    let tranche_name = tranche.name.trim().to_string();

    validate_contract_is_not_paused(&constants)?;
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
    info: MessageInfo,
    tranche_id: u64,
    tranche_name: Option<String>,
    tranche_metadata: Option<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
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
    validators: Vec<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;
    validate_contract_is_not_paused(&constants)?;
    // This function will return error if the first round hasn't started yet. It is necessarry
    // that it has started, since handling the results of the interchain queries relies on this.
    let round_id = compute_current_round_id(&env, &constants)?;
    initialize_validator_store(deps.storage, round_id)?;

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
            constants.hub_connection_id.clone(),
            vec![validator_address.clone()],
            constants.icq_update_period,
        )
        .map_err(|err| {
            StdError::generic_err(format!(
                "Failed to create staking validators interchain query. Error: {}",
                err
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
    deps: DepsMut<'_, NeutronQuery>,
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
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
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
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
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
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
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

fn validate_contract_is_not_paused(constants: &Constants) -> Result<(), ContractError> {
    match constants.paused {
        true => Err(ContractError::Paused),
        false => Ok(()),
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
pub fn query(deps: Deps<NeutronQuery>, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Constants {} => to_json_binary(&query_constants(deps)?),
        QueryMsg::Tranches {} => to_json_binary(&query_tranches(deps)?),
        QueryMsg::AllUserLockups {
            address,
            start_from,
            limit,
        } => to_json_binary(&query_all_user_lockups(
            deps, env, address, start_from, limit,
        )?),
        QueryMsg::ExpiredUserLockups {
            address,
            start_from,
            limit,
        } => to_json_binary(&query_expired_user_lockups(
            deps, env, address, start_from, limit,
        )?),
        QueryMsg::UserVotingPower { address } => {
            to_json_binary(&query_user_voting_power(deps, env, address)?)
        }
        QueryMsg::UserVote {
            round_id,
            tranche_id,
            address,
        } => to_json_binary(&query_user_vote(deps, round_id, tranche_id, address)?),
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
        QueryMsg::RoundEnd { round_id } => to_json_binary(&query_round_end(deps, round_id)?),
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
        QueryMsg::ValidatorPowerRatio {
            validator,
            round_id,
        } => to_json_binary(&query_validator_power_ratio(deps, validator, round_id)?),
        QueryMsg::ICQManagers {} => to_json_binary(&query_icq_managers(deps)?),
    }
}

pub fn query_round_total_power(
    deps: Deps<NeutronQuery>,
    round_id: u64,
) -> StdResult<RoundTotalVotingPowerResponse> {
    let total_round_power = get_total_power_for_round(deps, round_id)?;
    Ok(RoundTotalVotingPowerResponse {
        total_voting_power: total_round_power.to_uint_ceil(), // TODO: decide on rounding
    })
}

pub fn query_constants(deps: Deps<NeutronQuery>) -> StdResult<ConstantsResponse> {
    Ok(ConstantsResponse {
        constants: CONSTANTS.load(deps.storage)?,
    })
}

pub fn query_all_user_lockups(
    deps: Deps<NeutronQuery>,
    env: Env,
    address: String,
    start_from: u32,
    limit: u32,
) -> StdResult<AllUserLockupsResponse> {
    let raw_lockups = query_user_lockups(
        deps,
        deps.api.addr_validate(&address)?,
        |_| true,
        start_from,
        limit,
    );

    let constants = CONSTANTS.load(deps.storage)?;
    let current_round_id = compute_current_round_id(&env, &constants)?;
    let round_end = compute_round_end(&constants, current_round_id)?;

    // enrich the lockups by computing the voting power for each lockup
    let enriched_lockups = raw_lockups
        .iter()
        .map(|lock| {
            to_lockup_with_power(
                deps,
                env.clone(),
                &constants,
                current_round_id,
                round_end,
                lock.clone(),
            )
        })
        .collect();

    Ok(AllUserLockupsResponse {
        lockups: enriched_lockups,
    })
}

pub fn query_expired_user_lockups(
    deps: Deps<NeutronQuery>,
    env: Env,
    address: String,
    start_from: u32,
    limit: u32,
) -> StdResult<ExpiredUserLockupsResponse> {
    let user_address = deps.api.addr_validate(&address)?;
    let expired_lockup_predicate = |l: &LockEntry| l.lock_end < env.block.time;

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
        proposal: PROPOSAL_MAP.load(deps.storage, (round_id, tranche_id, proposal_id))?,
    })
}

pub fn query_user_voting_power(
    deps: Deps<NeutronQuery>,
    env: Env,
    address: String,
) -> StdResult<UserVotingPowerResponse> {
    let user_address = deps.api.addr_validate(&address)?;
    let constants = CONSTANTS.load(deps.storage)?;
    let current_round_id = compute_current_round_id(&env, &constants)?;
    let round_end = compute_round_end(&constants, current_round_id)?;

    let voting_power = LOCKS_MAP
        .prefix(user_address)
        .range(deps.storage, None, None, Order::Ascending)
        .map(|l| l.unwrap().1)
        .filter(|l| l.lock_end > round_end)
        .map(|lockup| {
            to_lockup_with_power(
                deps,
                env.clone(),
                &constants,
                current_round_id,
                round_end,
                lockup,
            )
            .current_voting_power
            .u128()
        })
        .sum();

    Ok(UserVotingPowerResponse { voting_power })
}

pub fn query_user_vote(
    deps: Deps<NeutronQuery>,
    round_id: u64,
    tranche_id: u64,
    user_address: String,
) -> StdResult<UserVoteResponse> {
    let vote = VOTE_MAP.load(
        deps.storage,
        (round_id, tranche_id, deps.api.addr_validate(&user_address)?),
    )?;

    let vote_with_power = VoteWithPower {
        prop_id: vote.prop_id,
        power: vote.time_weighted_shares.values().sum(),
    };

    Ok(UserVoteResponse {
        vote: vote_with_power,
    })
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
    let constants = &CONSTANTS.load(deps.storage)?;
    let round_id = compute_round_id_for_timestamp(constants, env.block.time.nanos())?;

    let round_end = compute_round_end(constants, round_id)?;

    Ok(CurrentRoundResponse {
        round_id,
        round_end,
    })
}

pub fn query_round_end(deps: Deps<NeutronQuery>, round_id: u64) -> StdResult<RoundEndResponse> {
    let constants = &CONSTANTS.load(deps.storage)?;
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
        let prop = PROPOSAL_MAP.load(deps.storage, (round_id, tranche_id, prop_id))?;
        top_props.push(prop);
    }

    // get total voting power for the round
    let total_voting_power = get_total_power_for_round(deps, round_id)?.to_uint_ceil(); // TODO: decide on rounding

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
    deps: Deps<NeutronQuery>,
    user_address: Addr,
    predicate: impl FnMut(&LockEntry) -> bool,
    start_from: u32,
    limit: u32,
) -> Vec<LockEntry> {
    LOCKS_MAP
        .prefix(user_address)
        .range(deps.storage, None, None, Order::Ascending)
        .map(|l| l.unwrap().1)
        .filter(predicate)
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
                    .debug(&format!("Error when querying validator query id: {:?}", l));
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

// Returns the power ratio of a validator for a given round
// This will return an error if there is an error parsing the store,
// but will return 0 if there is no power ratio for the given validator and the round.
pub fn query_validator_power_ratio(
    deps: Deps<NeutronQuery>,
    validator: String,
    round_id: u64,
) -> StdResult<ValidatorPowerRatioResponse> {
    get_validator_power_ratio_for_round(deps.storage, round_id, validator)
        .map(|r| ValidatorPowerRatioResponse { ratio: r }) // error can stay untouched
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
fn update_voting_power_on_proposals(
    deps: &mut DepsMut<NeutronQuery>,
    sender: &Addr,
    constants: &Constants,
    current_round: u64,
    old_lock_entry: Option<LockEntry>,
    new_lock_entry: LockEntry,
    validator: String,
) -> Result<(), ContractError> {
    let round_end = compute_round_end(constants, current_round)?;
    let lock_epoch_length = constants.lock_epoch_length;

    let old_scaled_shares = match old_lock_entry {
        None => Uint128::zero(),
        Some(lock_entry) => {
            get_lock_time_weighted_shares(round_end, lock_entry.clone(), lock_epoch_length)
        }
    };
    let new_scaled_shares =
        get_lock_time_weighted_shares(round_end, new_lock_entry, lock_epoch_length);

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
        if let Some(mut vote) =
            VOTE_MAP.may_load(deps.storage, (current_round, tranche_id, sender.clone()))?
        {
            let current_vote_shares = match vote.time_weighted_shares.get(&validator) {
                None => Decimal::zero(),
                Some(shares) => *shares,
            };

            let new_vote_shares = if power_change.is_increased {
                current_vote_shares.checked_add(power_change.scaled_power_change)?
            } else {
                current_vote_shares.checked_sub(power_change.scaled_power_change)?
            };

            vote.time_weighted_shares
                .insert(validator.clone(), new_vote_shares);

            VOTE_MAP.save(
                deps.storage,
                (current_round, tranche_id, sender.clone()),
                &vote,
            )?;

            if power_change.is_increased {
                add_validator_shares_to_proposal(
                    deps.storage,
                    current_round,
                    vote.prop_id,
                    validator.clone(),
                    power_change.scaled_power_change,
                )?;
            } else {
                remove_validator_shares_from_proposal(
                    deps.storage,
                    current_round,
                    vote.prop_id,
                    validator.clone(),
                    power_change.scaled_power_change,
                )?;
            }

            update_proposal_and_props_by_score_maps(
                deps.storage,
                current_round,
                tranche_id,
                vote.prop_id,
            )?;
        }
    }

    Ok(())
}

fn update_proposal_and_props_by_score_maps(
    storage: &mut dyn Storage,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<(), ContractError> {
    // Load the proposal that needs to be updated
    let mut proposal = PROPOSAL_MAP.load(storage, (round_id, tranche_id, proposal_id))?;

    // Delete the proposal's old power in PROPS_BY_SCORE
    PROPS_BY_SCORE.remove(
        storage,
        ((round_id, tranche_id), proposal.power.into(), proposal_id),
    );

    // Get the new total power of the proposal
    let total_power = get_total_power_for_proposal(storage, proposal_id)?;

    // Save the new power into the proposal
    proposal.power = total_power.to_uint_ceil(); // TODO: decide whether we need to round or represent as decimals

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
    constants: &Constants,
    start_round_id: u64,
    end_round_id: u64,
    lock_end: u64,
    shares_validator: String,
    amount: Uint128,
    get_old_voting_power: T,
) -> StdResult<()>
where
    T: Fn(u64, Timestamp, Uint128) -> Uint128,
{
    for round in start_round_id..=end_round_id {
        let round_end = compute_round_end(constants, round)?;
        let lockup_length = lock_end - round_end.nanos();
        let scaled_amount = scale_lockup_power(constants.lock_epoch_length, lockup_length, amount);
        let old_voting_power = get_old_voting_power(round, round_end, amount);
        let scaled_shares = Decimal::from_ratio(scaled_amount, Uint128::one())
            - Decimal::from_ratio(old_voting_power, Uint128::one());

        // save some gas if there was no power change
        if scaled_shares.is_zero() {
            continue;
        }

        // add the shares to the total power in the round
        add_validator_shares_to_round_total(
            deps.storage,
            round,
            shares_validator.clone(),
            scaled_shares,
        )?;
    }

    Ok(())
}

// Returns the number of locks for a given user
fn get_lock_count(deps: Deps<NeutronQuery>, user_address: Addr) -> usize {
    LOCKS_MAP
        .prefix(user_address)
        .range(deps.storage, None, None, Order::Ascending)
        .count()
}

fn to_lockup_with_power(
    deps: Deps<NeutronQuery>,
    env: Env,
    constants: &Constants,
    round_id: u64,
    round_end: Timestamp,
    lock_entry: LockEntry,
) -> LockEntryWithPower {
    match validate_denom(deps, env.clone(), constants, lock_entry.funds.denom.clone()) {
        Err(_) => {
            // If we fail to resove the denom, or the validator has dropped
            // from the top N, then this lockup has zero voting power.
            LockEntryWithPower {
                lock_entry,
                current_voting_power: Uint128::zero(),
            }
        }
        Ok(validator) => {
            match get_validator_power_ratio_for_round(deps.storage, round_id, validator) {
                Err(_) => {
                    deps.api.debug(&format!(
                        "An error occured while computing voting power for lock: {:?}",
                        lock_entry,
                    ));

                    LockEntryWithPower {
                        lock_entry,
                        current_voting_power: Uint128::zero(),
                    }
                }
                Ok(validator_power_ratio) => {
                    let time_weighted_shares = get_lock_time_weighted_shares(
                        round_end,
                        lock_entry.clone(),
                        constants.lock_epoch_length,
                    );

                    let current_voting_power = validator_power_ratio
                        .checked_mul(Decimal::from_ratio(time_weighted_shares, Uint128::one()));

                    match current_voting_power {
                        Err(_) => {
                            // if there was an overflow error, log this but return 0
                            deps.api.debug(&format!(
                                "Overflow error when computing voting power for lock: {:?}",
                                lock_entry
                            ));

                            LockEntryWithPower {
                                lock_entry: lock_entry.clone(),
                                current_voting_power: Uint128::zero(),
                            }
                        }
                        Ok(current_voting_power) => LockEntryWithPower {
                            lock_entry,
                            current_voting_power: current_voting_power.to_uint_ceil(),
                        },
                    }
                }
            }
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    msg: Reply,
) -> Result<Response<NeutronMsg>, ContractError> {
    handle_submsg_reply(deps, msg)
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
