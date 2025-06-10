use crate::error::ContractError;
use crate::state::{USER_LOCKS, USER_LOCKS_FOR_CLAIM, VOTE_MAP_V1};

use cosmwasm_std::{Addr, DepsMut, Order, Response, StdResult};
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};
use std::collections::{HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum MigrateMsgV3_3_0 {}

pub fn migrate_v3_3_0_to_v3_4_0(
    deps: &mut DepsMut<NeutronQuery>,
) -> Result<Response<NeutronMsg>, ContractError> {
    populate_user_locks_for_claim(deps)?;

    Ok(Response::new().add_attribute("action", "migrate_v3_3_0_to_v3_4_0"))
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
    // - This upgrade runs immediately after VOTE_MAP_V2 is introduced
    // - Both maps contain identical data at this point
    // - VOTE_MAP_V1 provides easier access to the owner information we need
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
