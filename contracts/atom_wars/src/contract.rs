// MAIN TODOS:
// - Add real covenant logic
// - Question: How to handle the case where a proposal is executed but the covenant fails?
// - Covenant Question: How to deal with someone using MEV to skew the pool ratio right before the liquidity is pulled? Streaming the liquidity pull? You'd have to set up a cron job for that.
// - Covenant Question: Can people sandwich this whole thing - covenant system has price limits - but we should allow people to retry executing the prop during the round

use std::convert::TryInto;

use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo,
    Order, Response, StdError, StdResult, Timestamp, Uint128,
};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg};
use crate::query::{QueryMsg, RoundProposalsResponse, UserLockupsResponse};
use crate::state::{
    Constants, CovenantParams, LockEntry, Proposal, Tranche, Vote, CONSTANTS, LOCKS_MAP, LOCK_ID,
    PROPOSAL_MAP, PROPS_BY_SCORE, PROP_ID, TOTAL_ROUND_POWER, TOTAL_VOTED_POWER, TRANCHE_MAP,
    VOTE_MAP, WHITELIST, WHITELIST_ADMINS,
};

/// Contract name that is used for migration.
const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const MAX_LOCK_ENTRIES: usize = 100;

#[entry_point]
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
        denom: msg.denom.clone(),
        round_length: msg.round_length,
        lock_epoch_length: msg.lock_epoch_length,
        first_round_start: msg.first_round_start,
    };

    CONSTANTS.save(deps.storage, &state)?;
    LOCK_ID.save(deps.storage, &0)?;
    PROP_ID.save(deps.storage, &0)?;

    let mut whitelist_admins: Vec<Addr> = vec![];
    let mut whitelist: Vec<CovenantParams> = vec![];
    for admin in msg.whitelist_admins {
        let admin_addr = deps.api.addr_validate(&admin)?;
        if !whitelist_admins.contains(&admin_addr) {
            whitelist_admins.push(admin_addr.clone());
        }
    }
    for covenant in msg.initial_whitelist {
        if !whitelist.contains(&covenant) {
            whitelist.push(covenant.clone());
        }
    }
    WHITELIST_ADMINS.save(deps.storage, &whitelist_admins)?;
    WHITELIST.save(deps.storage, &whitelist)?;

    // For each tranche, create a tranche in the TRANCHE_MAP and set the total power to 0
    let mut tranche_ids = std::collections::HashSet::new();

    for tranche in msg.tranches {
        if !tranche_ids.insert(tranche.tranche_id) {
            return Err(ContractError::Std(StdError::generic_err(
                "Duplicate tranche ID found in provided tranches, but tranche IDs must be unique",
            )));
        }
        TRANCHE_MAP.save(deps.storage, tranche.tranche_id, &tranche)?;
    }

    Ok(Response::new()
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender.clone())
        .add_attribute("denom", msg.denom))
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
            covenant_params,
        } => create_proposal(deps, env, tranche_id, covenant_params),
        ExecuteMsg::Vote {
            tranche_id,
            proposal_id,
        } => vote(deps, env, info, tranche_id, proposal_id),
        ExecuteMsg::AddToWhitelist { covenant_params } => {
            add_to_whitelist(deps, env, info, covenant_params)
        }
        ExecuteMsg::RemoveFromWhitelist { covenant_params } => {
            remove_from_whitelist(deps, env, info, covenant_params)
        }
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
    validate_lock_duration(constants.lock_epoch_length, lock_duration)?;

    // Validate that sent funds are the required denom
    if info.funds.len() != 1 {
        return Err(ContractError::Std(StdError::generic_err(
            "Must send exactly one coin",
        )));
    }

    let sent_funds = info.funds[0].clone();
    if sent_funds.denom != constants.denom {
        return Err(ContractError::Std(StdError::generic_err(
            "Must send the correct denom",
        )));
    }

    // validate that the user does not have too many locks
    if get_lock_count(deps.as_ref(), info.sender.clone()) >= MAX_LOCK_ENTRIES.try_into().unwrap() {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "User has too many locks, only {} locks allowed",
            MAX_LOCK_ENTRIES
        ))));
    }

    // Create entry in LocksMap
    let lock_entry = LockEntry {
        funds: sent_funds,
        lock_start: env.block.time,
        lock_end: env.block.time.plus_nanos(lock_duration),
    };
    let lock_end = lock_entry.lock_end.nanos();

    let lock_id = LOCK_ID.load(deps.storage)?;
    LOCK_ID.save(deps.storage, &(lock_id + 1))?;
    LOCKS_MAP.save(deps.storage, (info.sender, lock_id), &lock_entry)?;

    // Calculate and update the total voting power info for current and all
    // future rounds in which the user will have voting power greather than 0
    let current_round = compute_current_round_id(&env, &constants)?;
    let last_round_with_power = compute_round_id_for_timestamp(&constants, lock_end)? - 1;

    update_total_voting_power(
        deps,
        &constants,
        current_round,
        last_round_with_power,
        lock_end,
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

    // Calculate and update the total voting power info for current and all
    // future rounds in which the user will have voting power greather than 0.
    // The voting power originated from the old lockup is subtracted from the
    // total voting power, and the voting power gained with the new lockup is
    // added to the total voting power for each applicable round.
    let current_round = compute_current_round_id(&env, &constants)?;
    let old_last_round_with_power = compute_round_id_for_timestamp(&constants, old_lock_end)? - 1;
    let new_last_round_with_power = compute_round_id_for_timestamp(&constants, new_lock_end)? - 1;

    update_total_voting_power(
        deps,
        &constants,
        current_round,
        new_last_round_with_power,
        new_lock_end,
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
    validate_previous_round_vote(&deps, &env, info.sender.clone())?;

    // Iterate all locks for the caller and unlock them if lock_end < now
    let locks =
        LOCKS_MAP
            .prefix(info.sender.clone())
            .range(deps.storage, None, None, Order::Ascending);

    let mut send = Coin::new(0, CONSTANTS.load(deps.storage)?.denom);
    let mut to_delete = vec![];

    for lock in locks {
        let (lock_id, lock_entry) = lock?;
        if lock_entry.lock_end < env.block.time {
            // Send tokens back to caller
            send.amount = send.amount.checked_add(lock_entry.funds.amount)?;

            // Delete entry from LocksMap
            to_delete.push((info.sender.clone(), lock_id));
        }
    }

    // Delete unlocked locks
    for (addr, lock_id) in to_delete {
        LOCKS_MAP.remove(deps.storage, (addr, lock_id));
    }

    let mut response = Response::new().add_attribute("action", "unlock_tokens");

    if !send.amount.is_zero() {
        response = response.add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![send],
        })
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

// CreateProposal(covenant_params, tribute):
//     Validate covenant_params
//     Hold tribute in contract's account
//     Create in PropMap
fn create_proposal(
    deps: DepsMut,
    env: Env,
    tranche_id: u64,
    covenant_params: CovenantParams,
) -> Result<Response, ContractError> {
    TRANCHE_MAP.load(deps.storage, tranche_id)?;

    let constants = CONSTANTS.load(deps.storage)?;
    let round_id = compute_current_round_id(&env, &constants)?;
    let proposal_id = PROP_ID.load(deps.storage)?;

    // Create proposal in PropMap
    let proposal = Proposal {
        round_id,
        tranche_id,
        proposal_id,
        covenant_params,
        power: Uint128::zero(),
        percentage: Uint128::zero(),
    };

    PROP_ID.save(deps.storage, &(proposal_id + 1))?;
    PROPOSAL_MAP.save(deps.storage, (round_id, tranche_id, proposal_id), &proposal)?;

    // load the total voted power for this round and tranche
    let total_voted_power = TOTAL_VOTED_POWER.load(deps.storage, (round_id, tranche_id));

    // if there is no total voted power for this round and tranche, set it to 0
    if total_voted_power.is_err() {
        TOTAL_VOTED_POWER.save(deps.storage, (round_id, tranche_id), &Uint128::zero())?;
    }

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
        lockup_time if lockup_time > lock_epoch_length * 6 => raw_power * two * two,
        // 2x if lockup is between 3 and 6 epochs
        lockup_time if lockup_time > lock_epoch_length * 3 => raw_power * two,
        // 1.5x if lockup is between 1 and 3 epochs
        lockup_time if lockup_time > lock_epoch_length => raw_power + (raw_power / two),
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
    TRANCHE_MAP.load(deps.storage, tranche_id)?;

    let constants = CONSTANTS.load(deps.storage)?;

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

        // Decrement proposal's power
        proposal.power -= vote.power;

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

        // Decrement total voted power
        let total_voted_power = TOTAL_VOTED_POWER.load(deps.storage, (round_id, tranche_id))?;
        TOTAL_VOTED_POWER.save(
            deps.storage,
            (round_id, tranche_id),
            &(total_voted_power - vote.power),
        )?;

        // Delete vote
        VOTE_MAP.remove(deps.storage, (round_id, tranche_id, info.sender.clone()));
    }

    let lock_epoch_length = CONSTANTS.load(deps.storage)?.lock_epoch_length;
    // Get sender's total locked power
    let mut power: Uint128 = Uint128::zero();
    let locks =
        LOCKS_MAP
            .prefix(info.sender.clone())
            .range(deps.storage, None, None, Order::Ascending);

    for lock in locks {
        let (_, lock_entry) = lock?;

        // user gets 0 voting power for lockups that expire before the current round ends
        if round_end.nanos() > lock_entry.lock_end.nanos() {
            continue;
        }

        // Get the remaining lockup length at the end of this round.
        // This means that their power will be scaled the same by this function no matter when they vote in the round
        let lockup_length = lock_entry.lock_end.nanos() - round_end.nanos();

        // Scale power. This is what implements the different powers for different lockup times.
        let scaled_power =
            scale_lockup_power(lock_epoch_length, lockup_length, lock_entry.funds.amount);

        power += scaled_power;
    }

    let response = Response::new().add_attribute("action", "vote");

    // if users voting power is 0 we don't need to update any of the stores
    if power == Uint128::zero() {
        return Ok(response);
    }

    // Load the proposal being voted on
    let mut proposal = PROPOSAL_MAP.load(deps.storage, (round_id, tranche_id, proposal_id))?;

    // Delete the proposal's old power in PROPS_BY_SCORE
    PROPS_BY_SCORE.remove(
        deps.storage,
        ((round_id, tranche_id), proposal.power.into(), proposal_id),
    );

    // Update proposal's power
    proposal.power += power;

    // Save the proposal
    PROPOSAL_MAP.save(deps.storage, (round_id, tranche_id, proposal_id), &proposal)?;

    // Save the proposal's new power in PROPS_BY_SCORE
    PROPS_BY_SCORE.save(
        deps.storage,
        ((round_id, tranche_id), proposal.power.into(), proposal_id),
        &proposal_id,
    )?;

    // Increment total voted power
    let total_voted_power = TOTAL_VOTED_POWER.load(deps.storage, (round_id, tranche_id))?;
    TOTAL_VOTED_POWER.save(
        deps.storage,
        (round_id, tranche_id),
        &(total_voted_power + power),
    )?;

    // Create vote in Votemap
    let vote = Vote {
        prop_id: proposal_id,
        power,
    };
    VOTE_MAP.save(deps.storage, (round_id, tranche_id, info.sender), &vote)?;

    Ok(response)
}

fn _do_covenant_stuff(
    _deps: Deps,
    _env: Env,
    _info: MessageInfo,
    _covenant_params: String,
) -> Result<Response, ContractError> {
    // Do covenant stuff
    Ok(Response::new().add_attribute("action", "do_covenant_stuff"))
}

// Adds a new covenant target to the whitelist.
fn add_to_whitelist(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    covenant_params: CovenantParams,
) -> Result<Response, ContractError> {
    // Validate that the sender is a whitelist admin
    let whitelist_admins = WHITELIST_ADMINS.load(deps.storage)?;
    if !whitelist_admins.contains(&info.sender) {
        return Err(ContractError::Unauthorized {});
    }

    // Add covenant_params to whitelist
    let mut whitelist = WHITELIST.load(deps.storage)?;

    // return an error if the covenant_params is already in the whitelist
    if whitelist.contains(&covenant_params) {
        return Err(ContractError::Std(StdError::generic_err(
            "Covenant params already in whitelist",
        )));
    }

    whitelist.push(covenant_params.clone());
    WHITELIST.save(deps.storage, &whitelist)?;

    Ok(Response::new().add_attribute("action", "add_to_whitelist"))
}

fn remove_from_whitelist(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    covenant_params: CovenantParams,
) -> Result<Response, ContractError> {
    // Validate that the sender is a whitelist admin
    let whitelist_admins = WHITELIST_ADMINS.load(deps.storage)?;
    if !whitelist_admins.contains(&info.sender) {
        return Err(ContractError::Unauthorized {});
    }

    // Remove covenant_params from whitelist
    let mut whitelist = WHITELIST.load(deps.storage)?;
    whitelist.retain(|cp| cp != &covenant_params);
    WHITELIST.save(deps.storage, &whitelist)?;

    Ok(Response::new().add_attribute("action", "remove_from_whitelist"))
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
        QueryMsg::CurrentRound {} => to_json_binary(&compute_current_round_id(
            &env,
            &CONSTANTS.load(deps.storage)?,
        )?),
        QueryMsg::RoundEnd { round_id } => to_json_binary(&compute_round_end(
            &CONSTANTS.load(deps.storage)?,
            round_id,
        )?),
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
    }
}

pub fn query_round_total_power(deps: Deps, round_id: u64) -> StdResult<Uint128> {
    TOTAL_ROUND_POWER.load(deps.storage, round_id)
}

pub fn query_constants(deps: Deps) -> StdResult<Constants> {
    CONSTANTS.load(deps.storage)
}

pub fn query_all_user_lockups(
    deps: Deps,
    address: String,
    start_from: u32,
    limit: u32,
) -> StdResult<UserLockupsResponse> {
    Ok(UserLockupsResponse {
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
) -> StdResult<UserLockupsResponse> {
    let user_address = deps.api.addr_validate(&address)?;
    let expired_lockup_predicate = |l: &LockEntry| l.lock_end < env.block.time;

    Ok(UserLockupsResponse {
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
) -> StdResult<Proposal> {
    PROPOSAL_MAP.load(deps.storage, (round_id, tranche_id, proposal_id))
}

pub fn query_user_voting_power(deps: Deps, env: Env, address: String) -> StdResult<u128> {
    let user_address = deps.api.addr_validate(&address)?;
    let constants = CONSTANTS.load(deps.storage)?;
    let current_round_id = compute_current_round_id(&env, &constants)?;
    let round_end = compute_round_end(&constants, current_round_id)?;
    let lock_epoch_length = CONSTANTS.load(deps.storage)?.lock_epoch_length;

    Ok(LOCKS_MAP
        .prefix(user_address)
        .range(deps.storage, None, None, Order::Ascending)
        .map(|l| l.unwrap().1)
        .filter(|l| l.lock_end > round_end)
        .map(|lockup| {
            let lockup_length = lockup.lock_end.nanos() - round_end.nanos();
            scale_lockup_power(lock_epoch_length, lockup_length, lockup.funds.amount).u128()
        })
        .sum())
}

pub fn query_user_vote(
    deps: Deps,
    round_id: u64,
    tranche_id: u64,
    user_address: String,
) -> StdResult<Vote> {
    VOTE_MAP.load(
        deps.storage,
        (round_id, tranche_id, deps.api.addr_validate(&user_address)?),
    )
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

pub fn query_top_n_proposals(
    deps: Deps,
    round_id: u64,
    tranche_id: u64,
    num: usize,
) -> StdResult<Vec<Proposal>> {
    if TRANCHE_MAP.load(deps.storage, tranche_id).is_err() {
        return Err(StdError::generic_err("Tranche does not exist"));
    }

    // load the whitelist
    let whitelist = WHITELIST.load(deps.storage)?;

    // Iterate through PROPS_BY_SCORE to find the top num props, while ignoring
    // any props that are not on the whitelist
    let top_prop_ids: Vec<u64> = PROPS_BY_SCORE
        .sub_prefix((round_id, tranche_id))
        .range(deps.storage, None, None, Order::Descending)
        // filter out any props that are not on the whitelist
        .filter(|x| match x {
            Ok((_, prop_id)) => {
                let prop = PROPOSAL_MAP
                    .load(deps.storage, (round_id, tranche_id, *prop_id))
                    .unwrap();
                whitelist.contains(&prop.covenant_params)
            }
            Err(_) => false,
        })
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
    let total_voting_power = TOTAL_ROUND_POWER.load(deps.storage, round_id)?;

    // return top props
    Ok(top_props
        .into_iter()
        .map(|mut prop| {
            prop.percentage = (prop.power * Uint128::from(100u128)) / total_voting_power;
            prop
        })
        .collect())
}

pub fn query_tranches(deps: Deps) -> StdResult<Vec<Tranche>> {
    let tranches = TRANCHE_MAP
        .range(deps.storage, None, None, Order::Ascending)
        .map(|t| t.unwrap().1)
        .collect();

    Ok(tranches)
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

pub fn query_whitelist(deps: Deps) -> StdResult<Vec<CovenantParams>> {
    WHITELIST.load(deps.storage)
}

pub fn query_whitelist_admins(deps: Deps) -> StdResult<Vec<Addr>> {
    WHITELIST_ADMINS.load(deps.storage)
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

fn update_total_voting_power<T>(
    deps: DepsMut,
    constants: &Constants,
    start_round_id: u64,
    end_round_id: u64,
    lock_end: u64,
    funds: Uint128,
    get_old_voting_power: T,
) -> StdResult<()>
where
    T: Fn(u64, Timestamp, Uint128) -> Uint128,
{
    for round in start_round_id..=end_round_id {
        let round_end = compute_round_end(constants, round)?;
        let lockup_length = lock_end - round_end.nanos();
        let voting_power_change =
            scale_lockup_power(constants.lock_epoch_length, lockup_length, funds)
                - get_old_voting_power(round, round_end, funds);

        // save some gas if there was no power change
        if voting_power_change == Uint128::zero() {
            continue;
        }

        TOTAL_ROUND_POWER.update(deps.storage, round, |power| -> Result<Uint128, StdError> {
            match power {
                Some(power) => Ok(power + voting_power_change),
                None => Ok(voting_power_change),
            }
        })?;
    }

    Ok(())
}

// Returns the number of locks for a given user
fn get_lock_count(deps: Deps, user_address: Addr) -> u64 {
    LOCKS_MAP
        .prefix(user_address)
        .range(deps.storage, None, None, Order::Ascending)
        .count() as u64
}
