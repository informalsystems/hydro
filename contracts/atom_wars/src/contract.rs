// MAIN TODOS:
// - Query methods! We want a very complete set so that it is easy for third party tribute contracts
// - Tests!
// - Add real covenant logic
// - Make it work for separate tranches
// - Question: How to handle the case where a proposal is executed but the covenant fails?
// - Covenant Question: How to deal with someone using MEV to skew the pool ratio right before the liquidity is pulled? Streaming the liquidity pull? You'd have to set up a cron job for that.
// - Covenant Question: Can people sandwich this whole thing - covenant system has price limits - but we should allow people to retry executing the prop during the round

use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Deps, DepsMut, Env, MessageInfo, Order,
    Response, StdError, StdResult, Timestamp, Uint128,
};

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg};
use crate::query::{QueryMsg, RoundProposalsResponse, UserLockupsResponse};
use crate::state::{
    Constants, CovenantParams, LockEntry, Proposal, Tranche, Vote, CONSTANTS, LOCKS_MAP, LOCK_ID,
    PROPOSAL_MAP, PROPS_BY_SCORE, PROP_ID, TOTAL_POWER_VOTING, TRANCHE_MAP, VOTE_MAP, WHITELIST,
    WHITELIST_ADMINS,
};

pub const ONE_MONTH_IN_NANO_SECONDS: u64 = 2629746000000000; // 365 days / 12
pub const DEFAULT_MAX_ENTRIES: usize = 100;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    // validate that the first round starts in the future
    if msg.first_round_start < env.block.time {
        return Err(ContractError::Std(StdError::generic_err(
            "First round start time must be in the future",
        )));
    }

    let state = Constants {
        denom: msg.denom.clone(),
        round_length: msg.round_length,
        total_pool: msg.total_pool,
        first_round_start: msg.first_round_start,
    };

    CONSTANTS.save(deps.storage, &state)?;
    LOCK_ID.save(deps.storage, &0)?;
    PROP_ID.save(deps.storage, &0)?;

    WHITELIST_ADMINS.save(deps.storage, &msg.whitelist_admins)?;
    WHITELIST.save(deps.storage, &msg.initial_whitelist)?;

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
        .add_attribute("sender", _info.sender.clone())
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
        } // ExecuteMsg::ExecuteProposal { proposal_id } => {
          //     execute_proposal(deps, env, info, proposal_id)
          // }
    }
}

// LockTokens(lock_duration):
//     Receive tokens
//     Validate against denom whitelist
//     Create entry in LocksMap
fn lock_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    lock_duration: u64,
) -> Result<Response, ContractError> {
    // Validate that their lock duration (given in nanos) is either 1 month, 3 months, 6 months, or 12 months
    if lock_duration != ONE_MONTH_IN_NANO_SECONDS
        && lock_duration != ONE_MONTH_IN_NANO_SECONDS * 3
        && lock_duration != ONE_MONTH_IN_NANO_SECONDS * 6
        && lock_duration != ONE_MONTH_IN_NANO_SECONDS * 12
    {
        return Err(ContractError::Std(StdError::generic_err(
            "Lock duration must be 1, 3, 6, or 12 months",
        )));
    }

    // Validate that sent funds are the required denom
    if info.funds.len() != 1 {
        return Err(ContractError::Std(StdError::generic_err(
            "Must send exactly one coin",
        )));
    }

    let sent_funds = info
        .funds
        .get(0)
        .ok_or_else(|| ContractError::Std(StdError::generic_err("Must send exactly one coin")))?;

    if sent_funds.denom != CONSTANTS.load(deps.storage)?.denom {
        return Err(ContractError::Std(StdError::generic_err(
            "Must send the correct denom",
        )));
    }

    // Create entry in LocksMap
    let lock_entry = LockEntry {
        funds: sent_funds.clone(),
        lock_start: env.block.time,
        lock_end: env.block.time.plus_nanos(lock_duration),
    };
    let lock_id = LOCK_ID.load(deps.storage)?;
    LOCK_ID.save(deps.storage, &(lock_id + 1))?;
    LOCKS_MAP.save(deps.storage, (info.sender, lock_id), &lock_entry)?;

    Ok(Response::new().add_attribute("action", "lock_tokens"))
}

// UnlockTokens():
//     Validate caller
//     Validate `lock_end` < now
//     Send `amount` tokens back to caller
//     Delete entry from LocksMap
fn unlock_tokens(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    // Iterate all locks for the caller and unlock them if lock_end < now
    let locks =
        LOCKS_MAP
            .prefix(info.sender.clone())
            .range(deps.storage, None, None, Order::Ascending);

    let mut sends = vec![];
    let mut to_delete = vec![];

    for lock in locks {
        let (lock_id, lock_entry) = lock?;
        if lock_entry.lock_end < env.block.time {
            // Send tokens back to caller
            sends.push(lock_entry.funds.clone());

            // Delete entry from LocksMap
            to_delete.push((info.sender.clone(), lock_id));
        }
    }

    // Delete unlocked locks
    for (addr, lock_id) in to_delete {
        LOCKS_MAP.remove(deps.storage, (addr, lock_id));
    }

    let mut response = Response::new().add_attribute("action", "unlock_tokens");

    if sends.len() > 0 {
        response = response.add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: sends,
        })
    }

    Ok(response)
}

fn validate_covenant_params(_covenant_params: CovenantParams) -> Result<(), ContractError> {
    // Validate covenant_params
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
    validate_covenant_params(covenant_params.clone())?;
    TRANCHE_MAP.load(deps.storage, tranche_id)?;

    let round_id = compute_current_round_id(deps.as_ref(), env)?;
    let proposal_id = PROP_ID.load(deps.storage)?;

    // Create proposal in PropMap
    let proposal = Proposal {
        round_id,
        tranche_id,
        proposal_id,
        covenant_params,
        executed: false,
        power: Uint128::zero(),
        percentage: Uint128::zero(),
    };

    PROP_ID.save(deps.storage, &(proposal_id + 1))?;
    PROPOSAL_MAP.save(deps.storage, (round_id, tranche_id, proposal_id), &proposal)?;

    // load the total voting power for this round and tranche
    let total_power_voting = TOTAL_POWER_VOTING.load(deps.storage, (round_id, tranche_id));

    // if there is no total power voting for this round and tranche, set it to 0
    if total_power_voting.is_err() {
        TOTAL_POWER_VOTING.save(deps.storage, (round_id, tranche_id), &Uint128::zero())?;
    }

    Ok(Response::new().add_attribute("action", "create_proposal"))
}

fn scale_lockup_power(lockup_time: u64, raw_power: Uint128) -> Uint128 {
    let two: Uint128 = 2u16.into();

    // Scale lockup power
    // 1x if lockup is between 0 and 1 months
    // 1.5x if lockup is between 1 and 3 months
    // 2x if lockup is between 3 and 6 months
    // 4x if lockup is between 6 and 12 months
    // TODO: is there a less funky way to do Uint128 math???
    let scaled_power = match lockup_time {
        // 4x if lockup is over 6 months
        lockup_time if lockup_time > ONE_MONTH_IN_NANO_SECONDS * 6 => raw_power * two * two,
        // 2x if lockup is between 3 and 6 months
        lockup_time if lockup_time > ONE_MONTH_IN_NANO_SECONDS * 3 => raw_power * two,
        // 1.5x if lockup is between 1 and 3 months
        lockup_time if lockup_time > ONE_MONTH_IN_NANO_SECONDS => raw_power + (raw_power / two),
        // Covers 0 and 1 month which have no scaling
        _ => raw_power,
    };

    scaled_power
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

    // Load the round_id
    let round_id = compute_current_round_id(deps.as_ref(), env.clone())?;

    // compute the round end
    let round_end = compute_round_end(deps.as_ref(), round_id)?;

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
        PROPS_BY_SCORE.save(
            deps.storage,
            (
                (round_id, proposal.tranche_id),
                proposal.power.into(),
                vote.prop_id,
            ),
            &vote.prop_id,
        )?;

        // Decrement total power voting
        let total_power_voting = TOTAL_POWER_VOTING.load(deps.storage, (round_id, tranche_id))?;
        TOTAL_POWER_VOTING.save(
            deps.storage,
            (round_id, tranche_id),
            &(total_power_voting - vote.power),
        )?;

        // Delete vote
        VOTE_MAP.remove(deps.storage, (round_id, tranche_id, info.sender.clone()));
    }

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

        // Get the remaining lockup time at the end of this round.
        // This means that their power will be scaled the same by this function no matter when they vote in the round
        let lockup_time = lock_entry.lock_end.nanos() - round_end.nanos();

        // Scale power. This is what implements the different powers for different lockup times.
        let scaled_power = scale_lockup_power(lockup_time, lock_entry.funds.amount);

        power += scaled_power;
    }

    let response = Response::new().add_attribute("action", "vote");

    // if users voting power is 0 we don't need to update any of the stores
    if power.eq(&Uint128::zero()) {
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

    // Increment total power voting
    let total_power_voting = TOTAL_POWER_VOTING.load(deps.storage, (round_id, tranche_id))?;
    TOTAL_POWER_VOTING.save(
        deps.storage,
        (round_id, tranche_id),
        &(total_power_voting + power),
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

    // Validate covenant_params
    validate_covenant_params(covenant_params.clone())?;

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

    // Validate covenant_params
    validate_covenant_params(covenant_params.clone())?;

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
        QueryMsg::AllUserLockups { address } => {
            to_json_binary(&query_all_user_lockups(deps, address)?)
        }
        QueryMsg::ExpiredUserLockups { address } => {
            to_json_binary(&query_expired_user_lockups(deps, env, address)?)
        }
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
        QueryMsg::RoundProposals {
            round_id,
            tranche_id,
        } => to_json_binary(&query_round_tranche_proposals(deps, round_id, tranche_id)?),
        QueryMsg::CurrentRound {} => to_json_binary(&compute_current_round_id(deps, env)?),
        QueryMsg::RoundEnd { round_id } => to_json_binary(&compute_round_end(deps, round_id)?),
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

pub fn query_constants(deps: Deps) -> StdResult<Constants> {
    CONSTANTS.load(deps.storage)
}

pub fn query_all_user_lockups(deps: Deps, address: String) -> StdResult<UserLockupsResponse> {
    Ok(UserLockupsResponse {
        lockups: query_user_lockups(deps, deps.api.addr_validate(&address)?, |_| true),
    })
}

pub fn query_expired_user_lockups(
    deps: Deps,
    env: Env,
    address: String,
) -> StdResult<UserLockupsResponse> {
    let user_address = deps.api.addr_validate(&address)?;
    let expired_lockup_predicate = |l: &LockEntry| l.lock_end < env.block.time;

    Ok(UserLockupsResponse {
        lockups: query_user_lockups(deps, user_address, expired_lockup_predicate),
    })
}

pub fn query_proposal(
    deps: Deps,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> StdResult<Proposal> {
    Ok(PROPOSAL_MAP.load(deps.storage, (round_id, tranche_id, proposal_id))?)
}

pub fn query_user_voting_power(deps: Deps, env: Env, address: String) -> StdResult<u128> {
    let user_address = deps.api.addr_validate(&address)?;
    let current_round_id = compute_current_round_id(deps, env)?;
    let round_end = compute_round_end(deps, current_round_id)?;

    Ok(LOCKS_MAP
        .prefix(user_address)
        .range(deps.storage, None, None, Order::Ascending)
        .map(|l| l.unwrap().1)
        .filter(|l| l.lock_end > round_end)
        .map(|lockup| {
            let lockup_time = lockup.lock_end.nanos() - round_end.nanos();
            scale_lockup_power(lockup_time, lockup.funds.amount).u128()
        })
        .sum())
}

pub fn query_user_vote(
    deps: Deps,
    round_id: u64,
    tranche_id: u64,
    user_address: String,
) -> StdResult<Vote> {
    Ok(VOTE_MAP.load(
        deps.storage,
        (round_id, tranche_id, deps.api.addr_validate(&user_address)?),
    )?)
}

pub fn query_round_tranche_proposals(
    deps: Deps,
    round_id: u64,
    tranche_id: u64,
) -> StdResult<RoundProposalsResponse> {
    if let Err(_) = TRANCHE_MAP.load(deps.storage, tranche_id) {
        return Err(StdError::generic_err("Tranche does not exist"));
    }

    let props = PROPOSAL_MAP.prefix((round_id, tranche_id)).range(
        deps.storage,
        None,
        None,
        Order::Ascending,
    );

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
    if let Err(_) = TRANCHE_MAP.load(deps.storage, tranche_id) {
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
            Err(e) => false,
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

    // find sum of power
    let sum_power = top_props
        .iter()
        .fold(0u128, |sum, prop| sum + prop.power.u128());

    // return top props
    return Ok(top_props
        .into_iter() // Change from iter() to into_iter()
        .map(|mut prop| {
            // Change to mutable binding
            prop.percentage = (prop.power.u128() / sum_power).into();
            prop
        })
        .collect());
}

pub fn query_tranches(deps: Deps) -> StdResult<Vec<Tranche>> {
    let tranches = TRANCHE_MAP
        .range(deps.storage, None, None, Order::Ascending)
        .map(|t| t.unwrap().1)
        .collect::<Vec<_>>();

    Ok(tranches)
}

fn query_user_lockups(
    deps: Deps,
    user_address: Addr,
    predicate: impl FnMut(&LockEntry) -> bool,
) -> Vec<LockEntry> {
    LOCKS_MAP
        .prefix(user_address)
        .range(deps.storage, None, None, Order::Ascending)
        .map(|l| l.unwrap().1)
        .filter(predicate)
        .take(DEFAULT_MAX_ENTRIES)
        .collect()
}

fn query_whitelist(deps: Deps) -> StdResult<Vec<CovenantParams>> {
    WHITELIST.load(deps.storage)
}

fn query_whitelist_admins(deps: Deps) -> StdResult<Vec<Addr>> {
    WHITELIST_ADMINS.load(deps.storage)
}

// Computes the current round_id by taking contract_start_time and dividing the time since
// by the round_length.
pub fn compute_current_round_id(deps: Deps, env: Env) -> StdResult<u64> {
    let constants = CONSTANTS.load(deps.storage)?;
    let current_time = env.block.time.nanos();
    // If the first round has not started yet, return an error
    if current_time < constants.first_round_start.nanos() {
        return Err(StdError::generic_err("The first round has not started yet"));
    }
    let time_since_start = current_time - constants.first_round_start.nanos();
    let current_round_id = time_since_start / constants.round_length;

    Ok(current_round_id)
}

pub fn compute_round_end(deps: Deps, round_id: u64) -> StdResult<Timestamp> {
    let constants = CONSTANTS.load(deps.storage)?;

    let round_end = constants
        .first_round_start
        .plus_nanos(constants.round_length * (round_id + 1));

    Ok(round_end)
}
