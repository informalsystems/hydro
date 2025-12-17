use std::collections::HashSet;

use cosmwasm_std::{
    Addr, Decimal, Deps, DepsMut, Env, Order, StdError, StdResult, Storage, Timestamp, Uint128,
};
use cw_storage_plus::Bound;

use crate::{
    contract::{compute_current_round_id, compute_round_end},
    error::{new_generic_error, ContractError},
    msg::LiquidityDeployment,
    query::{LockEntryWithPower, LockupWithPerTrancheInfo, PerTrancheLockupInfo, RoundWithBid},
    score_keeper::get_total_power_for_round,
    state::{
        Constants, HeightRange, LockEntryV2, Proposal, RoundLockPowerSchedule, Vote,
        AVAILABLE_CONVERSION_FUNDS, CONSTANTS, EXTRA_LOCKED_TOKENS_CURRENT_USERS,
        EXTRA_LOCKED_TOKENS_ROUND_TOTAL, HEIGHT_TO_ROUND, LIQUIDITY_DEPLOYMENTS_MAP, LOCKED_TOKENS,
        LOCKS_MAP_V1, LOCKS_MAP_V2, LOCK_ID, PROPOSAL_MAP, ROUND_TO_HEIGHT_RANGE,
        SNAPSHOTS_ACTIVATION_HEIGHT, USER_LOCKS, USER_LOCKS_FOR_CLAIM, VOTE_MAP_V2,
        VOTING_ALLOWED_ROUND,
    },
    token_manager::TokenManager,
};

/// Loads the constants that are active for the current block according to the block timestamp.
pub fn load_current_constants(deps: &Deps, env: &Env) -> StdResult<Constants> {
    Ok(load_constants_active_at_timestamp(deps, env.block.time)?.1)
}

/// Loads the constants that were active at the given timestamp. Returns both the Constants and
/// their activation timestamp.
pub fn load_constants_active_at_timestamp(
    deps: &Deps,
    timestamp: Timestamp,
) -> StdResult<(u64, Constants)> {
    let current_constants: Vec<(u64, Constants)> = CONSTANTS
        .range(
            deps.storage,
            None,
            Some(Bound::inclusive(timestamp.nanos())),
            Order::Descending,
        )
        .take(1)
        .filter_map(|constants| constants.ok())
        .collect();

    Ok(match current_constants.len() {
        1 => current_constants[0].clone(),
        _ => {
            return Err(StdError::generic_err(
                "Failed to load constants active at the given timestamp.",
            ));
        }
    })
}

// This function validates if user should be allowed to lock more tokens, depending on the total amount of
// currently locked tokens, existence of the known users cap and the number of tokens user already locked in that cap.
// Caps that we consider:
//      1) Total cap: the maximum number of tokens that can be locked in the contract, regardless of the
//         known users cap existence.
//      2) Known users cap: the number of tokens reserved exclusively for users that had some voting power in previous round.
//         It will be available to those users to lock additional tokens even if the public cap is filled. However,
//         if the public cap isn't full yet, these users would lock in the public cap first. Only if the public cap
//         is full, these users start using the known users cap.
//      3) Public cap: during the existence of known users cap, new Hydro users will be allowed to lock tokens only
//         in public cap, where public_cap = total_cap - known_users_cap.
//         After the known users cap duration expires, public cap becomes equal to the total cap.
pub fn validate_locked_tokens_caps(
    deps: &DepsMut,
    constants: &Constants,
    current_round: u64,
    sender: &Addr,
    total_locked_tokens: u128,
    amount_to_lock: u128,
) -> Result<LockingInfo, ContractError> {
    let lock_limit_reached_error = Err(new_generic_error(
        "The limit for locking tokens has been reached. No more tokens can be locked.",
    ));

    // Regardless of public_cap and known_users_cap, we must make sure that nobody can lock more than total_cap
    let total_locked_after = total_locked_tokens + amount_to_lock;
    if total_locked_after > constants.max_locked_tokens {
        return lock_limit_reached_error;
    }

    let public_cap = constants.max_locked_tokens - constants.known_users_cap;

    // This branch will be executed in one of the following cases:
    //   1) constants.known_users_cap != 0 and there is SOME room in the public cap
    //   2) constants.known_users_cap == 0 and there is ENOUGH room in the public_cap.
    //      Since in this case public_cap == total_cap, there must be enough room in the public_cap,
    //      otherwise we would error out at the start of this function.
    if public_cap > total_locked_tokens {
        // Check if entire amount_to_lock can fit into a public cap
        if public_cap >= total_locked_after {
            return Ok(LockingInfo {
                lock_in_public_cap: Some(amount_to_lock),
                lock_in_known_users_cap: None,
            });
        }

        // Lock as much as possible in the public_cap, and the rest in the known_users_cap
        let lock_in_public_cap = public_cap - total_locked_tokens;
        let lock_in_known_users_cap = amount_to_lock - lock_in_public_cap;

        // If there is still room in known_users_cap, then check if this
        // is a user that should be allowed to use the known_users_cap.
        can_user_lock_in_known_users_cap(
            &deps.as_ref(),
            constants,
            current_round,
            sender,
            lock_in_known_users_cap,
        )?;

        return Ok(LockingInfo {
            lock_in_public_cap: Some(lock_in_public_cap),
            lock_in_known_users_cap: Some(lock_in_known_users_cap),
        });
    }

    // If we got through here, it means that constants.known_users_cap > 0.
    // If constants.known_users_cap was 0 and public_cap (equal to total_cap)
    // is equal to total_locked_tokens, then we would error out at the start of this
    // function, because any amount_to_lock would exceed the total_cap. This is just
    // a safety check to make the code resilient to future changes.
    if constants.known_users_cap == 0 {
        return lock_limit_reached_error;
    }

    // If there is still room in known_users_cap, then check if this
    // is a user that should be allowed to use the known_users_cap.
    can_user_lock_in_known_users_cap(
        &deps.as_ref(),
        constants,
        current_round,
        sender,
        amount_to_lock,
    )?;

    Ok(LockingInfo {
        lock_in_public_cap: None,
        lock_in_known_users_cap: Some(amount_to_lock),
    })
}

fn can_user_lock_in_known_users_cap(
    deps: &Deps,
    constants: &Constants,
    current_round: u64,
    sender: &Addr,
    amount_to_lock: u128,
) -> Result<(), ContractError> {
    let extra_locked_tokens_round_total = EXTRA_LOCKED_TOKENS_ROUND_TOTAL
        .may_load(deps.storage, current_round)?
        .unwrap_or(0);

    if extra_locked_tokens_round_total + amount_to_lock > constants.known_users_cap {
        return Err(
            new_generic_error(
                format!("Can not lock {} tokens in known users cap. Currently locked in known users cap: {}; Maximum allowed to be locked in known users cap: {}",
                amount_to_lock, extra_locked_tokens_round_total, constants.known_users_cap)));
    }

    // Determine if user has the right to lock in known_users_cap by looking at its voting power in previous round.
    let round_to_check = match current_round {
        0 => {
            return Err(new_generic_error(
                "Can't lock in known users cap during round 0.",
            ))
        }
        _ => current_round - 1,
    };

    // Calculate user's voting power share in the total voting power
    let users_voting_power = Decimal::from_ratio(
        get_user_voting_power_for_past_round(deps, constants, sender.clone(), round_to_check)?,
        Uint128::one(),
    );
    let total_voting_power = get_total_power_for_round(deps, round_to_check)?;

    // Prevent division by zero or break early in case user had no voting power in previous round.
    if total_voting_power == Decimal::zero() || users_voting_power == Decimal::zero() {
        return Err(new_generic_error(format!(
            "Can not lock {amount_to_lock} tokens in known users cap. User had zero voting power in previous round."
        )));
    }

    let users_voting_power_share = users_voting_power.checked_div(total_voting_power)?;

    // Calculate what would be users share of extra locked tokens in the maximum allowed extra locked tokens
    let users_current_extra_lock = EXTRA_LOCKED_TOKENS_CURRENT_USERS
        .may_load(deps.storage, (current_round, sender.clone()))?
        .unwrap_or(0);

    let users_extra_lock =
        Decimal::from_ratio(users_current_extra_lock + amount_to_lock, Uint128::one());
    let maximum_extra_lock = Decimal::from_ratio(constants.known_users_cap, Uint128::one());

    if maximum_extra_lock == Decimal::zero() {
        return Err(new_generic_error(
            "Can not lock tokens in known users cap because it is not active.",
        ));
    }

    let users_extra_lock_share = users_extra_lock.checked_div(maximum_extra_lock)?;

    // If users share in maximum allowed known users cap would be greater than its share in
    // total voting power, then don't allow this user to lock the given amount of tokens.
    if users_extra_lock_share > users_voting_power_share {
        return Err(new_generic_error(format!(
            "Can not lock {amount_to_lock} tokens in known users cap. User reached the personal cap for locking tokens in the known users cap."
        )));
    }

    Ok(())
}

// Whenever a users locks more tokens this function will update the necessary stores,
// depending on the amounts that user locked in public_cap and known_users_cap.
// Stores that will (potentially) be updated:
//      LOCKED_TOKENS, EXTRA_LOCKED_TOKENS_ROUND_TOTAL, EXTRA_LOCKED_TOKENS_CURRENT_USERS
pub fn update_locked_tokens_info(
    deps: &mut DepsMut,
    current_round: u64,
    sender: &Addr,
    mut total_locked_tokens: u128,
    locking_info: &LockingInfo,
) -> Result<(), ContractError> {
    if let Some(lock_in_public_cap) = locking_info.lock_in_public_cap {
        total_locked_tokens += lock_in_public_cap;
        LOCKED_TOKENS.save(deps.storage, &total_locked_tokens)?;
    }

    if let Some(lock_in_known_users_cap) = locking_info.lock_in_known_users_cap {
        LOCKED_TOKENS.save(
            deps.storage,
            &(total_locked_tokens + lock_in_known_users_cap),
        )?;

        EXTRA_LOCKED_TOKENS_ROUND_TOTAL.update(
            deps.storage,
            current_round,
            |current_value| -> StdResult<u128> {
                match current_value {
                    None => Ok(lock_in_known_users_cap),
                    Some(current_value) => Ok(current_value + lock_in_known_users_cap),
                }
            },
        )?;

        EXTRA_LOCKED_TOKENS_CURRENT_USERS.update(
            deps.storage,
            (current_round, sender.clone()),
            |current_value| -> StdResult<u128> {
                match current_value {
                    None => Ok(lock_in_known_users_cap),
                    Some(current_value) => Ok(current_value + lock_in_known_users_cap),
                }
            },
        )?;
    }

    Ok(())
}

// Calls other functions that will update various stores whenever a transaction is executed against the contract.
pub fn run_on_each_transaction(
    storage: &mut dyn Storage,
    env: &Env,
    round_id: u64,
) -> StdResult<()> {
    update_round_height_maps(storage, env, round_id)
}

// Updates round_id -> height_range and block_height -> round_id maps, for later use.
pub fn update_round_height_maps(
    storage: &mut dyn Storage,
    env: &Env,
    round_id: u64,
) -> StdResult<()> {
    ROUND_TO_HEIGHT_RANGE.update(
        storage,
        round_id,
        |height_range| -> Result<HeightRange, StdError> {
            match height_range {
                None => Ok(HeightRange {
                    lowest_known_height: env.block.height,
                    highest_known_height: env.block.height,
                }),
                Some(mut height_range) => {
                    height_range.highest_known_height = env.block.height;

                    Ok(height_range)
                }
            }
        },
    )?;

    HEIGHT_TO_ROUND.save(storage, env.block.height, &round_id)
}

/// Returns the round ID in which Hydro was at the given height. Note that if the required height is after the end
/// of round N, but before the first transaction is issued in round N+1, this would return N, not N+1.
pub fn get_round_id_for_height(storage: &dyn Storage, height: u64) -> StdResult<u64> {
    verify_historical_data_availability(storage, height)?;

    let round_id: Vec<u64> = HEIGHT_TO_ROUND
        .range(
            storage,
            None,
            Some(Bound::inclusive(height)),
            Order::Descending,
        )
        .take(1)
        .filter_map(|round| match round {
            Ok(round) => Some(round.1),
            Err(_) => None,
        })
        .collect();

    Ok(match round_id.len() {
        1 => round_id[0],
        _ => {
            return Err(StdError::generic_err(format!(
                "Failed to load round ID for height {height}."
            )));
        }
    })
}

pub fn get_highest_known_height_for_round_id(
    storage: &dyn Storage,
    round_id: u64,
) -> StdResult<u64> {
    Ok(ROUND_TO_HEIGHT_RANGE
        .may_load(storage, round_id)?
        .unwrap_or_default()
        .highest_known_height)
}

pub fn verify_historical_data_availability(storage: &dyn Storage, height: u64) -> StdResult<()> {
    let snapshot_activation_height = SNAPSHOTS_ACTIVATION_HEIGHT.load(storage)?;
    if height < snapshot_activation_height {
        return Err(StdError::generic_err(format!(
            "Historical data not available before height: {snapshot_activation_height}. Height requested: {height}",
        )));
    }

    Ok(())
}

pub fn get_current_user_voting_power(deps: &Deps, env: &Env, address: Addr) -> StdResult<u128> {
    let constants = load_current_constants(deps, env)?;
    let current_round_id = compute_current_round_id(env, &constants)?;
    let round_end = compute_round_end(&constants, current_round_id)?;
    let mut token_manager = TokenManager::new(deps);

    // Get all lockups owned by the address from USER_LOCKS
    let user_locks = USER_LOCKS
        .may_load(deps.storage, address.clone())?
        .unwrap_or_default();

    // For each lock ID, load from LOCKS_MAP_V2, verify ownership, and compute voting power
    Ok(user_locks
        .iter()
        .filter_map(
            |&lock_id| match LOCKS_MAP_V2.may_load(deps.storage, lock_id) {
                Ok(Some(lock_entry)) => Some(
                    to_lockup_with_power(
                        deps,
                        &constants,
                        &mut token_manager,
                        current_round_id,
                        round_end,
                        lock_entry,
                    )
                    .current_voting_power
                    .u128(),
                ),
                _ => None,
            },
        )
        .sum())
}

/// Utility function intended to be used by get_user_voting_power_for_past_round() and get_user_voting_power_for_past_height().
/// Both of these functions will ensure that the provided height indeed matches the given round, and vice versa.
/// If the function is used in different context, the caller is responsible for ensuring this condition is satisifed.
fn get_past_user_voting_power(
    deps: &Deps,
    constants: &Constants,
    address: Addr,
    height: u64,
    round_id: u64,
) -> StdResult<u128> {
    let round_end = compute_round_end(constants, round_id)?;
    let mut token_manager = TokenManager::new(deps);

    let user_locks_ids = USER_LOCKS
        .may_load_at_height(deps.storage, address.clone(), height)?
        .unwrap_or_default();

    Ok(user_locks_ids
        .into_iter()
        .filter_map(|lock_id| {
            LOCKS_MAP_V2
                .may_load_at_height(deps.storage, lock_id, height)
                .unwrap_or_default()
                .or(LOCKS_MAP_V1
                    .may_load_at_height(deps.storage, (address.clone(), lock_id), height)
                    .unwrap_or_default()
                    .map(|v1_lockup| v1_lockup.into_v2(address.clone())))
        })
        .map(|lockup| {
            to_lockup_with_power(
                deps,
                constants,
                &mut token_manager,
                round_id,
                round_end,
                lockup,
            )
            // Current voting power in this context means the voting power that the lockup had in the
            // given past round, with the applied token group ratios as they were in that round.
            .current_voting_power
            .u128()
        })
        .sum())
}

pub fn get_user_voting_power_for_past_round(
    deps: &Deps,
    constants: &Constants,
    address: Addr,
    round_id: u64,
) -> StdResult<u128> {
    let height = get_highest_known_height_for_round_id(deps.storage, round_id)?;
    verify_historical_data_availability(deps.storage, height)?;
    get_past_user_voting_power(deps, constants, address, height, round_id)
}

pub fn get_user_voting_power_for_past_height(
    deps: &Deps,
    constants: &Constants,
    address: Addr,
    height: u64,
) -> StdResult<u128> {
    let round_id = get_round_id_for_height(deps.storage, height)?;
    get_past_user_voting_power(deps, constants, address, height, round_id)
}

pub fn to_lockup_with_power(
    deps: &Deps,
    constants: &Constants,
    token_manager: &mut TokenManager,
    round_id: u64,
    round_end: Timestamp,
    lock_entry: LockEntryV2,
) -> LockEntryWithPower {
    let Ok(token_ratio) = token_manager
        .validate_denom(deps, round_id, lock_entry.funds.denom.clone())
        .and_then(|token_group_id| {
            token_manager.get_token_group_ratio(deps, round_id, token_group_id)
        })
    else {
        return LockEntryWithPower {
            lock_entry,
            current_voting_power: Uint128::zero(),
        };
    };

    let time_weighted_shares = get_lock_time_weighted_shares(
        &constants.round_lock_power_schedule,
        round_end,
        &lock_entry,
        constants.lock_epoch_length,
    );

    let current_voting_power = token_ratio
        .checked_mul(Decimal::from_ratio(time_weighted_shares, Uint128::one()))
        .ok()
        .map_or_else(Uint128::zero, Decimal::to_uint_ceil);

    LockEntryWithPower {
        lock_entry,
        current_voting_power,
    }
}

fn historic_voted_on_proposals(
    storage: &dyn Storage,
    constants: &Constants,
    tranche_id: u64,
    lock_id: u64,
    current_round_id: u64,
) -> Result<Vec<RoundWithBid>, ContractError> {
    let mut historic_voted_on_proposals: Vec<RoundWithBid> = vec![];

    // Get all votes from VOTE_MAP_V2 for this lock_id and tranche_id
    // In future, we might want to add fields like history_start_from and history_limit when querying lockups.
    for round_id in 0..current_round_id {
        if let Some(vote) = get_lock_vote(storage, round_id, tranche_id, lock_id)? {
            let round_end = compute_round_end(constants, round_id).unwrap();

            historic_voted_on_proposals.push(RoundWithBid {
                round_id,
                proposal_id: vote.prop_id,
                round_end,
            });
        }
    }

    Ok(historic_voted_on_proposals)
}

fn per_round_tranche_info(
    deps: &Deps,
    constants: &Constants,
    tranche_id: u64,
    lock_id: u64,
    current_round_id: u64,
) -> Result<PerTrancheLockupInfo, ContractError> {
    let historic_voted_on_proposals = historic_voted_on_proposals(
        deps.storage,
        constants,
        tranche_id,
        lock_id,
        current_round_id,
    )?;

    if let Some(Vote { prop_id, .. }) =
        get_lock_vote(deps.storage, current_round_id, tranche_id, lock_id)?
    {
        return Ok(PerTrancheLockupInfo {
            tranche_id,
            next_round_lockup_can_vote: current_round_id,
            current_voted_on_proposal: Some(prop_id),
            tied_to_proposal: None,
            historic_voted_on_proposals,
        });
    }

    let mut next_round_lockup_can_vote = VOTING_ALLOWED_ROUND
        .may_load(deps.storage, (tranche_id, lock_id))?
        .unwrap_or(current_round_id);

    let mut tied_to_proposal = None;

    if next_round_lockup_can_vote > current_round_id {
        let proposal = find_voted_proposal_for_lock(deps, current_round_id, tranche_id, lock_id)?;

        let deployment = get_deployment_for_proposal(deps, &proposal)?;

        // If the deployment for the proposals exists, and has zero funds,
        // then the lock can vote in the current round
        if deployment.is_some_and(|d| !d.has_nonzero_funds()) {
            next_round_lockup_can_vote = current_round_id;
        } else {
            tied_to_proposal = Some(proposal.proposal_id);
        }
    }

    Ok(PerTrancheLockupInfo {
        tranche_id,
        next_round_lockup_can_vote,
        current_voted_on_proposal: None,
        tied_to_proposal,
        historic_voted_on_proposals,
    })
}

pub fn to_lockup_with_tranche_infos(
    deps: &Deps,
    constants: &Constants,
    tranche_ids: &[u64],
    lock_with_power: LockEntryWithPower,
    current_round_id: u64,
) -> Result<LockupWithPerTrancheInfo, ContractError> {
    let mut per_tranche_info = Vec::with_capacity(tranche_ids.len());

    for tranche_id in tranche_ids {
        let tranche_info = per_round_tranche_info(
            deps,
            constants,
            *tranche_id,
            lock_with_power.lock_entry.lock_id,
            current_round_id,
        )?;

        per_tranche_info.push(tranche_info)
    }

    Ok(LockupWithPerTrancheInfo {
        lock_with_power,
        per_tranche_info,
    })
}

// Returns the time-weighted amount of shares locked in the given lock entry in a round with the given end time,
// and using the given lock epoch length.
pub fn get_lock_time_weighted_shares(
    round_lock_power_schedule: &RoundLockPowerSchedule,
    round_end: Timestamp,
    lock_entry: &LockEntryV2,
    lock_epoch_length: u64,
) -> Uint128 {
    if round_end.nanos() > lock_entry.lock_end.nanos() {
        return Uint128::zero();
    }
    let lockup_length = lock_entry.lock_end.nanos() - round_end.nanos();
    scale_lockup_power(
        round_lock_power_schedule,
        lock_epoch_length,
        lockup_length,
        lock_entry.funds.amount,
    )
}

// Returns the remaining rounds the lock entry is locked starting from the current round start
pub fn compute_lock_rounds_remaining(
    current_round_start: u64,
    lock_end: u64,
    round_length: u64,
) -> Result<u64, ContractError> {
    let remaining_time = lock_end.saturating_sub(current_round_start);

    remaining_time
        .checked_div(round_length)
        .ok_or_else(|| ContractError::Std(StdError::generic_err("round_length must be > 0")))
}

pub fn scale_lockup_power(
    round_lock_power_schedule: &RoundLockPowerSchedule,
    lock_epoch_length: u64,
    lockup_time: u64,
    raw_power: Uint128,
) -> Uint128 {
    for entry in round_lock_power_schedule.round_lock_power_schedule.iter() {
        let needed_lock_time = entry.locked_rounds * lock_epoch_length;
        if lockup_time <= needed_lock_time {
            let power = entry
                .power_scaling_factor
                .saturating_mul(Decimal::from_ratio(raw_power, Uint128::one()));
            return power.to_uint_floor();
        }
    }

    // if lockup time is longer than the longest lock time, return the maximum power
    let largest_multiplier = round_lock_power_schedule
        .round_lock_power_schedule
        .last()
        .unwrap()
        .power_scaling_factor;
    largest_multiplier
        .saturating_mul(Decimal::new(raw_power))
        .to_uint_floor()
}

pub struct LockingInfo {
    pub lock_in_public_cap: Option<u128>,
    pub lock_in_known_users_cap: Option<u128>,
}

/// Check if sender owns the specified lock
pub fn is_lock_owner(storage: &dyn Storage, sender: &Addr, lock_id: u64) -> bool {
    match LOCKS_MAP_V2.may_load(storage, lock_id) {
        Ok(Some(lock_entry)) => lock_entry.owner == *sender,
        _ => false,
    }
}

// Finds the last proposal the given lock has voted for.
// It will return an error if the lock has not voted for any proposal.
pub fn find_voted_proposal_for_lock(
    deps: &Deps,
    current_round_id: u64,
    tranche_id: u64,
    lock_id: u64,
) -> Result<Proposal, ContractError> {
    if current_round_id == 0 {
        return Err(ContractError::Std(StdError::generic_err(
            "Cannot find proposal for lock in round 0.",
        )));
    }

    let mut check_round = current_round_id - 1;
    loop {
        if let Some(prev_vote) = get_lock_vote(deps.storage, check_round, tranche_id, lock_id)? {
            // Found a vote, so get and return the proposal
            return PROPOSAL_MAP
                .load(deps.storage, (check_round, tranche_id, prev_vote.prop_id))
                .map_err(ContractError::Std);
        }
        // If we reached the beginning of the tranche, there is an error
        if check_round == 0 {
            return Err(ContractError::Std(StdError::generic_err(format!(
                "Could not find previous vote for lock_id {lock_id} in tranche {tranche_id}.",
            ))));
        }

        check_round -= 1;
    }
}

// Gets the deployment for a given proposal if it exists
pub fn get_deployment_for_proposal(
    deps: &Deps,
    proposal: &Proposal,
) -> Result<Option<LiquidityDeployment>, ContractError> {
    LIQUIDITY_DEPLOYMENTS_MAP
        .may_load(
            deps.storage,
            (proposal.round_id, proposal.tranche_id, proposal.proposal_id),
        )
        .map_err(|e| {
            ContractError::Std(StdError::generic_err(format!(
                "Could not read deployment store for proposal {} in tranche {} and round {}: {}",
                proposal.proposal_id, proposal.tranche_id, proposal.round_id, e
            )))
        })
}

// Finds the deployment for the last proposal the given lock has voted for.
pub fn find_deployment_for_voted_lock(
    deps: &Deps,
    current_round_id: u64,
    tranche_id: u64,
    lock_id: u64,
) -> Result<Option<LiquidityDeployment>, ContractError> {
    let proposal = find_voted_proposal_for_lock(deps, current_round_id, tranche_id, lock_id)?;
    get_deployment_for_proposal(deps, &proposal)
}

impl LiquidityDeployment {
    pub fn has_nonzero_funds(&self) -> bool {
        !self.deployed_funds.is_empty()
            && self
                .deployed_funds
                .iter()
                .any(|coin| coin.amount > Uint128::zero())
    }
}

/// Retrieves a lock entry by its ID and ensures the lock_owner is the actual owner.
///
/// # Arguments
/// - `lock_owner` - The expected owner of the lock entry.
/// - `lock_id` - The unique identifier of the lock entry.
///
/// # Returns
/// - `Ok(LockEntryV2)` - If the lock entry exists and the caller is the owner.
/// - `Err(ContractError::Unauthorized)` - If the caller is not the owner.
/// - `Err(ContractError::Std(_))` - If the lock entry does not exist or another storage error occurs.
pub fn get_owned_lock_entry(
    storage: &dyn Storage,
    lock_owner: &Addr,
    lock_id: u64,
) -> Result<LockEntryV2, ContractError> {
    let lock_entry = LOCKS_MAP_V2.load(storage, lock_id)?;

    if lock_entry.owner != lock_owner {
        return Err(ContractError::Unauthorized);
    }

    Ok(lock_entry)
}

/// Helper function to get user's lock IDs (can be used to claim tributes)
pub fn get_user_claimable_locks(storage: &dyn Storage, user_addr: Addr) -> StdResult<Vec<u64>> {
    let user_lock_ids = USER_LOCKS_FOR_CLAIM
        .may_load(storage, user_addr)?
        .unwrap_or_default();
    Ok(user_lock_ids)
}

/// Helper function to calculate vote power for a vote
pub fn calculate_vote_power(
    token_manager: &mut TokenManager,
    deps: &Deps,
    round_id: u64,
    vote: &Vote,
) -> StdResult<Decimal> {
    let vote_token_group_id = vote.time_weighted_shares.0.clone();
    let token_ratio = token_manager.get_token_group_ratio(deps, round_id, vote_token_group_id)?;

    let vote_power = vote.time_weighted_shares.1.checked_mul(token_ratio)?;

    Ok(vote_power)
}

/// Helper function to get vote for a specific lock in a round/tranche
pub fn get_lock_vote(
    storage: &dyn Storage,
    round_id: u64,
    tranche_id: u64,
    lock_id: u64,
) -> StdResult<Option<Vote>> {
    VOTE_MAP_V2.may_load(storage, ((round_id, tranche_id), lock_id))
}

pub fn get_proposal(
    storage: &dyn Storage,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> StdResult<Proposal> {
    PROPOSAL_MAP.load(storage, (round_id, tranche_id, proposal_id))
}

pub struct LockVotingAllowedRound {
    pub lock_id: u64,
    pub tranche_id: u64,
    pub voting_allowed_round: u64,
}

pub fn get_higest_voting_allowed_round(
    deps: &Deps,
    tranche_id: u64,
    lock_ids: &HashSet<u64>,
) -> Result<Option<LockVotingAllowedRound>, ContractError> {
    let mut highest_voting_allowed_round: Option<LockVotingAllowedRound> = None;

    for lock_id in lock_ids {
        if let Some(voting_allowed_round) =
            VOTING_ALLOWED_ROUND.may_load(deps.storage, (tranche_id, *lock_id))?
        {
            if let Some(current_highest) = &highest_voting_allowed_round {
                if voting_allowed_round > current_highest.voting_allowed_round {
                    highest_voting_allowed_round = Some(LockVotingAllowedRound {
                        lock_id: *lock_id,
                        tranche_id,
                        voting_allowed_round,
                    });
                }
            } else {
                highest_voting_allowed_round = Some(LockVotingAllowedRound {
                    lock_id: *lock_id,
                    tranche_id,
                    voting_allowed_round,
                });
            }
        }
    }

    Ok(highest_voting_allowed_round)
}

// Retrieves the next lock id and increments the stored value for the next lock id.
pub fn get_next_lock_id(storage: &mut dyn Storage) -> StdResult<u64> {
    let next_lock_id = LOCK_ID.load(storage)?;
    LOCK_ID.save(storage, &(next_lock_id + 1))?;

    Ok(next_lock_id)
}

// Updates `USER_LOCKS` with locks to be added and removed
pub fn update_user_locks(
    storage: &mut dyn Storage,
    env: &Env,
    user_addr: &Addr,
    locks_to_add: Vec<u64>,
    locks_to_remove: Vec<u64>,
) -> StdResult<()> {
    USER_LOCKS.update(
        storage,
        user_addr.clone(),
        env.block.height,
        |current_locks| -> Result<Vec<u64>, StdError> {
            let mut current_locks = current_locks.unwrap_or_default();
            current_locks.extend_from_slice(&locks_to_add);

            let locks_to_remove: HashSet<u64> = HashSet::from_iter(locks_to_remove);
            current_locks.retain(|lock_id| !locks_to_remove.contains(lock_id));

            Ok(current_locks)
        },
    )?;

    Ok(())
}

pub fn increase_available_conversion_funds(
    storage: &mut dyn Storage,
    denom: &str,
    amount: Uint128,
) -> Result<(), ContractError> {
    update_available_conversion_funds(storage, denom, |current_funds| {
        Ok(current_funds.checked_add(amount)?)
    })
}

pub fn decrease_available_conversion_funds(
    storage: &mut dyn Storage,
    denom: &str,
    amount: Uint128,
) -> Result<(), ContractError> {
    update_available_conversion_funds(storage, denom, |current_funds| {
        current_funds.checked_sub(amount)
            .map_err(|_| new_generic_error(
                format!("insufficient funds to perform conversion into denom: {denom}. required funds: {amount}, available funds: {current_funds}")))
    })
}

pub fn update_available_conversion_funds<T>(
    storage: &mut dyn Storage,
    denom: &str,
    compute_new_funds_fn: T,
) -> Result<(), ContractError>
where
    T: FnOnce(Uint128) -> Result<Uint128, ContractError>,
{
    AVAILABLE_CONVERSION_FUNDS.update(
        storage,
        denom.to_string(),
        |current_funds| -> Result<Uint128, ContractError> {
            compute_new_funds_fn(current_funds.unwrap_or_default())
        },
    )?;

    Ok(())
}

/// Converts a slice of items into a comma-separated string of their string representations.
pub fn get_slice_as_attribute<T: ToString>(input: &[T]) -> String {
    input
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<String>>()
        .join(",")
}
