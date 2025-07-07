use crate::error::{new_generic_error, ContractError};
use crate::migration::v3_2_0::ConstantsV3_2_0;
use crate::msg::CollectionInfo;
use crate::state::{Constants, CONSTANTS, USER_LOCKS, USER_LOCKS_FOR_CLAIM, VOTE_MAP_V1};

use cosmwasm_std::{Addr, DepsMut, Order, Response, StdResult};
use cw_storage_plus::Map;
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};
use std::collections::{HashMap, HashSet};

pub fn migrate_v3_2_0_to_v3_4_1(
    deps: &mut DepsMut<NeutronQuery>,
) -> Result<Response<NeutronMsg>, ContractError> {
    migrate_constants(deps)?;
    populate_user_locks_for_claim(deps)?;

    Ok(Response::new().add_attribute("action", "migrate_v3_2_0_to_v3_4_1"))
}

pub fn migrate_constants(deps: &mut DepsMut<NeutronQuery>) -> Result<(), ContractError> {
    const OLD_CONSTANTS: Map<u64, ConstantsV3_2_0> = Map::new("constants");

    let old_constants_entries = OLD_CONSTANTS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|result| result.ok())
        .collect::<Vec<(u64, ConstantsV3_2_0)>>();

    // Check if there are any constants to migrate
    if old_constants_entries.is_empty() {
        return Err(new_generic_error(
            "Couldn't find any Constants in the store.",
        ));
    }

    let mut updated_constants_entries = vec![];

    for (timestamp, old_constants) in old_constants_entries {
        let new_constants = Constants {
            round_length: old_constants.round_length,
            lock_epoch_length: old_constants.lock_epoch_length,
            first_round_start: old_constants.first_round_start,
            max_locked_tokens: old_constants.max_locked_tokens,
            known_users_cap: old_constants.known_users_cap,
            paused: old_constants.paused,
            max_deployment_duration: old_constants.max_deployment_duration,
            round_lock_power_schedule: old_constants.round_lock_power_schedule,
            cw721_collection_info: CollectionInfo {
                name: "Hydro Lockups".to_string(),
                symbol: "hydro-lockups".to_string(),
            },
        };

        updated_constants_entries.push((timestamp, new_constants));
    }

    // No need to remove the old entries since we are rewriting all values under the same keys
    for (timestamp, new_constants) in &updated_constants_entries {
        CONSTANTS.save(deps.storage, *timestamp, new_constants)?;
    }

    Ok(())
}

// Populates USER_LOCKS_FOR_CLAIM by combining historical voting data (VOTE_MAP_V1)
// with current lock ownership (USER_LOCKS)
fn populate_user_locks_for_claim(deps: &mut DepsMut<NeutronQuery>) -> Result<(), ContractError> {
    // Track all users who have locks they can claim tributes for
    let mut user_claim_locks: HashMap<Addr, HashSet<u64>> = HashMap::new();

    // 1. Add historical voting data from VOTE_MAP_V1
    // Users can claim tributes for locks they voted with (as they have not yet been transferred)
    //
    // Skip iterating over VOTE_MAP_V2 during this upgrade because:
    // - This upgrade runs soon after VOTE_MAP_V2 is introduced
    // - VOTE_MAP_V1 provides easier access to the owner information we need
    // - Any data in VOTE_MAP_V2 not in VOTE_MAP_V1 should also be in USER_LOCKS
    let vote_map_iter = VOTE_MAP_V1
        .range(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    for ((_round_tranche, user_addr, lock_id), _vote) in vote_map_iter {
        let entry = user_claim_locks.entry(user_addr).or_default();
        entry.insert(lock_id);
    }

    // 2. Add current lock ownership from USER_LOCKS
    // Users can claim tributes for locks they currently own
    let user_locks_iter = USER_LOCKS
        .range(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    for (user_addr, lock_ids) in user_locks_iter {
        let entry = user_claim_locks.entry(user_addr).or_default();
        for lock_id in lock_ids {
            entry.insert(lock_id);
        }
    }

    // 3. Save the aggregated data to USER_LOCKS_FOR_CLAIM
    for (user_addr, lock_ids) in user_claim_locks {
        let lock_ids_vec: Vec<u64> = lock_ids.into_iter().collect();
        USER_LOCKS_FOR_CLAIM.save(deps.storage, user_addr, &lock_ids_vec)?;
    }

    Ok(())
}
