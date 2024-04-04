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
use crate::msg::{CountResponse, ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{
    Constants, LockEntry, Proposal, Round, Vote, CONSTANTS, LOCKS_MAP, LOCK_ID, PROPOSAL_MAP,
    PROPS_BY_SCORE, PROP_ID, ROUND_ID, ROUND_MAP, TOTAL_POWER_VOTING, VOTE_MAP,
};

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let state = Constants {
        denom: msg.denom.clone(),
        round_length: msg.round_length,
        total_pool: msg.total_pool,
    };
    CONSTANTS.save(deps.storage, &state)?;
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
        ExecuteMsg::CreateProposal { covenant_params } => create_proposal(deps, covenant_params),
        ExecuteMsg::Vote { proposal_id } => vote(deps, info, proposal_id),
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
    let one_month_in_nanos: u64 = 2629746000000000;

    if lock_duration != one_month_in_nanos
        && lock_duration != one_month_in_nanos * 3
        && lock_duration != one_month_in_nanos * 6
        && lock_duration != one_month_in_nanos * 12
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

    Ok(Response::new()
        .add_attribute("action", "unlock_tokens")
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: sends,
        }))
}

fn validate_covenant_params(_covenant_params: String) -> Result<(), ContractError> {
    // Validate covenant_params
    Ok(())
}

// CreateProposal(covenant_params, tribute):
//     Validate covenant_params
//     Hold tribute in contract's account
//     Create in PropMap
fn create_proposal(deps: DepsMut, covenant_params: String) -> Result<Response, ContractError> {
    validate_covenant_params(covenant_params.clone())?;

    let round_id = ROUND_ID.load(deps.storage)?;

    // Create proposal in PropMap
    let proposal = Proposal {
        covenant_params,
        round_id,
        executed: false,
        power: Uint128::zero(),
        percentage: Uint128::zero(),
        amount: Uint128::zero(),
    };

    let prop_id = PROP_ID.load(deps.storage)?;
    PROP_ID.save(deps.storage, &(prop_id + 1))?;
    PROPOSAL_MAP.save(deps.storage, (round_id, prop_id), &proposal)?;

    Ok(Response::new().add_attribute("action", "create_proposal"))
}

fn scale_lockup_power(lockup_time: u64, raw_power: Uint128) -> Uint128 {
    let one_month_in_nanos: u64 = 2629746000000000;

    let two: Uint128 = 2u16.into();

    // Scale lockup power
    // 1x if lockup is between 0 and 1 months
    // 1.5x if lockup is between 1 and 3 months
    // 2x if lockup is between 3 and 6 months
    // 4x if lockup is between 6 and 12 months
    // TODO: is there a less funky way to do Uint128 math???
    let scaled_power = match lockup_time {
        // 4x if lockup is over 6 months
        lockup_time if lockup_time > one_month_in_nanos * 6 => raw_power * two * two,
        // 2x if lockup is between 3 and 6 months
        lockup_time if lockup_time > one_month_in_nanos * 3 => raw_power * two,
        // 1.5x if lockup is between 1 and 3 months
        lockup_time if lockup_time > one_month_in_nanos => raw_power + (raw_power / two),
        // Covers 0 and 1 month which have no scaling
        _ => raw_power,
    };

    scaled_power
}

fn vote(deps: DepsMut, info: MessageInfo, proposal_id: u64) -> Result<Response, ContractError> {
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

    // Load the round_id
    let round_id = ROUND_ID.load(deps.storage)?;

    // Load the round
    let round = ROUND_MAP.load(deps.storage, round_id)?;

    // Get any existing vote for this sender and reverse it- this may be a vote for a different proposal (if they are switching their vote),
    // or it may be a vote for the same proposal (if they have increased their power by locking more and want to update their vote).
    // TODO: this could be made more gas-efficient by using a separate path with fewer writes if the vote is for the same proposal
    let vote = VOTE_MAP.load(deps.storage, (round_id, info.sender.clone()));
    if let Ok(vote) = vote {
        // Load the proposal in the vote
        let mut proposal = PROPOSAL_MAP.load(deps.storage, (round_id, vote.prop_id))?;

        // Remove proposal's old power in PROPS_BY_SCORE
        PROPS_BY_SCORE.remove(
            deps.storage,
            (round_id, proposal.power.into(), vote.prop_id),
        );

        // Decrement proposal's power
        proposal.power -= vote.power;

        // Save the proposal
        PROPOSAL_MAP.save(deps.storage, (round_id, vote.prop_id), &proposal)?;

        // Add proposal's new power in PROPS_BY_SCORE
        PROPS_BY_SCORE.save(
            deps.storage,
            (round_id, proposal.power.into(), vote.prop_id),
            &vote.prop_id,
        )?;

        // Decrement total power voting
        let total_power_voting = TOTAL_POWER_VOTING.load(deps.storage, round_id)?;
        TOTAL_POWER_VOTING.save(deps.storage, round_id, &(total_power_voting - vote.power))?;

        // Delete vote
        VOTE_MAP.remove(deps.storage, (round_id, info.sender.clone()));
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
    let mut proposal = PROPOSAL_MAP.load(deps.storage, (round_id, proposal_id))?;

    // Delete the proposal's old power in PROPS_BY_SCORE
    PROPS_BY_SCORE.remove(deps.storage, (round_id, proposal.power.into(), proposal_id));

    // Update proposal's power
    proposal.power += power;

    // Save the proposal
    PROPOSAL_MAP.save(deps.storage, (round_id, proposal_id), &proposal)?;

    // Save the proposal's new power in PROPS_BY_SCORE
    PROPS_BY_SCORE.save(
        deps.storage,
        (round_id, proposal.power.into(), proposal_id),
        &proposal_id,
    )?;

    // Increment total power voting
    let total_power_voting = TOTAL_POWER_VOTING.load(deps.storage, round_id)?;
    TOTAL_POWER_VOTING.save(deps.storage, round_id, &(total_power_voting + power))?;

    // Create vote in Votemap
    let vote = Vote {
        prop_id: proposal_id,
        power,
    };
    VOTE_MAP.save(deps.storage, (round_id, info.sender), &vote)?;

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

    Ok(Response::new().add_attribute("action", "tally"))
}

fn do_covenant_stuff(
    _deps: Deps,
    _env: Env,
    _info: MessageInfo,
    _covenant_params: String,
) -> Result<Response, ContractError> {
    // Do covenant stuff
    Ok(Response::new().add_attribute("action", "do_covenant_stuff"))
}

fn get_top_props(deps: Deps, round_id: u64, num: usize) -> Result<Vec<Proposal>, ContractError> {
    // Iterate through PROPS_BY_SCORE to find the top ten props
    let top_prop_ids: Vec<u64> = PROPS_BY_SCORE
        .sub_prefix(round_id)
        .range(deps.storage, None, None, Order::Descending)
        .take(num)
        .map(|x| match x {
            Ok((_, prop_id)) => prop_id,
            Err(_) => 0, // Handle the error case appropriately
        })
        .collect();

    let mut top_props = vec![];

    for prop_id in top_prop_ids {
        let prop = PROPOSAL_MAP.load(deps.storage, (round_id, prop_id))?;
        top_props.push(prop);
    }

    // find sum of power
    let sum_power = top_props.iter().fold(0u128, |sum, prop| { sum + prop.power.u128() });

    // // loop to find percentage of power
    // for mut prop in top_props {
    //     prop.percentage = (prop.power.u128() / sum_power).into();
    // }

    // // Multiply by total pool to find exact amount
    // for mut prop in top_props {
    //     prop.amount = prop.percentage * CONSTANTS.load(deps.storage)?.total_pool;
    // }

    // for top_props {
    //     prop.percentage = (prop.power.u128() / sum_power).into();
    //     prop.amount = prop.percentage * CONSTANTS.load(deps.storage)?.total_pool;
    // }

    let total_pool = CONSTANTS.load(deps.storage)?.total_pool;

    // return top props
    return Ok(top_props
        .into_iter() // Change from iter() to into_iter()
        .map(|mut prop| {
            // Change to mutable binding
            prop.percentage = (prop.power.u128() / sum_power).into();
            prop.amount = prop.percentage * total_pool;
            prop
        })
        .collect());
}

// TODO: we need to do this differently to allocate liquidity proportionally to the top props instead of the old
// winner takes all method
// fn execute_proposal(
//     deps: DepsMut,
//     env: Env,
//     info: MessageInfo,
//     proposal_id: u64,
// ) -> Result<Response, ContractError> {
//     // Load the last round_id
//     let last_round_id = ROUND_ID.load(deps.storage)? - 1;

//     // Load the winning prop_id
//     let winning_prop_id = get_winning_prop(deps.as_ref(), last_round_id)?;

//     // Check that this prop is the one that won
//     if winning_prop_id != proposal_id {
//         return Err(ContractError::Std(StdError::generic_err(
//             "Proposal did not win the last round",
//         )));
//     }

//     // Load the proposal
//     let mut proposal = PROPOSAL_MAP.load(deps.storage, (last_round_id, proposal_id))?;

//     // Check that the proposal has not already been executed
//     if proposal.executed {
//         return Err(ContractError::Std(StdError::generic_err(
//             "Proposal already executed",
//         )));
//     }

//     // Execute proposal
//     do_covenant_stuff(deps.as_ref(), env, info, proposal.clone().covenant_params)?;

//     // Mark proposal as executed
//     proposal.executed = true;
//     PROPOSAL_MAP.save(deps.storage, (last_round_id, proposal_id), &proposal)?;

//     Ok(Response::new().add_attribute("action", "execute_proposal"))
// }

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetCount {} => query_count(deps),
    }
}

// pub fn query_winning_proposal(deps: Deps) -> StdResult<Binary> {
//     let winning_prop_id = get_winning_prop(deps)?;
//     to_json_binary(&winning_prop_id)
// }

pub fn query_count(_deps: Deps) -> StdResult<Binary> {
    to_json_binary(&(CountResponse { count: 0 }))
}
