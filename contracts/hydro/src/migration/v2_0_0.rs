use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, DepsMut, Env, Order, StdError, Storage, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};
use neutron_sdk::bindings::query::NeutronQuery;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::contract::{compute_current_round_id, compute_round_end, get_lock_time_weighted_shares};
use crate::error::ContractError;

use crate::lsm_integration::validate_denom;
use crate::migration::v1_1_0::{ConstantsV1_1_0, ProposalV1_1_0, VoteV1_1_0};
use crate::state::{CONSTANTS, LOCKS_MAP};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsgV2_0_0 {
    pub max_bid_duration: u64,
}

#[cw_serde]
pub struct ConstantsV2_0_0 {
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
    pub max_bid_duration: u64,
}

#[cw_serde]
pub struct ProposalV2_0_0 {
    pub round_id: u64,
    pub tranche_id: u64,
    pub proposal_id: u64,
    pub title: String,
    pub description: String,
    pub power: Uint128,
    pub percentage: Uint128,
    pub bid_duration: u64,
    pub minimum_atom_liquidity_request: Uint128,
}

#[cw_serde]
pub struct VoteV2_0_0 {
    pub prop_id: u64,
    pub time_weighted_shares: (String, Decimal),
}

struct OldVoteInfo {
    pub round_id: u64,
    pub tranche_id: u64,
    pub voter: Addr,
}

struct NewVoteInfo {
    pub round_id: u64,
    pub tranche_id: u64,
    pub voter: Addr,
    pub lock_id: u64,
    pub vote: VoteV2_0_0,
}

// Migrating from 1.1.1 to 2.0.0 will:
// - Migrate the existing Constants to add "max_bid_duration" field
// - Migrate the existing Proposals to add "bid_duration" and "minimum_atom_liquidity_request" fields
// - Migrate each Vote from first round to a new format where the key will also include lock_id, and the value
//   will no longer contain HashMap<String, Decimal> but only (String, Decimal), since each vote refers to
//   a single lock entry, and therefore has only one LSM token denom associated with it. To construct new votes,
//   we iterate over all lock entries that belong to a user and create a vote for each lock entry.
//   All votes saved under the old keys are removed from the store, and the replacing votes are added
//   under the new keys.
//
// Note that this migration will only work properly if the contract is currently within the first round.
// The reason is that migration is relaying on helper function validate_denom() that calculates the current
// round ID during its execution. It then queries the validator info for the given round to determine if that
// validator was among the top N, and only then such lockup would have voting power. If we were to migrate
// votes from multiple rounds, it could happen that validator was in top N in round 0, and then droped from the
// top N in round 1. This would cause the lockup from round 0 to be interpreted as one having 0 power,
// which wasn't the case.
pub fn migrate_v1_1_1_to_v2_0_0(
    deps: &mut DepsMut<NeutronQuery>,
    env: Env,
    msg: MigrateMsgV2_0_0,
) -> Result<(), ContractError> {
    migrate_constants(deps.storage, msg)?;

    // Ensure that the contract is currently within the first round.
    // This is done after constants migration in order to be able to
    // use the compute_current_round_id() helper function.
    let constants = CONSTANTS.load(deps.storage)?;
    let current_round_id = compute_current_round_id(&env, &constants)?;
    if current_round_id != 0 {
        return Err(ContractError::Std(StdError::generic_err(
            "Migration to version 2.0.0 can only be done within the first round.",
        )));
    }

    migrate_proposals(deps.storage)?;
    migrate_votes(deps, env)?;

    Ok(())
}

fn migrate_constants(
    storage: &mut dyn Storage,
    migrate_msg: MigrateMsgV2_0_0,
) -> Result<(), ContractError> {
    const OLD_CONSTANTS: Item<ConstantsV1_1_0> = Item::new("constants");
    const NEW_CONSTANTS: Item<ConstantsV2_0_0> = Item::new("constants");

    let old_constants = OLD_CONSTANTS.load(storage)?;
    let new_constants = ConstantsV2_0_0 {
        round_length: old_constants.round_length,
        lock_epoch_length: old_constants.lock_epoch_length,
        first_round_start: old_constants.first_round_start,
        max_locked_tokens: old_constants.max_locked_tokens,
        max_validator_shares_participating: old_constants.max_validator_shares_participating,
        hub_connection_id: old_constants.hub_connection_id,
        hub_transfer_channel_id: old_constants.hub_transfer_channel_id,
        icq_update_period: old_constants.icq_update_period,
        paused: old_constants.paused,
        is_in_pilot_mode: old_constants.is_in_pilot_mode,
        max_bid_duration: migrate_msg.max_bid_duration,
    };

    NEW_CONSTANTS.save(storage, &new_constants)?;

    Ok(())
}

fn migrate_proposals(storage: &mut dyn Storage) -> Result<(), ContractError> {
    const OLD_PROPOSAL_MAP: Map<(u64, u64, u64), ProposalV1_1_0> = Map::new("prop_map");
    const NEW_PROPOSAL_MAP: Map<(u64, u64, u64), ProposalV2_0_0> = Map::new("prop_map");

    let old_proposals = OLD_PROPOSAL_MAP.range(storage, None, None, Order::Descending);
    let mut new_proposals = vec![];

    for old_proposal in old_proposals {
        let ((_, _, _), old_proposal) = old_proposal?;

        let new_proposal = ProposalV2_0_0 {
            round_id: old_proposal.round_id,
            tranche_id: old_proposal.tranche_id,
            proposal_id: old_proposal.proposal_id,
            power: old_proposal.power,
            percentage: old_proposal.percentage,
            title: old_proposal.title,
            description: old_proposal.description,
            bid_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        };

        new_proposals.push(new_proposal);
    }

    for new_proposal in new_proposals {
        NEW_PROPOSAL_MAP.save(
            storage,
            (
                new_proposal.round_id,
                new_proposal.tranche_id,
                new_proposal.proposal_id,
            ),
            &new_proposal,
        )?;
    }

    Ok(())
}

fn migrate_votes(deps: &mut DepsMut<NeutronQuery>, env: Env) -> Result<(), ContractError> {
    const OLD_VOTE_MAP: Map<(u64, u64, Addr), VoteV1_1_0> = Map::new("vote_map");
    const NEW_VOTE_MAP: Map<((u64, u64), Addr, u64), VoteV2_0_0> = Map::new("vote_map");

    let mut old_votes = vec![];
    let mut new_votes = vec![];

    // We are migrating votes for the first round and single existing tranche
    let round_id = 0;
    let tranche_id = 1;

    // Here we rely on CONSTANTS being already migrated, and therefore we can use
    // the new struct and helper functions from the latest code.
    let constants = CONSTANTS.load(deps.storage)?;
    let round_end = compute_round_end(&constants, round_id)?;
    let lock_epoch_length = constants.lock_epoch_length;

    for old_vote in OLD_VOTE_MAP.prefix((round_id, tranche_id)).range(
        deps.storage,
        None,
        None,
        Order::Descending,
    ) {
        let (voter, old_vote) = old_vote?;

        old_votes.push(OldVoteInfo {
            round_id,
            tranche_id,
            voter: voter.clone(),
        });

        // We use LOCKS_MAP from the latest code since this storage hasn't changed.
        // This is needed in order to use get_lock_time_weighted_shares() helper function.
        for lock_entry in
            LOCKS_MAP
                .prefix(voter.clone())
                .range(deps.storage, None, None, Order::Ascending)
        {
            let (_, lock_entry) = lock_entry?;
            let validator = match validate_denom(
                deps.as_ref(),
                env.clone(),
                &constants,
                lock_entry.clone().funds.denom,
            ) {
                Ok(validator) => validator,
                Err(_) => {
                    continue;
                }
            };

            let scaled_shares = Decimal::from_ratio(
                get_lock_time_weighted_shares(round_end, lock_entry.clone(), lock_epoch_length),
                Uint128::one(),
            );

            if scaled_shares.is_zero() {
                continue;
            }

            let new_vote = NewVoteInfo {
                round_id,
                tranche_id,
                voter: voter.clone(),
                lock_id: lock_entry.lock_id,
                vote: VoteV2_0_0 {
                    prop_id: old_vote.prop_id,
                    time_weighted_shares: (validator, scaled_shares),
                },
            };

            new_votes.push(new_vote);
        }
    }

    for old_vote in old_votes {
        OLD_VOTE_MAP.remove(
            deps.storage,
            (old_vote.round_id, old_vote.tranche_id, old_vote.voter),
        );
    }

    for new_vote in new_votes {
        NEW_VOTE_MAP.save(
            deps.storage,
            (
                (new_vote.round_id, new_vote.tranche_id),
                new_vote.voter,
                new_vote.lock_id,
            ),
            &new_vote.vote,
        )?;
    }

    Ok(())
}
