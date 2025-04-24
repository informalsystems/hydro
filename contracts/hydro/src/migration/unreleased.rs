use cosmwasm_std::{Deps, DepsMut, Order, Response, StdResult};
use cw_storage_plus::Map;
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

use crate::{
    error::{new_generic_error, ContractError},
    migration::v3_1_1::ConstantsV3_1_1,
    state::{
        Constants, LockEntryV2, CONSTANTS, LOCKS_MAP_V1, LOCKS_MAP_V2, TOKEN_INFO_PROVIDERS,
        VOTE_MAP_V1, VOTE_MAP_V2,
    },
    token_manager::{TokenInfoProvider, TokenInfoProviderLSM, LSM_TOKEN_INFO_PROVIDER_ID},
};

pub fn migrate_v3_1_1_to_unreleased(
    deps: &mut DepsMut<NeutronQuery>,
) -> Result<Response<NeutronMsg>, ContractError> {
    const OLD_CONSTANTS: Map<u64, ConstantsV3_1_1> = Map::new("constants");

    let old_constants = OLD_CONSTANTS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|result| match result {
            Err(_) => None,
            Ok(constants) => Some(constants),
        })
        .collect::<Vec<(u64, ConstantsV3_1_1)>>();

    let mut constants_to_add = vec![];
    let mut latest_constants: Option<ConstantsV3_1_1> = None;

    for constants_tuple in old_constants {
        latest_constants = Some(constants_tuple.1.clone());

        let new_constants = Constants {
            round_length: constants_tuple.1.round_length,
            lock_epoch_length: constants_tuple.1.lock_epoch_length,
            first_round_start: constants_tuple.1.first_round_start,
            max_locked_tokens: constants_tuple.1.max_locked_tokens,
            known_users_cap: constants_tuple.1.known_users_cap,
            paused: constants_tuple.1.paused,
            max_deployment_duration: constants_tuple.1.max_deployment_duration,
            round_lock_power_schedule: constants_tuple.1.round_lock_power_schedule,
        };

        constants_to_add.push((constants_tuple.0, new_constants));
    }

    if latest_constants.is_none() {
        return Err(new_generic_error(
            "Couldn't find any Constants in the store.",
        ));
    }

    for constants_to_add in constants_to_add {
        // No need to remove the old ones since we are rewriting all values under the same keys
        CONSTANTS.save(deps.storage, constants_to_add.0, &constants_to_add.1)?;
    }

    let latest_constants = latest_constants.unwrap();
    let lsm_token_info_provider = TokenInfoProviderLSM {
        hub_connection_id: latest_constants.hub_connection_id,
        hub_transfer_channel_id: latest_constants.hub_transfer_channel_id,
        icq_update_period: latest_constants.icq_update_period,
        max_validator_shares_participating: latest_constants.max_validator_shares_participating,
    };

    TOKEN_INFO_PROVIDERS.save(
        deps.storage,
        LSM_TOKEN_INFO_PROVIDER_ID.to_string(),
        &TokenInfoProvider::LSM(lsm_token_info_provider),
    )?;

    Ok(Response::new().add_attribute("action", "migrate_v3_1_1_to_unreleased"))
}

// Migrate locks from V1 to V2 storage structures
// This function prepares our locks storage for NFT features by removing the address from the storage keys
pub fn migrate_locks_batch(
    deps: &mut DepsMut<NeutronQuery>,
    current_height: u64,
    start: usize,
    limit: usize,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Get locks from LOCKS_MAP_V1
    // We use the range method to get a slice of the locks
    let locks = LOCKS_MAP_V1
        .range(deps.storage, None, None, Order::Ascending)
        .skip(start)
        .take(limit)
        .collect::<StdResult<Vec<_>>>()?;

    let mut total_locks_migrated = 0;

    // For each lock entry, create an entry in LOCKS_MAP_V2
    for ((addr, lock_id), entry) in locks {
        let lock_entry_v2 = LockEntryV2 {
            lock_id: entry.lock_id,
            owner: addr.clone(),
            funds: entry.funds.clone(),
            lock_start: entry.lock_start,
            lock_end: entry.lock_end,
        };

        // Save to LOCKS_MAP_V2 with just lock_id as key
        LOCKS_MAP_V2.save(deps.storage, lock_id, &lock_entry_v2, current_height)?;
        total_locks_migrated += 1;
    }

    Ok(Response::new()
        .add_attribute("action", "migrate_locks_batch")
        .add_attribute("start", start.to_string())
        .add_attribute("limit", limit.to_string())
        .add_attribute("locks_migrated", total_locks_migrated.to_string()))
}

// Migrate votes from V1 to V2 storage structures
// This function prepares our votes storage for NFT features by removing the address from the storage keys
pub fn migrate_votes_batch(
    deps: &mut DepsMut<NeutronQuery>,
    start: usize,
    limit: usize,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Get votes from VOTE_MAP_V1
    let votes = VOTE_MAP_V1
        .range(deps.storage, None, None, Order::Ascending)
        .skip(start)
        .take(limit)
        .collect::<StdResult<Vec<_>>>()?;

    let mut total_votes_migrated = 0;

    for (((round_id, tranche_id), _addr, lock_id), vote) in votes {
        // Save to new VOTE_MAP_V2 with lock_id as key instead of (addr, lock_id)
        VOTE_MAP_V2.save(deps.storage, ((round_id, tranche_id), lock_id), &vote)?;
        total_votes_migrated += 1;
    }

    Ok(Response::new()
        .add_attribute("action", "migrate_votes_batch")
        .add_attribute("start", start.to_string())
        .add_attribute("limit", limit.to_string())
        .add_attribute("votes_migrated", total_votes_migrated.to_string()))
}

// Check if the migration is done
// This function checks if all locks and votes from V1 have been migrated to V2
// by checking if each lock and vote key exists in the new map.
// It returns true if the migration is done, false otherwise
pub fn is_full_migration_done(deps: Deps<NeutronQuery>) -> Result<bool, ContractError> {
    // Verify that each lock (by lock_id) exists in the new map
    let old_locks = LOCKS_MAP_V1.keys(deps.storage, None, None, Order::Ascending);

    for item in old_locks {
        let (_addr, lock_id) = item?;
        let exists = LOCKS_MAP_V2.key(lock_id).has(deps.storage);

        if !exists {
            return Ok(false);
        }
    }

    // Verify that each vote (by lock_id) exists in the new map
    let old_votes = VOTE_MAP_V1.keys(deps.storage, None, None, Order::Ascending);

    for item in old_votes {
        let ((round_id, tranche_id), _addr, lock_id) = item?;
        let exists = VOTE_MAP_V2
            .key(((round_id, tranche_id), lock_id))
            .has(deps.storage);

        if !exists {
            return Ok(false);
        }
    }

    Ok(true)
}
