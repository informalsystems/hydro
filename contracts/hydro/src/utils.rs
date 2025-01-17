use cosmwasm_std::{
    Addr, Decimal, Deps, DepsMut, Env, Order, StdError, StdResult, Timestamp, Uint128,
};
use cw_storage_plus::Bound;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    contract::get_user_voting_power,
    error::ContractError,
    lsm_integration::get_total_power_for_round,
    state::{
        Constants, CONSTANTS, EXTRA_LOCKED_TOKENS_CURRENT_USERS, EXTRA_LOCKED_TOKENS_ROUND_TOTAL,
        LOCKED_TOKENS, LOCKS_MAP,
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

// TODO: add desc
pub fn validate_locked_tokens_caps(
    deps: &DepsMut<NeutronQuery>,
    env: &Env,
    constants: &Constants,
    current_round: u64,
    sender: &Addr,
    total_locked_tokens: u128,
    amount_to_lock: u128,
) -> Result<LockingInfo, ContractError> {
    let lock_limit_reached_error = Err(ContractError::Std(StdError::generic_err(
        "The limit for locking tokens has been reached. No more tokens can be locked.",
    )));

    // Regardless of public_cap and extra_cap, we must make sure that nobody can lock more than total_cap
    let total_locked_after = total_locked_tokens + amount_to_lock;
    if total_locked_after > constants.max_locked_tokens {
        return lock_limit_reached_error;
    }

    let public_cap = constants.max_locked_tokens - constants.current_users_extra_cap;

    // This branch will be executed in one of the following cases:
    //   1) constants.current_users_extra_cap != 0 and there is SOME room in the public cap
    //   2) constants.current_users_extra_cap == 0 and there is ENOUGH room in the public_cap.
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

        // Lock as much as possible in the public_cap, and the rest in the extra_cap
        let lock_in_public_cap = public_cap - total_locked_tokens;
        let lock_in_known_users_cap = amount_to_lock - lock_in_public_cap;

        // If there is still room in extra_cap, then check if this
        // is a user that should be allowed to use the extra_cap.
        if !can_user_lock_in_extra_cap(
            deps,
            env,
            constants,
            current_round,
            sender,
            lock_in_known_users_cap,
        )? {
            return lock_limit_reached_error;
        }

        return Ok(LockingInfo {
            lock_in_public_cap: Some(lock_in_public_cap),
            lock_in_known_users_cap: Some(lock_in_known_users_cap),
        });
    }

    // If we got through here, it means that constants.current_users_extra_cap > 0.
    // If constants.current_users_extra_cap was 0 and public_cap (equal to total_cap)
    // is equal to total_locked_tokens, then we would error out at the start of this
    // function, because any amount_to_lock would exceed the total_cap. This is just
    // a safety check to make the code resilient to future changes.
    if constants.current_users_extra_cap == 0 {
        return lock_limit_reached_error;
    }

    // If there is still room in extra_cap, then check if this
    // is a user that should be allowed to use the extra_cap.
    if !can_user_lock_in_extra_cap(deps, env, constants, current_round, sender, amount_to_lock)? {
        return lock_limit_reached_error;
    }

    Ok(LockingInfo {
        lock_in_public_cap: None,
        lock_in_known_users_cap: Some(amount_to_lock),
    })
}

fn can_user_lock_in_extra_cap(
    deps: &DepsMut<NeutronQuery>,
    env: &Env,
    constants: &Constants,
    current_round: u64,
    sender: &Addr,
    amount_to_lock: u128,
) -> Result<bool, ContractError> {
    let extra_locked_tokens_round_total = EXTRA_LOCKED_TOKENS_ROUND_TOTAL
        .may_load(deps.storage, current_round)?
        .unwrap_or(0);

    if extra_locked_tokens_round_total + amount_to_lock > constants.current_users_extra_cap {
        return Ok(false);
    }

    let user_has_lockups = LOCKS_MAP
        .prefix(sender.clone())
        .range(deps.storage, None, None, Order::Ascending)
        .count()
        > 0;

    if !user_has_lockups {
        return Ok(false);
    }

    // Calculate user's voting power share in the total voting power
    let users_voting_power = Decimal::from_ratio(
        get_user_voting_power(&deps.as_ref(), env, sender.clone())?,
        Uint128::one(),
    );
    let total_voting_power = get_total_power_for_round(deps.as_ref(), current_round)?;
    let users_voting_power_share = users_voting_power.checked_div(total_voting_power)?;

    // Calculate what would be users share of extra locked tokens in the maximum allowed extra locked tokens
    let users_current_extra_lock = EXTRA_LOCKED_TOKENS_CURRENT_USERS
        .may_load(deps.storage, (current_round, sender.clone()))?
        .unwrap_or(0);

    let users_extra_lock =
        Decimal::from_ratio(users_current_extra_lock + amount_to_lock, Uint128::one());
    let maximum_extra_lock = Decimal::from_ratio(constants.current_users_extra_cap, Uint128::one());
    let users_extra_lock_share = users_extra_lock.checked_div(maximum_extra_lock)?;

    // If users share in maximum allowed extra cap would be greater than its share in
    // total voting power, then don't allow this user to lock the given amount of tokens.
    if users_extra_lock_share > users_voting_power_share {
        return Ok(false);
    }

    Ok(true)
}

// TODO: add desc
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

pub struct LockingInfo {
    pub lock_in_public_cap: Option<u128>,
    pub lock_in_known_users_cap: Option<u128>,
}
