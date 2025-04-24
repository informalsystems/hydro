use std::collections::HashMap;

use cosmwasm_std::{Addr, DepsMut, Order, Response};

use crate::{
    error::ContractError,
    state::{CONFIG, ID_TO_TRIBUTE_MAP, TRIBUTE_CLAIMED_LOCKS, TRIBUTE_CLAIMS},
};

// A struct to represent a unique round/tranche/voter combination
#[derive(Hash, Eq, PartialEq, Clone)]
struct VoterRoundContext {
    voter: Addr,
    round_id: u64,
    tranche_id: u64,
}

// A struct to group tributes by proposal for efficient processing
struct TributeClaimInfo {
    tribute_id: u64,
    proposal_id: u64,
}

/// Migrates the contract to version 3.2.0
/// This migration adds the TRIBUTE_CLAIMED_LOCKS state to track which locks have claimed which tributes
/// It populates this state from the existing TRIBUTE_CLAIMS
pub fn migrate_v3_1_1_to_unreleased(deps: &mut DepsMut) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let mut context_to_tributes: HashMap<VoterRoundContext, Vec<TributeClaimInfo>> = HashMap::new();

    for claim_entry in TRIBUTE_CLAIMS.range(deps.storage, None, None, Order::Ascending) {
        let ((voter_addr, tribute_id), _amount) = claim_entry?;

        let tribute = ID_TO_TRIBUTE_MAP
            .load(deps.storage, tribute_id)
            .expect("claimed tribute ID always valid");

        let context = VoterRoundContext {
            voter: voter_addr.clone(),
            round_id: tribute.round_id,
            tranche_id: tribute.tranche_id,
        };

        context_to_tributes
            .entry(context)
            .or_default()
            .push(TributeClaimInfo {
                tribute_id,
                proposal_id: tribute.proposal_id,
            });
    }

    let mut tribute_claimed_locks = 0;

    for (context, tributes) in context_to_tributes {
        let user_voted_locks = crate::contract::query_user_voted_locks(
            &deps.as_ref(),
            &config.hydro_contract,
            context.round_id,
            context.tranche_id,
            context.voter.to_string(),
        )?;

        let prop_to_locks: HashMap<_, _> = user_voted_locks.voted_locks.into_iter().collect();

        tribute_claimed_locks = tributes
            .into_iter()
            .filter_map(|t| {
                prop_to_locks
                    .get(&t.proposal_id)
                    // return an iterator of (tribute_id, lock_id)
                    .map(|lock_ids| {
                        lock_ids
                            .iter()
                            .map(move |lock| (t.tribute_id, lock.lock_id))
                    })
            })
            .flatten()
            .try_fold(0, |count, key| {
                TRIBUTE_CLAIMED_LOCKS
                    .save(deps.storage, key, &true)
                    .map(|_| count + 1)
            })?;
    }

    Ok(Response::new()
        .add_attribute("action", "migrate_v3_1_1_to_unreleased")
        .add_attribute("tribute_claimed_locks", tribute_claimed_locks.to_string()))
}
