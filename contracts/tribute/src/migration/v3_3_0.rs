use cosmwasm_schema::cw_serde;
use std::collections::HashMap;

use cosmwasm_std::{Addr, DepsMut, Order, Response};

use crate::{
    error::ContractError,
    state::{Tribute, CONFIG, ID_TO_TRIBUTE_MAP, TRIBUTE_CLAIMED_LOCKS, TRIBUTE_CLAIMS},
};

#[cw_serde]
pub struct MigrateMsgV3_3_0 {}

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

/// Migrates the contract to version 3.4.0
/// This migration adds the TRIBUTE_CLAIMED_LOCKS state to track which locks have claimed which tributes
/// It populates this state from the existing TRIBUTE_CLAIMS
pub fn migrate_v3_3_0_to_v3_4_0(deps: &mut DepsMut) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let mut context_to_tributes: HashMap<VoterRoundContext, Vec<TributeClaimInfo>> = HashMap::new();

    // Cache to store loaded tributes by ID
    let mut tribute_cache: HashMap<u64, Tribute> = HashMap::new();

    for claim_entry in TRIBUTE_CLAIMS.range(deps.storage, None, None, Order::Ascending) {
        let ((voter_addr, tribute_id), _amount) = claim_entry?;

        let tribute = tribute_cache.entry(tribute_id).or_insert_with(|| {
            ID_TO_TRIBUTE_MAP
                .load(deps.storage, tribute_id)
                .expect("claimed tribute ID always valid")
        });

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
            context.voter.to_string(),
            context.round_id,
            context.tranche_id,
            None,
        )?;

        let prop_to_locks: HashMap<_, _> = user_voted_locks.voted_locks.into_iter().collect();

        for tribute in tributes {
            let Some(locks) = prop_to_locks.get(&tribute.proposal_id) else {
                continue;
            };

            for lock in locks {
                TRIBUTE_CLAIMED_LOCKS.save(
                    deps.storage,
                    (tribute.tribute_id, lock.lock_id),
                    &true,
                )?;
                tribute_claimed_locks += 1;
            }
        }
    }

    Ok(Response::new()
        .add_attribute("action", "migrate_v3_3_0_to_v3_4_0")
        .add_attribute("tribute_claimed_locks", tribute_claimed_locks.to_string()))
}
