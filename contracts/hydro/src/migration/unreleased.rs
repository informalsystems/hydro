use std::collections::HashMap;

use cosmwasm_std::{Decimal, DepsMut, Order, Response};
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

use crate::{
    error::ContractError,
    state::{Proposal, PROPOSAL_MAP, PROPOSAL_TOTAL_MAP, VOTE_MAP_V2},
    token_manager::TokenManager,
};

// Iterates over all votes in the given round and tranche and computes total voting power for each
// of the proposals, based on the vote entries. Then iterates over all proposals in the given round
// and tranche and, if necessary, updates their voting power in the store.
pub fn update_proposals_powers(
    deps: &mut DepsMut<NeutronQuery>,
    round_id: u64,
    tranche_id: u64,
) -> Result<Response<NeutronMsg>, ContractError> {
    // proposal_id -> [(token_group_id -> scaled_proposal_shares)]
    let mut proposals_total_shares: HashMap<u64, HashMap<String, Decimal>> = HashMap::new();

    for vote in VOTE_MAP_V2
        .prefix((round_id, tranche_id))
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|vote_res| match vote_res {
            Err(_) => None,
            Ok(vote) => Some(vote.1),
        })
    {
        let proposal_entry = proposals_total_shares.entry(vote.prop_id).or_default();

        let token_group_shares_entry = proposal_entry
            .entry(vote.time_weighted_shares.0.clone())
            .or_default();

        *token_group_shares_entry =
            token_group_shares_entry.checked_add(vote.time_weighted_shares.1)?;
    }

    let mut token_manager = TokenManager::new(&deps.as_ref());

    // TokenManager doesn't have caching for LSM token group ratios, so caching is introduced here.
    let mut token_group_ratios: HashMap<String, Decimal> = HashMap::new();

    let mut proposal_total_powers: HashMap<u64, Decimal> = HashMap::new();

    for proposal_token_group_shares in proposals_total_shares {
        let proposal_id = proposal_token_group_shares.0;
        let mut proposal_total_power = Decimal::zero();

        for token_group_shares in proposal_token_group_shares.1 {
            let token_group_id = token_group_shares.0;
            let token_group_shares_num = token_group_shares.1;

            let token_group_ratio = match token_group_ratios.get(&token_group_id) {
                Some(token_group_ratio) => *token_group_ratio,
                None => {
                    match token_manager.get_token_group_ratio(
                        &deps.as_ref(),
                        round_id,
                        token_group_id.clone(),
                    ) {
                        Err(_) => continue,
                        Ok(token_group_ratio) => {
                            token_group_ratios.insert(token_group_id, token_group_ratio);

                            token_group_ratio
                        }
                    }
                }
            };

            let token_group_shares_power = token_group_shares_num.checked_mul(token_group_ratio)?;
            proposal_total_power = proposal_total_power.checked_add(token_group_shares_power)?;
        }

        proposal_total_powers.insert(proposal_id, proposal_total_power);
    }

    // Ierate over all proposals in the given round and tranche and update their voting powers
    let round_tranche_proposals = PROPOSAL_MAP
        .prefix((round_id, tranche_id))
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|prop_res| match prop_res {
            Err(_) => None,
            Ok(prop) => Some(prop.1),
        })
        .collect::<Vec<Proposal>>();

    // Record both old and new proposal voting power to attach response attributes
    let mut proposals_power_updates = vec![];

    for mut proposal in round_tranche_proposals {
        // In case there were no votes for the given proposal, we will set its power to 0
        let new_proposal_power_dec = proposal_total_powers
            .get(&proposal.proposal_id)
            .cloned()
            .unwrap_or_default();
        let new_proposal_power_int = new_proposal_power_dec.to_uint_ceil();

        if proposal.power == new_proposal_power_int {
            continue;
        }

        proposals_power_updates.push((
            proposal.proposal_id,
            proposal.power,
            new_proposal_power_int,
        ));

        proposal.power = new_proposal_power_int;

        PROPOSAL_MAP.save(
            deps.storage,
            (round_id, tranche_id, proposal.proposal_id),
            &proposal,
        )?;

        PROPOSAL_TOTAL_MAP.save(deps.storage, proposal.proposal_id, &new_proposal_power_dec)?;
    }

    let mut response = Response::new().add_attribute("action", "update_proposals_powers");

    for proposal_power_update in proposals_power_updates {
        response = response.add_attribute(
            format!("proposal_id_{}", proposal_power_update.0),
            format!(
                "old_voting_power: {}, new_voting_power: {}",
                proposal_power_update.1, proposal_power_update.2
            ),
        )
    }

    Ok(response)
}
