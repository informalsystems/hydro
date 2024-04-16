// MAIN TODOS:
// - Query methods! We want a very complete set so that it is easy for third party tribute contracts
// - Tests!
// - Add real covenant logic
// - Make it work for separate tranches
// - Question: How to handle the case where a proposal is executed but the covenant fails?
// - Covenant Question: How to deal with someone using MEV to skew the pool ratio right before the liquidity is pulled? Streaming the liquidity pull? You'd have to set up a cron job for that.
// - Covenant Question: Can people sandwich this whole thing - covenant system has price limits - but we should allow people to retry executing the prop during the round

use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdError, StdResult, Uint128,
};

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg};
use crate::query::{QueryMsg, RoundProposalsResponse, UserLockupsResponse};
use crate::state::{
    Constants, LockEntry, Proposal, Round, Tranche, Vote, CONSTANTS, LOCKS_MAP, LOCK_ID,
    PROPOSAL_MAP, PROPS_BY_SCORE, PROP_ID, ROUND_ID, ROUND_MAP, TOTAL_POWER_VOTING, TRANCHE_MAP,
    VOTE_MAP,
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
    let state = Constants {
        denom: msg.denom.clone(),
        round_length: msg.round_length,
        total_pool: msg.total_pool,
    };

    CONSTANTS.save(deps.storage, &state)?;
    LOCK_ID.save(deps.storage, &0)?;
    PROP_ID.save(deps.storage, &0)?;

    // TODO: Is it ok to start the first round immediately?
    // If not, just EndRound is not enough, sice we need to create initial round somehow.
    // Possible solutions:
    //      1. Create initial round when the first proposal gets submitted
    //      2. Specify initial round start time through some InstantiateMsg field
    let round_id = 0;
    ROUND_ID.save(deps.storage, &round_id)?;
    ROUND_MAP.save(
        deps.storage,
        0,
        &Round {
            round_id: round_id,
            round_end: env.block.time.plus_nanos(msg.round_length),
        },
    )?;

    // For each tranche, create a tranche in the TRANCHE_MAP and set the total power to 0
    let mut tranche_ids = std::collections::HashSet::new();

    for tranche in msg.tranches {
        if !tranche_ids.insert(tranche.tranche_id) {
            return Err(ContractError::Std(StdError::generic_err(
                "Duplicate tranche ID found in provided tranches, but tranche IDs must be unique",
            )));
        }
        TRANCHE_MAP.save(deps.storage, tranche.tranche_id, &tranche)?;
        TOTAL_POWER_VOTING.save(
            deps.storage,
            (round_id, tranche.tranche_id),
            &Uint128::zero(),
        )?;
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
        } => create_proposal(deps, tranche_id, covenant_params),
        ExecuteMsg::Vote {
            tranche_id,
            proposal_id,
        } => vote(deps, info, tranche_id, proposal_id),
        ExecuteMsg::EndRound {} => end_round(deps, env, info),
        // ExecuteMsg::ExecuteProposal { proposal_id } => {
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

fn validate_covenant_params(_covenant_params: String) -> Result<(), ContractError> {
    // Validate covenant_params
    Ok(())
}

// CreateProposal(covenant_params, tribute):
//     Validate covenant_params
//     Hold tribute in contract's account
//     Create in PropMap
fn create_proposal(
    deps: DepsMut,
    tranche_id: u64,
    covenant_params: String,
) -> Result<Response, ContractError> {
    validate_covenant_params(covenant_params.clone())?;
    TRANCHE_MAP.load(deps.storage, tranche_id)?;

    let round_id = ROUND_ID.load(deps.storage)?;

    // Create proposal in PropMap
    let proposal = Proposal {
        round_id,
        tranche_id,
        covenant_params,
        executed: false,
        power: Uint128::zero(),
        percentage: Uint128::zero(),
    };

    let prop_id = PROP_ID.load(deps.storage)?;
    PROP_ID.save(deps.storage, &(prop_id + 1))?;
    PROPOSAL_MAP.save(deps.storage, (round_id, tranche_id, prop_id), &proposal)?;

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
    let round_id = ROUND_ID.load(deps.storage)?;

    // Load the round
    let round = ROUND_MAP.load(deps.storage, round_id)?;

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

        // Get the remaining lockup time at the end of this round.
        // This means that their power will be scaled the same by this function no matter when they vote in the round
        let lockup_time = lock_entry.lock_end.nanos() - round.round_end.nanos();

        // Scale power. This is what implements the different powers for different lockup times.
        let scaled_power = scale_lockup_power(lockup_time, lock_entry.funds.amount);

        power += scaled_power;
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

    Ok(Response::new().add_attribute("action", "vote"))
}

fn end_round(deps: DepsMut, env: Env, _info: MessageInfo) -> Result<Response, ContractError> {
    // Check that round has ended by getting latest round and checking if round_end < now
    let round_id = ROUND_ID.load(deps.storage)?;
    let round = ROUND_MAP.load(deps.storage, round_id)?;

    if round.round_end > env.block.time {
        return Err(ContractError::Std(StdError::generic_err(
            "Round has not ended yet",
        )));
    }

    // Calculate the round_end for the next round
    let round_end = env
        .block
        .time
        .plus_nanos(CONSTANTS.load(deps.storage)?.round_length);

    // Increment the round_id
    let round_id = round.round_id + 1;
    ROUND_ID.save(deps.storage, &(round_id))?;
    // Save the round
    ROUND_MAP.save(
        deps.storage,
        round_id,
        &Round {
            round_end,
            round_id,
        },
    )?;

    let tranches = TRANCHE_MAP
        .range(deps.storage, None, None, Order::Ascending)
        .map(|t| t.unwrap().1)
        .collect::<Vec<_>>();

    // Iterate through each tranche
    for tranche in tranches {
        // Initialize total voting power for new round
        TOTAL_POWER_VOTING.save(
            deps.storage,
            (round_id, tranche.tranche_id),
            &Uint128::zero(),
        )?;
    }

    Ok(Response::new().add_attribute("action", "tally"))
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

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Constants {} => to_json_binary(&query_constants(deps)?),
        QueryMsg::AllUserLockups { address } => {
            to_json_binary(&query_all_user_lockups(deps, address)?)
        }
        QueryMsg::Proposal {
            round_id,
            tranche_id,
            proposal_id,
        } => to_json_binary(&query_proposal(deps, round_id, tranche_id, proposal_id)?),
        QueryMsg::RoundProposals {
            round_id,
            tranche_id,
        } => to_json_binary(&query_round_tranche_proposals(deps, round_id, tranche_id)?),
        QueryMsg::CurrentRound {} => to_json_binary(&query_current_round(deps)?),
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
    }
}

pub fn query_constants(deps: Deps) -> StdResult<Constants> {
    CONSTANTS.load(deps.storage)
}

// TODO: implement a proper pagination for this and other queries
pub fn query_all_user_lockups(deps: Deps, address: String) -> StdResult<UserLockupsResponse> {
    let user_address = deps.api.addr_validate(address.as_str())?;

    let user_lockups: Vec<LockEntry> = LOCKS_MAP
        .prefix(user_address)
        .range(deps.storage, None, None, Order::Ascending)
        .take(DEFAULT_MAX_ENTRIES)
        .into_iter()
        .map(|l| l.unwrap().1)
        .collect();

    Ok(UserLockupsResponse {
        lockups: user_lockups,
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

pub fn query_round_tranche_proposals(
    deps: Deps,
    round_id: u64,
    tranche_id: u64,
) -> StdResult<RoundProposalsResponse> {
    // check if the round exists so that we can make distinction between non-existing round and round without proposals
    if let Err(_) = ROUND_MAP.may_load(deps.storage, round_id) {
        return Err(StdError::generic_err("Round does not exist"));
    }

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

pub fn query_current_round(deps: Deps) -> StdResult<Round> {
    Ok(ROUND_MAP.load(deps.storage, ROUND_ID.load(deps.storage)?)?)
}

pub fn query_top_n_proposals(
    deps: Deps,
    round_id: u64,
    tranche_id: u64,
    num: usize,
) -> StdResult<Vec<Proposal>> {
    // check if the round exists
    if let Err(_) = ROUND_MAP.may_load(deps.storage, round_id) {
        return Err(StdError::generic_err("Round does not exist"));
    }

    if let Err(_) = TRANCHE_MAP.load(deps.storage, tranche_id) {
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
