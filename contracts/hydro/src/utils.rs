use cosmwasm_std::{
    Addr, Decimal, Deps, DepsMut, Env, Order, StdError, StdResult, Storage, Timestamp, Uint128,
};
use cw_storage_plus::Bound;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    contract::{compute_current_round_id, compute_round_end},
    error::{new_generic_error, ContractError},
    lsm_integration::initialize_validator_store,
    msg::LiquidityDeployment,
    query::LockEntryWithPower,
    score_keeper::get_total_power_for_round,
    state::{
        Constants, HeightRange, LockEntry, Proposal, RoundLockPowerSchedule, CONSTANTS,
        EXTRA_LOCKED_TOKENS_CURRENT_USERS, EXTRA_LOCKED_TOKENS_ROUND_TOTAL, HEIGHT_TO_ROUND,
        LIQUIDITY_DEPLOYMENTS_MAP, LOCKED_TOKENS, LOCKS_MAP, PROPOSAL_MAP, ROUND_TO_HEIGHT_RANGE,
        SNAPSHOTS_ACTIVATION_HEIGHT, USER_LOCKS, VOTE_MAP,
    },
    token_manager::TokenManager,
};

/// Loads the constants that are active for the current block according to the block timestamp.
pub fn load_current_constants(deps: &Deps<NeutronQuery>, env: &Env) -> StdResult<Constants> {
    Ok(load_constants_active_at_timestamp(deps, env.block.time)?.1)
}

/// Loads the constants that were active at the given timestamp. Returns both the Constants and
/// their activation timestamp.
pub fn load_constants_active_at_timestamp(
    deps: &Deps<NeutronQuery>,
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
        .filter_map(|constants| match constants {
            Ok(constants) => Some(constants),
            Err(_) => None,
        })
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
    deps: &DepsMut<NeutronQuery>,
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
    deps: &Deps<NeutronQuery>,
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
            "Can not lock {} tokens in known users cap. User had zero voting power in previous round.",
            amount_to_lock
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
            "Can not lock {} tokens in known users cap. User reached the personal cap for locking tokens in the known users cap.",
            amount_to_lock
        )));
    }

    Ok(())
}

// Whenever a users locks more tokens this function will update the necessary stores,
// depending on the amounts that user locked in public_cap and known_users_cap.
// Stores that will (potentially) be updated:
//      LOCKED_TOKENS, EXTRA_LOCKED_TOKENS_ROUND_TOTAL, EXTRA_LOCKED_TOKENS_CURRENT_USERS
pub fn update_locked_tokens_info(
    deps: &mut DepsMut<NeutronQuery>,
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
    initialize_validator_store(storage, round_id)?;
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
                "Failed to load round ID for height {}.",
                height
            )));
        }
    })
}

fn get_highest_known_height_for_round_id(storage: &dyn Storage, round_id: u64) -> StdResult<u64> {
    Ok(ROUND_TO_HEIGHT_RANGE
        .may_load(storage, round_id)?
        .unwrap_or_default()
        .highest_known_height)
}

pub fn verify_historical_data_availability(storage: &dyn Storage, height: u64) -> StdResult<()> {
    let snapshot_activation_height = SNAPSHOTS_ACTIVATION_HEIGHT.load(storage)?;
    if height < snapshot_activation_height {
        return Err(StdError::generic_err(format!(
            "Historical data not available before height: {}. Height requested: {}",
            snapshot_activation_height, height,
        )));
    }

    Ok(())
}

pub fn get_current_user_voting_power(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    address: Addr,
) -> StdResult<u128> {
    let constants = load_current_constants(deps, env)?;
    let current_round_id = compute_current_round_id(env, &constants)?;
    let round_end = compute_round_end(&constants, current_round_id)?;
    let mut token_manager = TokenManager::new(deps);

    Ok(LOCKS_MAP
        .prefix(address)
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|lockup| match lockup {
            Err(_) => None,
            Ok(lockup) => Some(
                to_lockup_with_power(
                    deps,
                    &constants,
                    &mut token_manager,
                    current_round_id,
                    round_end,
                    lockup.1,
                )
                .current_voting_power
                .u128(),
            ),
        })
        .sum())
}

/// Utility function intended to be used by get_user_voting_power_for_past_round() and get_user_voting_power_for_past_height().
/// Both of these functions will ensure that the provided height indeed matches the given round, and vice versa.
/// If the function is used in different context, the caller is responsible for ensuring this condition is satisifed.
fn get_past_user_voting_power(
    deps: &Deps<NeutronQuery>,
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
            LOCKS_MAP
                .may_load_at_height(deps.storage, (address.clone(), lock_id), height)
                .unwrap_or_default()
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
    deps: &Deps<NeutronQuery>,
    constants: &Constants,
    address: Addr,
    round_id: u64,
) -> StdResult<u128> {
    let height = get_highest_known_height_for_round_id(deps.storage, round_id)?;
    verify_historical_data_availability(deps.storage, height)?;
    get_past_user_voting_power(deps, constants, address, height, round_id)
}

pub fn get_user_voting_power_for_past_height(
    deps: &Deps<NeutronQuery>,
    constants: &Constants,
    address: Addr,
    height: u64,
) -> StdResult<u128> {
    let round_id = get_round_id_for_height(deps.storage, height)?;
    get_past_user_voting_power(deps, constants, address, height, round_id)
}

pub fn to_lockup_with_power(
    deps: &Deps<NeutronQuery>,
    constants: &Constants,
    token_manager: &mut TokenManager,
    round_id: u64,
    round_end: Timestamp,
    lock_entry: LockEntry,
) -> LockEntryWithPower {
    match token_manager.validate_denom(deps, round_id, lock_entry.funds.denom.clone()) {
        Err(_) => {
            // If we fail to resove the denom, then this lockup has zero voting power.
            LockEntryWithPower {
                lock_entry,
                current_voting_power: Uint128::zero(),
            }
        }
        Ok(token_group_id) => {
            match token_manager.get_token_group_ratio(deps, round_id, token_group_id) {
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
                Ok(token_ratio) => {
                    let time_weighted_shares = get_lock_time_weighted_shares(
                        &constants.round_lock_power_schedule,
                        round_end,
                        &lock_entry,
                        constants.lock_epoch_length,
                    );

                    let current_voting_power = token_ratio
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

// Returns the time-weighted amount of shares locked in the given lock entry in a round with the given end time,
// and using the given lock epoch length.
pub fn get_lock_time_weighted_shares(
    round_lock_power_schedule: &RoundLockPowerSchedule,
    round_end: Timestamp,
    lock_entry: &LockEntry,
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

// Finds the last proposal the given lock has voted for.
// It will return an error if the lock has not voted for any proposal.
pub fn find_voted_proposal_for_lock(
    deps: &Deps<NeutronQuery>,
    current_round_id: u64,
    tranche_id: u64,
    lock_voter: &Addr,
    lock_id: u64,
) -> Result<Proposal, ContractError> {
    if current_round_id == 0 {
        return Err(ContractError::Std(StdError::generic_err(
            "Cannot find proposal for lock in round 0.",
        )));
    }

    let mut check_round = current_round_id - 1;
    loop {
        if let Some(prev_vote) = VOTE_MAP.may_load(
            deps.storage,
            ((check_round, tranche_id), lock_voter.clone(), lock_id),
        )? {
            // Found a vote, so get and return the proposal
            return PROPOSAL_MAP
                .load(deps.storage, (check_round, tranche_id, prev_vote.prop_id))
                .map_err(ContractError::Std);
        }
        // If we reached the beginning of the tranche, there is an error
        if check_round == 0 {
            return Err(ContractError::Std(StdError::generic_err(format!(
                "Could not find previous vote for lock_id {} in tranche {}.",
                lock_id, tranche_id,
            ))));
        }

        check_round -= 1;
    }
}

// Gets the deployment for a given proposal if it exists
pub fn get_deployment_for_proposal(
    deps: &Deps<NeutronQuery>,
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
    deps: &Deps<NeutronQuery>,
    current_round_id: u64,
    tranche_id: u64,
    lock_voter: &Addr,
    lock_id: u64,
) -> Result<Option<LiquidityDeployment>, ContractError> {
    let proposal =
        find_voted_proposal_for_lock(deps, current_round_id, tranche_id, lock_voter, lock_id)?;
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
