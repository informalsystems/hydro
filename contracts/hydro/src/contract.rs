use std::collections::HashMap;

use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Decimal, Deps, DepsMut, Env,
    MessageInfo, Order, Response, StdError, StdResult, Timestamp, Uint128,
};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::lsm_integration::{get_validator_from_denom, validate_denom};
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, TrancheInfo};
use crate::query::{
    AllUserLockupsResponse, ConstantsResponse, CurrentRoundResponse, ExpiredUserLockupsResponse,
    ProposalResponse, QueryMsg, RoundEndResponse, RoundProposalsResponse,
    RoundTotalVotingPowerResponse, TopNProposalsResponse, TotalLockedTokensResponse,
    TranchesResponse, UserVoteResponse, UserVotingPowerResponse, WhitelistAdminsResponse,
    WhitelistResponse,
};
use crate::score_keeper::{
    add_validator_shares_to_proposal, add_validator_shares_to_round_total, get_total_power,
    remove_validator_shares, remove_validator_shares_from_proposal,
};
use crate::state::{
    Constants, LockEntry, Proposal, Tranche, Vote, VoteWithPower, CONSTANTS, LOCKED_TOKENS,
    LOCKS_MAP, LOCK_ID, PROPOSAL_MAP, PROPS_BY_SCORE, PROP_ID, TRANCHE_ID, TRANCHE_MAP, VOTE_MAP,
    WHITELIST, WHITELIST_ADMINS,
};

use crate::score_keeper_state::{get_prop_power_key, get_total_round_power_key};

/// Contract name that is used for migration.
const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const MAX_LOCK_ENTRIES: usize = 100;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // validate that the first round starts in the future
    if msg.first_round_start < env.block.time {
        return Err(ContractError::Std(StdError::generic_err(
            "First round start time must be in the future",
        )));
    }

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
        max_locked_tokens: msg.max_locked_tokens,
        paused: false,
        max_validator_shares_participating: msg.max_validator_shares_participating,
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

    Ok(Response::new()
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender.clone()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::LockTokens { lock_duration } => lock_tokens(deps, env, info, lock_duration),
        ExecuteMsg::RefreshLockDuration {
            lock_id,
            lock_duration,
        } => refresh_lock_duration(deps, env, info, lock_id, lock_duration),
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
    }
}

// LockTokens(lock_duration):
//     Receive tokens
//     Validate against the accepted denom
//     Create entry in LocksMap
fn lock_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    lock_duration: u64,
) -> Result<Response, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
    validate_lock_duration(constants.lock_epoch_length, lock_duration)?;

    if info.funds.len() != 1 {
        return Err(ContractError::Std(StdError::generic_err(
            "Must provide exactly one coin to lock",
        )));
    }

    let funds = info.funds[0].clone();

    let validator = validate_denom(deps.as_ref(), env.clone(), funds.denom).map_err(|err| {
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

    let lock_entry = LockEntry {
        funds: info.funds[0].clone(),
        lock_start: env.block.time,
        lock_end: env.block.time.plus_nanos(lock_duration),
    };
    let lock_end = lock_entry.lock_end.nanos();

    let lock_id = LOCK_ID.load(deps.storage)?;
    LOCK_ID.save(deps.storage, &(lock_id + 1))?;
    LOCKS_MAP.save(deps.storage, (info.sender, lock_id), &lock_entry)?;
    LOCKED_TOKENS.save(deps.storage, &(locked_tokens + amount_to_lock))?;

    // Calculate and update the total voting power info for current and all
    // future rounds in which the user will have voting power greather than 0
    let current_round = compute_current_round_id(&env, &constants)?;
    let last_round_with_power = compute_round_id_for_timestamp(&constants, lock_end)? - 1;

    update_total_time_weighted_shares(
        deps,
        &constants,
        current_round,
        last_round_with_power,
        lock_end,
        validator,
        lock_entry.funds.amount,
        |_, _, _| Uint128::zero(),
    )?;

    Ok(Response::new().add_attribute("action", "lock_tokens"))
}

// Extends the lock duration of a lock entry to be current_block_time + lock_duration,
// assuming that this would actually increase the lock_end_time (so this *should not* be a way to make the lock time shorter).
// Thus, the lock_end_time afterwards *must* be later than the lock_end_time before.
// This should essentially have the same effect as removing the old lock and immediately re-locking all
// the same funds for the new lock duration.
fn refresh_lock_duration(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    lock_id: u64,
    lock_duration: u64,
) -> Result<Response, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
    validate_lock_duration(constants.lock_epoch_length, lock_duration)?;

    // try to get the lock with the given id
    // note that this is already indexed by the caller, so if it is successful, the sender owns this lock
    let mut lock_entry = LOCKS_MAP.load(deps.storage, (info.sender.clone(), lock_id))?;

    // log the lock entry
    deps.api.debug(&format!("lock_entry: {:?}", lock_entry));

    // compute the new lock_end_time
    let new_lock_end = env.block.time.plus_nanos(lock_duration).nanos();

    let old_lock_end = lock_entry.lock_end.nanos();

    // check that the new lock_end_time is later than the old lock_end_time
    if new_lock_end <= old_lock_end {
        return Err(ContractError::Std(StdError::generic_err(
            "Shortening locks is not allowed, new lock end time must be after the old lock end",
        )));
    }

    // update the lock entry with the new lock_end_time
    lock_entry.lock_end = Timestamp::from_nanos(new_lock_end);

    // save the updated lock entry
    LOCKS_MAP.save(deps.storage, (info.sender, lock_id), &lock_entry)?;

    // get the validator whose shares are in this lock
    let validator = get_validator_from_denom(lock_entry.funds.denom)?;

    // Calculate and update the total voting power info for current and all
    // future rounds in which the user will have voting power greather than 0.
    // The voting power originated from the old lockup is subtracted from the
    // total voting power, and the voting power gained with the new lockup is
    // added to the total voting power for each applicable round.
    let current_round = compute_current_round_id(&env, &constants)?;
    let old_last_round_with_power = compute_round_id_for_timestamp(&constants, old_lock_end)? - 1;
    let new_last_round_with_power = compute_round_id_for_timestamp(&constants, new_lock_end)? - 1;

    update_total_time_weighted_shares(
        deps,
        &constants,
        current_round,
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

    Ok(Response::new().add_attribute("action", "refresh_lock_duration"))
}

// Validate that the lock duration (given in nanos) is either 1, 3, 6, or 12 epochs
fn validate_lock_duration(lock_epoch_length: u64, lock_duration: u64) -> Result<(), ContractError> {
    if lock_duration != lock_epoch_length
        && lock_duration != lock_epoch_length * 3
        && lock_duration != lock_epoch_length * 6
        && lock_duration != lock_epoch_length * 12
    {
        return Err(ContractError::Std(StdError::generic_err(
            "Lock duration must be 1, 3, 6, or 12 epochs",
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
fn unlock_tokens(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
    validate_previous_round_vote(&deps, &env, info.sender.clone())?;

    // Iterate all locks for the caller and unlock them if lock_end < now
    let locks =
        LOCKS_MAP
            .prefix(info.sender.clone())
            .range(deps.storage, None, None, Order::Ascending);

    let mut to_delete = vec![];
    let mut total_unlocked_amount = Uint128::zero();

    let mut response = Response::new().add_attribute("action", "unlock_tokens");

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

    Ok(response)
}

fn validate_previous_round_vote(
    deps: &DepsMut,
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
// It will also instantiate the total voted power for this round and tranche if it does not exist.
fn create_proposal(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    tranche_id: u64,
    title: String,
    description: String,
) -> Result<Response, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;
    validate_contract_is_not_paused(&constants)?;

    // validate that the sender is on the whitelist
    let whitelist = WHITELIST.load(deps.storage)?;

    if !whitelist.contains(&info.sender) {
        return Err(ContractError::Unauthorized);
    }

    // check that the tranche with the given id exists
    TRANCHE_MAP.load(deps.storage, tranche_id)?;

    let round_id = compute_current_round_id(&env, &constants)?;
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

    Ok(Response::new().add_attribute("action", "create_proposal"))
}

fn scale_lockup_power(lock_epoch_length: u64, lockup_time: u64, raw_power: Uint128) -> Uint128 {
    let two: Uint128 = 2u16.into();

    // Scale lockup power
    // 1x if lockup is between 0 and 1 epochs
    // 1.5x if lockup is between 1 and 3 epochs
    // 2x if lockup is between 3 and 6 epochs
    // 4x if lockup is between 6 and 12 epochs
    // TODO: is there a less funky way to do Uint128 math???
    match lockup_time {
        // 4x if lockup is over 6 epochs
        _ if lockup_time > lock_epoch_length * 6 => raw_power * two * two,
        // 2x if lockup is between 3 and 6 epochs
        _ if lockup_time > lock_epoch_length * 3 => raw_power * two,
        // 1.5x if lockup is between 1 and 3 epochs
        _ if lockup_time > lock_epoch_length => raw_power + (raw_power / two),
        // Covers 0 and 1 epoch which have no scaling
        _ => raw_power,
    }
}

fn vote(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<Response, ContractError> {
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

    // check that the tranche with the given id exists
    TRANCHE_MAP.load(deps.storage, tranche_id)?;

    // compute the round_id
    let round_id = compute_current_round_id(&env, &constants)?;

    // compute the round end
    let round_end = compute_round_end(&constants, round_id)?;

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

        // get key for score keeper store of this proposal
        let prop_power_key = get_prop_power_key(proposal.proposal_id);

        // gro through all the shares that were voted with and subtract them from the proposal's power and the total power that voted
        // TODO: we need to limit the number of different share types that users can lock; is the existing lock limit good enough?
        // TODO: do we need to make sure we don't iterate over validators outside of the set here? it seems ok to me, but should double-check
        for (validator, num_shares) in vote.time_weighted_shares.iter() {
            // TODO: make more efficient by writing only a single time to the store

            // remove the validator shares from the previous proposal
            remove_validator_shares_from_proposal(
                deps.storage,
                round_id,
                vote.prop_id,
                validator.to_string(),
                *num_shares,
            )?;
        }

        // save the new power into the proposal
        let total_power = get_total_power(deps.storage, &prop_power_key.as_str())?;
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

        // shares from locks that expire before the current round ends don't need to be counted
        if round_end.nanos() > lock_entry.lock_end.nanos() {
            continue;
        }

        // Get the remaining lockup length at the end of this round.
        // This means that their power will be scaled the same by this function no matter when they vote in the round
        let lockup_length = lock_entry.lock_end.nanos() - round_end.nanos();

        // Scale the number of shares. This is what implements the different powers for different lockup times.
        let scaled_shares =
            scale_lockup_power(lock_epoch_length, lockup_length, lock_entry.funds.amount);

        // get the validator from the denom
        let validator = get_validator_from_denom(lock_entry.funds.denom)?;

        // add the shares to the map
        let shares = time_weighted_shares_map.get(&validator);
        let shares = match shares {
            Some(shares) => shares,
            None => &Decimal::zero(),
        };
        let new_shares = shares.checked_add(Decimal::from_ratio(scaled_shares, Uint128::one()))?;

        // insert the shares into the time_weigted_shares_map
        time_weighted_shares_map.insert(validator.clone(), new_shares);
    }

    let response = Response::new().add_attribute("action", "vote");

    // if the user doesn't have any shares that give voting power, we don't need to do anything
    if time_weighted_shares_map.is_empty() {
        return Ok(response);
    }

    // Load the proposal being voted on
    let mut proposal = PROPOSAL_MAP.load(deps.storage, (round_id, tranche_id, proposal_id))?;

    // Delete the proposal's old power in PROPS_BY_SCORE
    PROPS_BY_SCORE.remove(
        deps.storage,
        ((round_id, tranche_id), proposal.power.into(), proposal_id),
    );

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

    // get the new total power of the proposal
    let prop_power_key = get_prop_power_key(proposal.proposal_id);
    let total_power = get_total_power(deps.storage, &prop_power_key.as_str())?;

    // save the new power into the proposal
    proposal.power = total_power.to_uint_ceil(); // TODO: decide whether we need to round or represent as decimals

    // Save the proposal
    PROPOSAL_MAP.save(deps.storage, (round_id, tranche_id, proposal_id), &proposal)?;

    // Save the proposal's new power in PROPS_BY_SCORE
    PROPS_BY_SCORE.save(
        deps.storage,
        ((round_id, tranche_id), proposal.power.into(), proposal_id),
        &proposal_id,
    )?;

    // Create vote in Votemap
    let vote = Vote {
        prop_id: proposal_id,
        time_weighted_shares: time_weighted_shares_map,
    };
    VOTE_MAP.save(deps.storage, (round_id, tranche_id, info.sender), &vote)?;

    Ok(response)
}

// Adds a new account address to the whitelist.
fn add_to_whitelist(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
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

    Ok(Response::new().add_attribute("action", "add_to_whitelist"))
}

// Removes an account address from the whitelist.
fn remove_from_whitelist(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    let constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
    validate_sender_is_whitelist_admin(&deps, &info)?;

    // Remove the account address from the whitelist
    let mut whitelist = WHITELIST.load(deps.storage)?;

    let whitelist_account_addr = deps.api.addr_validate(&address)?;

    whitelist.retain(|cp| cp != whitelist_account_addr);
    WHITELIST.save(deps.storage, &whitelist)?;

    Ok(Response::new().add_attribute("action", "remove_from_whitelist"))
}

fn update_max_locked_tokens(
    deps: DepsMut,
    info: MessageInfo,
    max_locked_tokens: u128,
) -> Result<Response, ContractError> {
    let mut constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
    validate_sender_is_whitelist_admin(&deps, &info)?;

    constants.max_locked_tokens = max_locked_tokens;
    CONSTANTS.save(deps.storage, &constants)?;

    Ok(Response::new()
        .add_attribute("action", "update_max_locked_tokens")
        .add_attribute("sender", info.sender.clone())
        .add_attribute("max_locked_tokens", max_locked_tokens.to_string()))
}

// Pause:
//     Validate that the contract isn't already paused
//     Validate sender is whitelist admin
//     Set paused to true and save the changes
fn pause_contract(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    let mut constants = CONSTANTS.load(deps.storage)?;

    validate_contract_is_not_paused(&constants)?;
    validate_sender_is_whitelist_admin(&deps, &info)?;

    constants.paused = true;
    CONSTANTS.save(deps.storage, &constants)?;

    Ok(Response::new()
        .add_attribute("action", "pause_contract")
        .add_attribute("sender", info.sender.clone())
        .add_attribute("paused", "true"))
}

// AddTranche:
//     Validate that the contract isn't paused
//     Validate sender is whitelist admin
//     Validate that the tranche with the same name doesn't already exist
//     Add new tranche to the store
fn add_tranche(
    deps: DepsMut,
    info: MessageInfo,
    tranche: TrancheInfo,
) -> Result<Response, ContractError> {
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
        .add_attribute("sender", info.sender.clone())
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
    deps: DepsMut,
    info: MessageInfo,
    tranche_id: u64,
    tranche_name: Option<String>,
    tranche_metadata: Option<String>,
) -> Result<Response, ContractError> {
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
        .add_attribute("sender", info.sender.clone())
        .add_attribute("tranche id", tranche.id.to_string())
        .add_attribute("old tranche name", old_tranche_name)
        .add_attribute("old tranche metadata", old_tranche_metadata)
        .add_attribute("new tranche name", tranche.name)
        .add_attribute("new tranche metadata", tranche.metadata))
}

fn validate_sender_is_whitelist_admin(
    deps: &DepsMut,
    info: &MessageInfo,
) -> Result<(), ContractError> {
    let whitelist_admins = WHITELIST_ADMINS.load(deps.storage)?;
    if !whitelist_admins.contains(&info.sender) {
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
    deps: &DepsMut,
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
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Constants {} => to_json_binary(&query_constants(deps)?),
        QueryMsg::Tranches {} => to_json_binary(&query_tranches(deps)?),
        QueryMsg::AllUserLockups {
            address,
            start_from,
            limit,
        } => to_json_binary(&query_all_user_lockups(deps, address, start_from, limit)?),
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
    }
}

pub fn query_round_total_power(
    deps: Deps,
    round_id: u64,
) -> StdResult<RoundTotalVotingPowerResponse> {
    let total_round_power_key = get_total_round_power_key(round_id);
    let total_round_power = get_total_power(deps.storage, total_round_power_key.as_str())?;
    Ok(RoundTotalVotingPowerResponse {
        total_voting_power: total_round_power.to_uint_ceil(), // TODO: decide on rounding
    })
}

pub fn query_constants(deps: Deps) -> StdResult<ConstantsResponse> {
    Ok(ConstantsResponse {
        constants: CONSTANTS.load(deps.storage)?,
    })
}

pub fn query_all_user_lockups(
    deps: Deps,
    address: String,
    start_from: u32,
    limit: u32,
) -> StdResult<AllUserLockupsResponse> {
    Ok(AllUserLockupsResponse {
        lockups: query_user_lockups(
            deps,
            deps.api.addr_validate(&address)?,
            |_| true,
            start_from,
            limit,
        ),
    })
}

pub fn query_expired_user_lockups(
    deps: Deps,
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
    deps: Deps,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> StdResult<ProposalResponse> {
    Ok(ProposalResponse {
        proposal: PROPOSAL_MAP.load(deps.storage, (round_id, tranche_id, proposal_id))?,
    })
}

pub fn query_user_voting_power(
    deps: Deps,
    env: Env,
    address: String,
) -> StdResult<UserVotingPowerResponse> {
    let user_address = deps.api.addr_validate(&address)?;
    let constants = CONSTANTS.load(deps.storage)?;
    let current_round_id = compute_current_round_id(&env, &constants)?;
    let round_end = compute_round_end(&constants, current_round_id)?;
    let lock_epoch_length = constants.lock_epoch_length;

    let voting_power = LOCKS_MAP
        .prefix(user_address)
        .range(deps.storage, None, None, Order::Ascending)
        .map(|l| l.unwrap().1)
        .filter(|l| l.lock_end > round_end)
        .map(|lockup| {
            let lockup_length = lockup.lock_end.nanos() - round_end.nanos();
            scale_lockup_power(lock_epoch_length, lockup_length, lockup.funds.amount).u128()
        })
        .sum();

    Ok(UserVotingPowerResponse { voting_power })
}

pub fn query_user_vote(
    deps: Deps,
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
    deps: Deps,
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

pub fn query_current_round_id(deps: Deps, env: Env) -> StdResult<CurrentRoundResponse> {
    let constants = &CONSTANTS.load(deps.storage)?;
    let round_id = compute_round_id_for_timestamp(constants, env.block.time.nanos())?;

    Ok(CurrentRoundResponse { round_id })
}

pub fn query_round_end(deps: Deps, round_id: u64) -> StdResult<RoundEndResponse> {
    let constants = &CONSTANTS.load(deps.storage)?;
    let round_end = compute_round_end(constants, round_id)?;

    Ok(RoundEndResponse { round_end })
}

pub fn query_top_n_proposals(
    deps: Deps,
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
    let total_voting_power_key = get_total_round_power_key(round_id);
    let total_voting_power = get_total_power(deps.storage, &total_voting_power_key)?.to_uint_ceil(); // TODO: decide on rounding

    let top_proposals = top_props
        .into_iter()
        .map(|mut prop| {
            prop.percentage = (prop.power * Uint128::from(100u128)) / total_voting_power;
            prop
        })
        .collect();

    // return top props
    Ok(TopNProposalsResponse {
        proposals: top_proposals,
    })
}

pub fn query_tranches(deps: Deps) -> StdResult<TranchesResponse> {
    let tranches = TRANCHE_MAP
        .range(deps.storage, None, None, Order::Ascending)
        .map(|t| t.unwrap().1)
        .collect();

    Ok(TranchesResponse { tranches })
}

fn query_user_lockups(
    deps: Deps,
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

pub fn query_whitelist(deps: Deps) -> StdResult<WhitelistResponse> {
    Ok(WhitelistResponse {
        whitelist: WHITELIST.load(deps.storage)?,
    })
}

pub fn query_whitelist_admins(deps: Deps) -> StdResult<WhitelistAdminsResponse> {
    Ok(WhitelistAdminsResponse {
        admins: WHITELIST_ADMINS.load(deps.storage)?,
    })
}

pub fn query_total_locked_tokens(deps: Deps) -> StdResult<TotalLockedTokensResponse> {
    Ok(TotalLockedTokensResponse {
        total_locked_tokens: LOCKED_TOKENS.load(deps.storage)?,
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

fn compute_round_end(constants: &Constants, round_id: u64) -> StdResult<Timestamp> {
    let round_end = constants
        .first_round_start
        .plus_nanos(constants.round_length * (round_id + 1));

    Ok(round_end)
}

fn update_total_time_weighted_shares<T>(
    deps: DepsMut,
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
fn get_lock_count(deps: Deps, user_address: Addr) -> usize {
    LOCKS_MAP
        .prefix(user_address)
        .range(deps.storage, None, None, Order::Ascending)
        .count()
}

/// In the first version of Hydro, we allow contract to be un-paused through the Cosmos Hub governance
/// by migrating contract to the same code ID. This will trigger the migrate() function where we set
/// the paused flag to false.
/// Keep in mind that, for the future versions, this function should check the `CONTRACT_VERSION` and
/// perform any state changes needed. It should also handle the un-pausing of the contract, depending if
/// it was previously paused or not.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    CONSTANTS.update(
        deps.storage,
        |mut constants| -> Result<Constants, ContractError> {
            constants.paused = false;
            Ok(constants)
        },
    )?;

    Ok(Response::default())
}
