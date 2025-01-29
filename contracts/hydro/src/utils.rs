use cosmwasm_std::{
    Addr, Decimal, Deps, DepsMut, Env, Order, StdError, StdResult, Storage, Timestamp, Uint128,
};
use cw_storage_plus::Bound;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    contract::get_user_voting_power_for_past_round,
    error::{new_generic_error, ContractError},
    lsm_integration::{get_total_power_for_round, initialize_validator_store},
    state::{
        Constants, HeightRange, CONSTANTS, EXTRA_LOCKED_TOKENS_CURRENT_USERS,
        EXTRA_LOCKED_TOKENS_ROUND_TOTAL, HEIGHT_TO_ROUND, LOCKED_TOKENS, ROUND_TO_HEIGHT_RANGE,
    },
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
            deps,
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
    can_user_lock_in_known_users_cap(deps, constants, current_round, sender, amount_to_lock)?;

    Ok(LockingInfo {
        lock_in_public_cap: None,
        lock_in_known_users_cap: Some(amount_to_lock),
    })
}

fn can_user_lock_in_known_users_cap(
    deps: &DepsMut<NeutronQuery>,
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
        get_user_voting_power_for_past_round(
            &deps.as_ref(),
            constants,
            sender.clone(),
            round_to_check,
        )?,
        Uint128::one(),
    );
    let total_voting_power = get_total_power_for_round(deps.as_ref(), round_to_check)?;

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
    locking_info: LockingInfo,
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

pub struct LockingInfo {
    pub lock_in_public_cap: Option<u128>,
    pub lock_in_known_users_cap: Option<u128>,
}
