use std::collections::HashMap;

use cosmwasm_std::{Addr, Decimal, DepsMut, Env, Order, StdResult};
use cw_storage_plus::{Item, Map};
use neutron_sdk::bindings::query::NeutronQuery;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    contract::compute_current_round_id,
    error::ContractError,
    lsm_integration::load_validators_infos,
    migration::v3_0_0::ConstantsV3_0_0,
    state::{
        Constants, HeightRange, LockEntry, CONSTANTS, HEIGHT_TO_ROUND, LOCKS_MAP,
        ROUND_TO_HEIGHT_RANGE, SCALED_ROUND_POWER_SHARES_MAP, SNAPSHOTS_ACTIVATION_HEIGHT,
        TOTAL_VOTING_POWER_PER_ROUND, USER_LOCKS,
    },
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsgV3_1_0 {}

pub fn migrate_v3_0_0_to_v3_1_0(
    deps: &mut DepsMut<NeutronQuery>,
    env: Env,
) -> Result<(), ContractError> {
    let constants = migrate_constants(deps)?;
    let round_id = compute_current_round_id(&env, &constants)?;

    populate_rounds_total_voting_powers(deps, &env, round_id)?;
    migrate_user_lockups(deps, &env)?;
    populate_round_height_mappings(deps, &env, round_id)?;

    SNAPSHOTS_ACTIVATION_HEIGHT.save(deps.storage, &env.block.height)?;

    Ok(())
}

// Convert CONSTANTS storage from Item to Map and insert single constants instance
// under the timestamp of the first round start time, and set the known_users_cap to zero.
fn migrate_constants(deps: &mut DepsMut<NeutronQuery>) -> StdResult<Constants> {
    const OLD_CONSTANTS: Item<ConstantsV3_0_0> = Item::new("constants");
    let old_constants = OLD_CONSTANTS.load(deps.storage)?;

    let new_constants = Constants {
        round_length: old_constants.round_length,
        lock_epoch_length: old_constants.lock_epoch_length,
        first_round_start: old_constants.first_round_start,
        max_locked_tokens: old_constants.max_locked_tokens,
        max_validator_shares_participating: old_constants.max_validator_shares_participating,
        hub_connection_id: old_constants.hub_connection_id,
        hub_transfer_channel_id: old_constants.hub_transfer_channel_id,
        icq_update_period: old_constants.icq_update_period,
        paused: old_constants.paused,
        max_deployment_duration: old_constants.max_deployment_duration,
        round_lock_power_schedule: old_constants.round_lock_power_schedule,
        known_users_cap: 0, // set the known users cap to 0 during the migration
    };

    OLD_CONSTANTS.remove(deps.storage);
    CONSTANTS.save(
        deps.storage,
        new_constants.first_round_start.nanos(),
        &new_constants,
    )?;

    Ok(new_constants)
}

// Populate round total power starting from round 0 and all the way to the last round
// in which any existing lock gives voting power.
fn populate_rounds_total_voting_powers(
    deps: &mut DepsMut<NeutronQuery>,
    env: &Env,
    current_round_id: u64,
) -> StdResult<()> {
    let current_validator_ratios: HashMap<String, Decimal> =
        load_validators_infos(deps.storage, current_round_id)
            .iter()
            .map(|validator_info| (validator_info.address.clone(), validator_info.power_ratio))
            .collect();

    let mut round_id = 0;
    loop {
        let validator_power_ratios: HashMap<String, Decimal> = if round_id >= current_round_id {
            current_validator_ratios.clone()
        } else {
            load_validators_infos(deps.storage, round_id)
                .iter()
                .map(|validator_info| (validator_info.address.clone(), validator_info.power_ratio))
                .collect()
        };

        let round_validator_shares = SCALED_ROUND_POWER_SHARES_MAP
            .prefix(round_id)
            .range(deps.storage, None, None, Order::Ascending)
            .filter_map(|val_shares| match val_shares {
                Err(_) => None,
                Ok(val_shares) => Some(val_shares),
            })
            .collect::<Vec<(String, Decimal)>>();

        // When we encounter the round with zero shares of any validator, it means that there
        // was no lock entry that would give voting power for the given round, or any subsequent
        // rounds, so we break the loop at that point.
        if round_validator_shares.is_empty() {
            break;
        }

        let round_total_power: Decimal = round_validator_shares
            .iter()
            .map(|validator_shares| {
                validator_power_ratios
                    .get(&validator_shares.0)
                    .map_or_else(Decimal::zero, |power_ratio| {
                        power_ratio * validator_shares.1
                    })
            })
            .sum();

        TOTAL_VOTING_POWER_PER_ROUND.save(
            deps.storage,
            round_id,
            &round_total_power.to_uint_ceil(),
            env.block.height,
        )?;

        round_id += 1;
    }

    Ok(())
}

// Converts the LOCKS_MAP from Map into SnapshotMap and populates USER_LOCKS map.
fn migrate_user_lockups(deps: &mut DepsMut<NeutronQuery>, env: &Env) -> StdResult<()> {
    const OLD_LOCKS_MAP: Map<(Addr, u64), LockEntry> = Map::new("locks_map");

    let mut user_locks_map: HashMap<Addr, Vec<u64>> = HashMap::new();
    let user_lockups: Vec<(Addr, LockEntry)> = OLD_LOCKS_MAP
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|lockup| match lockup {
            Err(_) => None,
            Ok(lockup) => {
                user_locks_map
                    .entry(lockup.0 .0.clone())
                    .and_modify(|user_locks| user_locks.push(lockup.1.lock_id))
                    .or_insert(vec![lockup.1.lock_id]);

                Some((lockup.0 .0, lockup.1))
            }
        })
        .collect();

    for user_lockup in &user_lockups {
        OLD_LOCKS_MAP.remove(deps.storage, (user_lockup.0.clone(), user_lockup.1.lock_id));
    }

    for user_lockup in user_lockups {
        LOCKS_MAP.save(
            deps.storage,
            (user_lockup.0.clone(), user_lockup.1.lock_id),
            &user_lockup.1,
            env.block.height,
        )?;
    }

    for user_locks in user_locks_map {
        USER_LOCKS.save(deps.storage, user_locks.0, &user_locks.1, env.block.height)?;
    }

    Ok(())
}

// Populates ROUND_TO_HEIGHT_RANGE and HEIGHT_TO_ROUND maps
fn populate_round_height_mappings(
    deps: &mut DepsMut<NeutronQuery>,
    env: &Env,
    current_round_id: u64,
) -> StdResult<()> {
    ROUND_TO_HEIGHT_RANGE.save(
        deps.storage,
        current_round_id,
        &HeightRange {
            lowest_known_height: env.block.height,
            highest_known_height: env.block.height,
        },
    )?;

    HEIGHT_TO_ROUND.save(deps.storage, env.block.height, &current_round_id)?;

    Ok(())
}
