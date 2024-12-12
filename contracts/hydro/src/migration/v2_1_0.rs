use std::{collections::HashMap, str::FromStr};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, DepsMut, Env, Order, Storage, Timestamp};
use cw_storage_plus::Item;
use neutron_sdk::bindings::query::NeutronQuery;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    contract::compute_current_round_id,
    error::ContractError,
    state::{
        RoundLockPowerSchedule, CONSTANTS, PROPOSAL_MAP, TRANCHE_MAP, VOTE_MAP,
        VOTING_ALLOWED_ROUND,
    },
};

// Message to migrate the contract from v2.0.4 to v2.1.0
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsgV2_1_0 {}

#[cw_serde]
pub struct ConstantsV2_0_4 {
    pub round_length: u64,
    pub lock_epoch_length: u64,
    pub first_round_start: Timestamp,
    pub max_locked_tokens: u128,
    pub max_validator_shares_participating: u64,
    pub hub_connection_id: String,
    pub hub_transfer_channel_id: String,
    pub icq_update_period: u64,
    pub paused: bool,
    pub is_in_pilot_mode: bool,
    pub max_deployment_duration: u64,
}

#[cw_serde]
pub struct ConstantsV2_1_0 {
    pub round_length: u64,
    pub lock_epoch_length: u64,
    pub first_round_start: Timestamp,
    pub max_locked_tokens: u128,
    pub max_validator_shares_participating: u64,
    pub hub_connection_id: String,
    pub hub_transfer_channel_id: String,
    pub icq_update_period: u64,
    pub paused: bool,
    pub max_deployment_duration: u64,
    pub round_lock_power_schedule: RoundLockPowerSchedule,
}

impl ConstantsV2_1_0 {
    pub fn from(old_constants: ConstantsV2_0_4) -> Self {
        Self {
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
            round_lock_power_schedule: RoundLockPowerSchedule::new(get_round_2_power_schedule()),
        }
    }
}

pub fn get_round_2_power_schedule() -> Vec<(u64, Decimal)> {
    vec![
        (1, Decimal::from_str("1").unwrap()),
        (2, Decimal::from_str("1.25").unwrap()),
        (3, Decimal::from_str("1.5").unwrap()),
    ]
}

pub fn migrate_v2_0_4_to_v2_1_0(
    deps: &mut DepsMut<NeutronQuery>,
    env: Env,
    _msg: MigrateMsgV2_1_0,
) -> Result<(), ContractError> {
    migrate_constants(deps.storage)?;
    migrate_voting_allowed_info(deps, &env)?;

    Ok(())
}

fn migrate_constants(storage: &mut dyn Storage) -> Result<(), ContractError> {
    const OLD_CONSTANTS: Item<ConstantsV2_0_4> = Item::new("constants");
    const NEW_CONSTANTS: Item<ConstantsV2_1_0> = Item::new("constants");

    let old_constants = OLD_CONSTANTS.load(storage)?;
    let new_constants = ConstantsV2_1_0::from(old_constants);
    NEW_CONSTANTS.save(storage, &new_constants)?;

    Ok(())
}

fn migrate_voting_allowed_info(
    deps: &mut DepsMut<NeutronQuery>,
    env: &Env,
) -> Result<(), ContractError> {
    // migrate_constants() must be executed first
    let constants = CONSTANTS.load(deps.storage)?;

    let tranche_ids: Vec<u64> = TRANCHE_MAP
        .keys(deps.storage, None, None, Order::Ascending)
        .filter_map(|tranche_id| match tranche_id {
            Ok(tranche_id) => Some(tranche_id),
            Err(_) => None,
        })
        .collect();

    // don't rely on migration being run in round 1, even though it probably will
    let current_round_id = compute_current_round_id(env, &constants)?;

    // to cache proposal deployment durations once we load them; saves some gas
    let mut deployment_durations: HashMap<u64, u64> = HashMap::new();

    // no need to populate this info for round 0 since we only had 1-round-long deployment proposals
    for round_id in 1..=current_round_id {
        for &tranche_id in tranche_ids.iter() {
            let votes: Vec<VoteMigrationInfo> = VOTE_MAP
                .sub_prefix((round_id, tranche_id))
                .range(deps.storage, None, None, Order::Ascending)
                .filter_map(|vote| match vote {
                    Err(_) => None,
                    Ok(vote) => Some(VoteMigrationInfo {
                        lock_id: vote.0 .1,
                        proposal_id: vote.1.prop_id,
                    }),
                })
                .collect();

            for vote in votes {
                if VOTING_ALLOWED_ROUND
                    .may_load(deps.storage, (tranche_id, vote.lock_id))?
                    .is_some()
                {
                    continue;
                }

                let deployment_duration = match deployment_durations.get(&vote.proposal_id) {
                    Some(deployment_duration) => *deployment_duration,
                    None => {
                        let proposal = PROPOSAL_MAP
                            .load(deps.storage, (round_id, tranche_id, vote.proposal_id))?;
                        deployment_durations
                            .insert(proposal.proposal_id, proposal.deployment_duration);

                        proposal.deployment_duration
                    }
                };

                let voting_allowed_round = round_id + deployment_duration;
                VOTING_ALLOWED_ROUND.save(
                    deps.storage,
                    (tranche_id, vote.lock_id),
                    &voting_allowed_round,
                )?;
            }
        }
    }

    Ok(())
}

pub struct VoteMigrationInfo {
    pub lock_id: u64,
    pub proposal_id: u64,
}
