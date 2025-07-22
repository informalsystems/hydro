use cosmwasm_std::{
    Decimal, Deps, DepsMut, Order, SignedDecimal, StdError, StdResult, Storage, Uint128,
};
use cw_storage_plus::Map;
use neutron_sdk::bindings::query::NeutronQuery;
use std::collections::HashMap;
use std::convert::TryFrom;

use crate::{
    state::{
        Proposal, PROPOSAL_MAP, PROPOSAL_TOTAL_MAP, PROPS_BY_SCORE, SCALED_PROPOSAL_SHARES_MAP,
        SCALED_ROUND_POWER_SHARES_MAP, TOTAL_VOTING_POWER_PER_ROUND, TRANCHE_MAP,
    },
    token_manager::TokenManager,
};

pub fn get_total_power_for_proposal(storage: &dyn Storage, prop_id: u64) -> StdResult<Decimal> {
    Ok(PROPOSAL_TOTAL_MAP
        .may_load(storage, prop_id)?
        .unwrap_or(Decimal::zero()))
}

pub fn get_total_power_for_round(deps: &Deps<NeutronQuery>, round_id: u64) -> StdResult<Decimal> {
    Ok(
        match TOTAL_VOTING_POWER_PER_ROUND.may_load(deps.storage, round_id)? {
            None => Decimal::zero(),
            Some(total_voting_power) => Decimal::from_ratio(total_voting_power, Uint128::one()),
        },
    )
}

pub fn get_token_group_shares_for_proposal(
    storage: &dyn Storage,
    prop_id: u64,
    token_group_id: String,
) -> StdResult<Decimal> {
    Ok(SCALED_PROPOSAL_SHARES_MAP
        .may_load(storage, (prop_id, token_group_id))?
        .unwrap_or(Decimal::zero()))
}

// Add token group shares and update the total power
pub fn add_token_group_shares(
    storage: &mut dyn Storage,
    index_key: u64,
    shares_map: Map<(u64, String), Decimal>,
    total_map: Map<u64, Decimal>,
    token_group_id: String,
    num_shares: Decimal,
    power_ratio: Decimal,
) -> StdResult<()> {
    let key = (index_key, token_group_id.clone());

    // Update the shares map
    let current_shares = shares_map
        .may_load(storage, key.clone())?
        .unwrap_or_else(Decimal::zero);
    let updated_shares = current_shares + num_shares;
    shares_map.save(storage, key, &updated_shares)?;

    // Update the total power
    let mut current_power = total_map
        .load(storage, index_key)
        .unwrap_or(Decimal::zero());
    let added_power = num_shares * power_ratio;

    current_power += added_power;
    total_map.save(storage, index_key, &current_power)?;

    Ok(())
}

pub fn add_token_group_shares_to_proposal(
    deps: &mut DepsMut<NeutronQuery>,
    token_manager: &mut TokenManager,
    round_id: u64,
    prop_id: u64,
    token_group_id: String,
    num_shares: Decimal,
) -> StdResult<()> {
    let power_ratio =
        token_manager.get_token_group_ratio(&deps.as_ref(), round_id, token_group_id.clone())?;
    add_token_group_shares(
        deps.storage,
        prop_id,
        SCALED_PROPOSAL_SHARES_MAP,
        PROPOSAL_TOTAL_MAP,
        token_group_id,
        num_shares,
        power_ratio,
    )
}

// Remove token group shares and update the total power
pub fn remove_token_group_shares(
    storage: &mut dyn Storage,
    index_key: u64,
    shares_map: Map<(u64, String), Decimal>,
    total_map: Map<u64, Decimal>,
    token_group_id: String,
    num_shares: Decimal,
    power_ratio: Decimal,
) -> StdResult<()> {
    let key = (index_key, token_group_id.clone());

    // Load current shares
    let current_shares = shares_map
        .may_load(storage, key.clone())?
        .unwrap_or_else(Decimal::zero);

    // Ensure the token group has enough shares
    if current_shares < num_shares {
        return Err(StdError::generic_err(
            "Insufficient shares for the token group",
        ));
    }

    // Update the shares map
    let updated_shares = current_shares - num_shares;
    shares_map.save(storage, key, &updated_shares)?;

    // Update the total power
    let mut current_power = total_map.load(storage, index_key)?;
    let removed_power = num_shares * power_ratio;
    current_power -= removed_power;
    total_map.save(storage, index_key, &current_power)?;

    Ok(())
}

pub fn remove_token_group_shares_from_proposal(
    deps: &mut DepsMut<NeutronQuery>,
    token_manager: &mut TokenManager,
    round_id: u64,
    prop_id: u64,
    token_group_id: String,
    num_shares: Decimal,
) -> StdResult<()> {
    let power_ratio =
        token_manager.get_token_group_ratio(&deps.as_ref(), round_id, token_group_id.clone())?;
    remove_token_group_shares(
        deps.storage,
        prop_id,
        SCALED_PROPOSAL_SHARES_MAP,
        PROPOSAL_TOTAL_MAP,
        token_group_id,
        num_shares,
        power_ratio,
    )
}

// A more gas efficient version of remove_token_group_shares_from_proposal
// when removing shares from multiple tokens
pub fn remove_many_token_group_shares_from_proposal(
    deps: &mut DepsMut<NeutronQuery>,
    token_manager: &mut TokenManager,
    round_id: u64,
    prop_id: u64,
    token_groups_and_shares: Vec<(String, Decimal)>,
) -> StdResult<()> {
    let mut total_power = get_total_power_for_proposal(deps.storage, prop_id)?;

    // do not reuse remove_token_group_shares_from_proposal
    // instead, we will directly update the shares and power
    // to be more gas efficient
    for (token_group_id, num_shares) in token_groups_and_shares {
        let power_ratio = token_manager.get_token_group_ratio(
            &deps.as_ref(),
            round_id,
            token_group_id.clone(),
        )?;
        let current_shares = SCALED_PROPOSAL_SHARES_MAP
            .may_load(deps.storage, (prop_id, token_group_id.clone()))?
            .unwrap_or_else(Decimal::zero);

        // Ensure the token group has enough shares
        if current_shares < num_shares {
            return Err(StdError::generic_err(
                "Insufficient shares for the token group",
            ));
        }

        // Update the shares map
        let updated_shares = current_shares - num_shares;
        SCALED_PROPOSAL_SHARES_MAP.save(
            deps.storage,
            (prop_id, token_group_id),
            &updated_shares,
        )?;

        // Update the total power
        let removed_power = num_shares * power_ratio;

        if total_power < removed_power {
            return Err(StdError::generic_err("Insufficient total power"));
        }

        total_power -= removed_power;
    }

    PROPOSAL_TOTAL_MAP.save(deps.storage, prop_id, &total_power)
}

// Helper function to batch add token group shares to proposals
// This function takes a SignedDecimal in shares so it is possible to use it to "remove" shares
// The function remove_many_token_group_shares_from_proposal should not be needed anymore.
// Note: The below allow is necesary to avoid clippy warning about redundant closure
// Otherwise, it throws another error: expected an `FnOnce()` closure, found `cosmwasm_std::Decimal`
// and suggests to wrap the Decimal in a closure with no arguments: `|| { /* code */ }`
#[allow(clippy::redundant_closure)]
pub fn add_many_token_group_shares_to_proposal(
    deps: &mut DepsMut<NeutronQuery>,
    token_manager: &mut TokenManager,
    round_id: u64,
    proposal_id: u64,
    token_groups_and_shares: Vec<(String, SignedDecimal)>,
) -> StdResult<()> {
    let total_power_decimal = get_total_power_for_proposal(deps.storage, proposal_id)?;

    // Convert total_power to SignedDecimal for computation
    let mut total_power = SignedDecimal::try_from(total_power_decimal).map_err(|_| {
        StdError::generic_err("Failed to convert Decimal to SignedDecimal for total_power")
    })?;

    for (token_group_id, num_shares) in token_groups_and_shares {
        let power_ratio_decimal = token_manager.get_token_group_ratio(
            &deps.as_ref(),
            round_id,
            token_group_id.clone(),
        )?;

        let power_ratio = SignedDecimal::try_from(power_ratio_decimal).map_err(|_| {
            StdError::generic_err("Failed to convert Decimal to SignedDecimal for power_ratio")
        })?;

        let current_shares = SCALED_PROPOSAL_SHARES_MAP
            .may_load(deps.storage, (proposal_id, token_group_id.clone()))?
            .unwrap_or_else(|| Decimal::zero());

        // Convert current shares to SignedDecimal for computation
        let current_shares_signed = SignedDecimal::try_from(current_shares).map_err(|_| {
            StdError::generic_err("Failed to convert Decimal to SignedDecimal for current_shares")
        })?;

        // Compute the updated shares as SignedDecimal
        let updated_shares_signed = current_shares_signed + num_shares;

        // Ensure updated shares cannot go negative
        if updated_shares_signed.is_negative() {
            return Err(StdError::generic_err(
                "Shares for a token group cannot be negative",
            ));
        }

        // Convert updated shares back to Decimal for storage
        let updated_shares = Decimal::try_from(updated_shares_signed).map_err(|_| {
            StdError::generic_err("Failed to convert SignedDecimal to Decimal for updated_shares")
        })?;

        // Save updated shares
        SCALED_PROPOSAL_SHARES_MAP.save(
            deps.storage,
            (proposal_id, token_group_id.clone()),
            &updated_shares,
        )?;

        // Update the total power
        let delta_power = num_shares * power_ratio;
        let new_total_power = total_power + delta_power;

        // Ensure total power cannot be negative
        if new_total_power.is_negative() {
            return Err(StdError::generic_err("Total power cannot be negative"));
        }

        total_power = new_total_power;
    }

    // Convert total_power back to Decimal for storage
    let final_total_power = Decimal::try_from(total_power).map_err(|_| {
        StdError::generic_err("Failed to convert SignedDecimal to Decimal for total_power")
    })?;

    // Save updated total power
    PROPOSAL_TOTAL_MAP.save(deps.storage, proposal_id, &final_total_power)
}

// Struct to track voting power changes for a proposal
#[derive(Debug, Default, Clone)]
pub struct ProposalPowerUpdate {
    pub token_group_shares: HashMap<String, SignedDecimal>, // token_group_id -> shares
}

// This function combines 2 proposal power updates into a single one.
// It sums the shares for each token group ID in the updates.
pub fn combine_proposal_power_updates(
    updates1: HashMap<u64, ProposalPowerUpdate>,
    updates2: HashMap<u64, ProposalPowerUpdate>,
) -> HashMap<u64, ProposalPowerUpdate> {
    let mut combined_updates = updates1;

    for (proposal_id, mut update2) in updates2 {
        // Clean up any zero entries in update2 before inserting
        update2
            .token_group_shares
            .retain(|_, shares| !shares.is_zero());

        if update2.token_group_shares.is_empty() {
            continue;
        }

        combined_updates
            .entry(proposal_id)
            .and_modify(|update1| {
                for (token_group_id, shares) in update2.token_group_shares.drain() {
                    update1
                        .token_group_shares
                        .entry(token_group_id)
                        .and_modify(|existing| {
                            *existing += shares;
                        })
                        .or_insert(shares);
                }

                // Remove any token group entries that sum to zero
                update1
                    .token_group_shares
                    .retain(|_, shares| !shares.is_zero());
            })
            .or_insert(update2);
    }

    // Remove any proposals that end up with no token group shares
    combined_updates.retain(|_, update| !update.token_group_shares.is_empty());

    combined_updates
}

// This function applies the changes in the proposal power updates to the storage.
// As it is basically a wrapper around add_many_token_group_shares_to_proposal function,
//  it will update SCALED_PROPOSAL_SHARES_MAP and TOTAL_PROPOSAL_MAP.
pub fn apply_proposal_changes(
    deps: &mut DepsMut<NeutronQuery>,
    token_manager: &mut TokenManager,
    round_id: u64,
    changes: HashMap<u64, ProposalPowerUpdate>,
) -> StdResult<()> {
    for (proposal_id, changes) in changes {
        let token_groups_and_shares: Vec<(String, SignedDecimal)> = changes
            .token_group_shares
            .into_iter()
            .filter(|(_, shares)| !shares.is_zero())
            .collect();

        if !token_groups_and_shares.is_empty() {
            add_many_token_group_shares_to_proposal(
                deps,
                token_manager,
                round_id,
                proposal_id,
                token_groups_and_shares,
            )?;
        }
    }

    Ok(())
}

/// Updates all the required stores each time some token group ratio towards the base token is changed
pub fn apply_token_groups_ratio_changes(
    storage: &mut dyn Storage,
    current_height: u64,
    current_round_id: u64,
    tokens_ratio_changes: &Vec<TokenGroupRatioChange>,
) -> StdResult<()> {
    update_scores_due_to_tokens_ratio_changes(storage, current_round_id, tokens_ratio_changes)?;

    update_total_power_due_to_tokens_ratio_changes(
        storage,
        current_height,
        current_round_id,
        tokens_ratio_changes,
    )?;

    Ok(())
}

// Applies the new token ratio for the list of provided token groups to the score keepers.
// It updates all proposals of the given round. For each proposal, and each token group,
// it will recompute the new power by subtracting the old_token_ratio*that token group shares
// and adding the new_token_ratio*that token group shares.
pub fn update_scores_due_to_tokens_ratio_changes(
    storage: &mut dyn Storage,
    round_id: u64,
    tokens_ratio_changes: &Vec<TokenGroupRatioChange>,
) -> StdResult<()> {
    // go through each tranche in the TRANCHE_MAP and collect its tranche_id
    let tranche_ids: Vec<u64> = TRANCHE_MAP
        .range(storage, None, None, Order::Ascending)
        .map(|tranche_res| {
            let tranche = tranche_res.unwrap();
            tranche.0
        })
        .collect();
    for tranche_id in tranche_ids {
        // go through each proposal in the PROPOSAL_MAP for this round and tranche

        // collect all proposal ids
        let proposals: Vec<Proposal> = PROPOSAL_MAP
            .prefix((round_id, tranche_id))
            .range(storage, None, None, Order::Ascending)
            .map(|prop_res| {
                let prop = prop_res.unwrap();
                prop.1
            })
            .collect();

        for proposal in proposals {
            // update the power ratios for the proposal
            update_power_ratio_for_proposal(storage, proposal.proposal_id, tokens_ratio_changes)?;

            // create a mutable copy of the proposal that we can safely manipulate in this loop
            let mut proposal_copy = proposal.clone();

            // save the new power for the proposal in the store
            proposal_copy.power =
                get_total_power_for_proposal(storage, proposal_copy.proposal_id)?.to_uint_ceil();

            PROPOSAL_MAP.save(
                storage,
                (round_id, tranche_id, proposal.proposal_id),
                &proposal_copy,
            )?;

            // remove proposals old score
            PROPS_BY_SCORE.remove(
                storage,
                (
                    (round_id, tranche_id),
                    proposal.power.into(),
                    proposal.proposal_id,
                ),
            );

            PROPS_BY_SCORE.save(
                storage,
                (
                    (round_id, tranche_id),
                    proposal_copy.power.into(),
                    proposal_copy.proposal_id,
                ),
                &proposal_copy.proposal_id,
            )?;
        }
    }
    Ok(())
}

/// Updates the total voting power for the current and future rounds when the ratios of the given token groups are changed.
pub fn update_total_power_due_to_tokens_ratio_changes(
    storage: &mut dyn Storage,
    current_height: u64,
    current_round_id: u64,
    tokens_ratio_changes: &[TokenGroupRatioChange],
) -> StdResult<()> {
    let mut round_id = current_round_id;

    // Convert to HashMap in order to remove the fully processed token groups more easily
    let mut tokens_ratio_changes: HashMap<String, TokenGroupRatioChange> =
        HashMap::from_iter(tokens_ratio_changes.iter().map(|token_ratio_change| {
            (
                token_ratio_change.token_group_id.clone(),
                token_ratio_change.clone(),
            )
        }));

    // Try to update the total voting power starting from the current round id and moving to next rounds until
    // we reach the round for which there is no entry in the TOTAL_VOTING_POWER_PER_ROUND. This implies the first
    // round in which no lock entry gives voting power, which also must be true for all rounds after that round,
    // so we break the loop at that point.
    loop {
        let total_voting_power_initial =
            match TOTAL_VOTING_POWER_PER_ROUND.may_load(storage, round_id)? {
                None => break,
                Some(total_voting_power) => Decimal::from_ratio(total_voting_power, Uint128::one()),
            };
        let mut total_voting_power_current = total_voting_power_initial;

        let mut processed_token_groups = vec![];

        for token_ratio_change in &tokens_ratio_changes {
            let token_group_shares =
                get_token_group_shares_for_round(storage, round_id, token_ratio_change.0.clone())?;

            if token_group_shares == Decimal::zero() {
                // If we encounter a round that doesn't have the token group ID shares, then no subsequent
                // round could also have its shares, so mark this token group as processed.
                processed_token_groups.push(token_ratio_change.0.clone());

                continue;
            }

            let old_token_group_shares_power = token_group_shares * token_ratio_change.1.old_ratio;
            let new_token_group_shares_power = token_group_shares * token_ratio_change.1.new_ratio;

            total_voting_power_current = total_voting_power_current
                .checked_add(new_token_group_shares_power)?
                .checked_sub(old_token_group_shares_power)?;
        }

        // Remove all processed token groups in order to save some gas for the next round execution
        for token_group_id in processed_token_groups {
            tokens_ratio_changes.remove(&token_group_id);
        }

        if total_voting_power_current != total_voting_power_initial {
            TOTAL_VOTING_POWER_PER_ROUND.save(
                storage,
                round_id,
                &total_voting_power_current.to_uint_ceil(),
                current_height,
            )?;
        }

        round_id += 1;
    }

    Ok(())
}

pub fn get_token_group_shares_for_round(
    storage: &dyn Storage,
    round_id: u64,
    token_group_id: String,
) -> StdResult<Decimal> {
    Ok(SCALED_ROUND_POWER_SHARES_MAP
        .may_load(storage, (round_id, token_group_id))?
        .unwrap_or(Decimal::zero()))
}

pub fn add_token_group_shares_to_round_total(
    storage: &mut dyn Storage,
    current_height: u64,
    round_id: u64,
    token_group_id: String,
    token_ratio: Decimal,
    num_shares: Decimal,
) -> StdResult<()> {
    let current_shares =
        get_token_group_shares_for_round(storage, round_id, token_group_id.clone())?;
    let new_shares = current_shares.checked_add(num_shares)?;
    SCALED_ROUND_POWER_SHARES_MAP.save(storage, (round_id, token_group_id.clone()), &new_shares)?;

    // Update total voting power for the round
    TOTAL_VOTING_POWER_PER_ROUND.update(
        storage,
        round_id,
        current_height,
        |total_power_before| -> Result<Uint128, StdError> {
            let total_power_before = match total_power_before {
                None => Decimal::zero(),
                Some(total_power_before) => Decimal::from_ratio(total_power_before, Uint128::one()),
            };

            Ok(total_power_before
                .checked_add(num_shares.checked_mul(token_ratio)?)?
                .to_uint_ceil())
        },
    )?;

    Ok(())
}

pub fn update_power_ratio_for_proposal(
    storage: &mut dyn Storage,
    proposal_id: u64,
    tokens_ratio_changes: &Vec<TokenGroupRatioChange>,
) -> StdResult<()> {
    // Load current power of proposal
    let initial_power = PROPOSAL_TOTAL_MAP
        .load(storage, proposal_id)
        .unwrap_or(Decimal::zero());
    let mut current_power = initial_power;

    for token_ratio_change in tokens_ratio_changes {
        // Load current shares
        let current_shares = SCALED_PROPOSAL_SHARES_MAP
            .may_load(
                storage,
                (proposal_id, token_ratio_change.token_group_id.clone()),
            )?
            .unwrap_or_else(Decimal::zero);

        // No operation if the token group has no shares
        if current_shares == Decimal::zero() {
            continue;
        }

        // Compute old and new power of the given token group shares
        let old_power = current_shares * token_ratio_change.old_ratio;
        let new_power = current_shares * token_ratio_change.new_ratio;

        current_power = current_power - old_power + new_power;
    }

    if current_power != initial_power {
        PROPOSAL_TOTAL_MAP.save(storage, proposal_id, &current_power)?;
    }

    Ok(())
}

#[derive(Clone)]
pub struct TokenGroupRatioChange {
    pub token_group_id: String,
    pub old_ratio: Decimal,
    pub new_ratio: Decimal,
}

#[cfg(test)]
mod tests {
    use crate::state::TOKEN_INFO_PROVIDERS;
    use crate::testing::get_default_lsm_token_info_provider;
    use crate::testing_lsm_integration::set_validator_power_ratio;
    use crate::testing_mocks::mock_dependencies;
    use crate::testing_mocks::no_op_grpc_query_mock;
    use crate::token_manager::TokenManager;
    use crate::token_manager::LSM_TOKEN_INFO_PROVIDER_ID;

    use super::*;
    use cosmwasm_std::{Decimal, StdError, Storage};
    use proptest::prelude::*;
    fn get_shares_and_power_for_proposal(
        storage: &dyn Storage,
        prop_id: u64,
        token_group: String,
    ) -> (Decimal, Decimal) {
        let shares = get_token_group_shares_for_proposal(storage, prop_id, token_group).unwrap();
        let total_power = get_total_power_for_proposal(storage, prop_id).unwrap();
        (shares, total_power)
    }

    // Table-based tests
    #[test]
    fn test_uninitialized() {
        let deps = mock_dependencies(no_op_grpc_query_mock());
        let index_key = 5;

        let total_power = get_total_power_for_round(&deps.as_ref(), index_key).unwrap();
        assert_eq!(total_power, Decimal::zero());

        let total_power = get_total_power_for_proposal(deps.as_ref().storage, index_key).unwrap();
        assert_eq!(total_power, Decimal::zero());
    }

    #[test]
    fn test_add_token_group_shares() {
        let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let storage = deps.as_mut().storage;

        let index_key = 5;
        let token_group = "validator1";
        let num_shares = Decimal::from_ratio(100u128, 1u128);
        let power_ratio = Decimal::from_ratio(2u128, 1u128);

        // add token group shares with the specified power ratio
        let result = add_token_group_shares(
            storage,
            index_key,
            SCALED_PROPOSAL_SHARES_MAP,
            PROPOSAL_TOTAL_MAP,
            token_group.to_string(),
            num_shares,
            power_ratio,
        );
        assert!(result.is_ok());

        let (shares, total_power) =
            get_shares_and_power_for_proposal(storage, index_key, token_group.to_string());
        assert_eq!(shares, num_shares);
        assert_eq!(total_power, num_shares * power_ratio);
    }

    #[test]
    fn test_remove_token_group_shares() {
        let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let storage = deps.as_mut().storage;

        let index_key = 5;
        let token_group = "validator1";
        let num_shares = Decimal::from_ratio(100u128, 1u128);
        let power_ratio = Decimal::from_ratio(2u128, 1u128);

        // Add shares first
        let _ = add_token_group_shares(
            storage,
            index_key,
            SCALED_PROPOSAL_SHARES_MAP,
            PROPOSAL_TOTAL_MAP,
            token_group.to_string(),
            num_shares,
            power_ratio,
        );

        // Now remove shares
        let result = remove_token_group_shares(
            storage,
            index_key,
            SCALED_PROPOSAL_SHARES_MAP,
            PROPOSAL_TOTAL_MAP,
            token_group.to_string(),
            num_shares,
            power_ratio,
        );
        assert!(result.is_ok());

        let (shares, total_power) =
            get_shares_and_power_for_proposal(storage, index_key, token_group.to_string());
        assert_eq!(shares, Decimal::zero());
        assert_eq!(total_power, Decimal::zero());
    }

    #[test]
    fn test_remove_token_group_shares_insufficient_shares() {
        let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let storage = deps.as_mut().storage;

        let index_key = 10;
        let token_group = "validator1";
        let num_shares = Decimal::from_ratio(100u128, 1u128);
        let power_ratio = Decimal::from_ratio(2u128, 1u128);

        // Add a smaller amount of shares
        let _ = add_token_group_shares(
            storage,
            index_key,
            SCALED_PROPOSAL_SHARES_MAP,
            PROPOSAL_TOTAL_MAP,
            token_group.to_string(),
            Decimal::from_ratio(50u128, 1u128),
            power_ratio,
        );

        // Attempt to remove more shares than exist
        let result = remove_token_group_shares(
            storage,
            index_key,
            SCALED_PROPOSAL_SHARES_MAP,
            PROPOSAL_TOTAL_MAP,
            token_group.to_string(),
            num_shares,
            power_ratio,
        );
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            StdError::generic_err("Insufficient shares for the token group")
        );
    }

    #[test]
    fn test_update_power_ratio() {
        let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let storage = deps.as_mut().storage;

        let key = 5;
        let token_group = "validator1";
        let num_shares = Decimal::from_ratio(100u128, 1u128);
        let old_power_ratio = Decimal::from_ratio(2u128, 1u128);
        let new_power_ratio = Decimal::from_ratio(3u128, 1u128);

        // Add shares
        let _ = add_token_group_shares(
            storage,
            key,
            SCALED_PROPOSAL_SHARES_MAP,
            PROPOSAL_TOTAL_MAP,
            token_group.to_string(),
            num_shares,
            old_power_ratio,
        );

        // update the power ratio
        let tokens_ratio_changes = vec![TokenGroupRatioChange {
            token_group_id: token_group.to_string(),
            old_ratio: old_power_ratio,
            new_ratio: new_power_ratio,
        }];
        let res = update_power_ratio_for_proposal(storage, key, &tokens_ratio_changes);

        assert!(res.is_ok());

        let (_, total_power) =
            get_shares_and_power_for_proposal(storage, key, token_group.to_string());
        assert_eq!(total_power, num_shares * new_power_ratio);
    }

    // Property-based tests using proptest
    proptest! {
        #[test]
        fn proptest_multi_add_remove_token_group_shares(num_shares in 1u128..1_000_000u128, num_shares2 in 1u128..1_000_000u128, power_ratio in 1u128..1_000u128, power_ratio2 in 1u128..1_000u128) {
            let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let storage = deps.as_mut().storage;

            let key = 5;
            let token_group = "validator1";
            let num_shares = Decimal::from_ratio(num_shares, 1u128);
            let power_ratio = Decimal::from_ratio(power_ratio, 1u128);
            let num_shares2 = Decimal::from_ratio(num_shares2, 1u128);
            let power_ratio2 = Decimal::from_ratio(power_ratio2, 1u128);

            let res = add_token_group_shares(storage,
                key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                token_group.to_string(), num_shares, power_ratio);
            assert!(res.is_ok(), "Error adding token group shares: {res:?}");
            let (shares, total_power) = get_shares_and_power_for_proposal(storage, key, token_group.to_string());

            // Check if shares and power are correct after adding
            assert_eq!(shares, num_shares);
            assert_eq!(total_power, num_shares * power_ratio);

            // set the power ratio
            let tokens_ratio_changes = vec![TokenGroupRatioChange {
                token_group_id: token_group.to_string(),
                old_ratio: power_ratio,
                new_ratio: power_ratio2,
            }];
            let res = update_power_ratio_for_proposal(
                storage,
                key,
                &tokens_ratio_changes);
            assert!(res.is_ok(), "Error updating power ratio: {res:?}");

            // add the second shares
            let res = add_token_group_shares(
                storage,
                key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                token_group.to_string(), num_shares2, power_ratio2);
            assert!(res.is_ok(), "Error adding token group shares: {res:?}");
            let (shares, total_power) = get_shares_and_power_for_proposal(storage, key, token_group.to_string());

            // Check if shares and power are correct after the second addition
            assert_eq!(shares, num_shares + num_shares2);
            assert_eq!(total_power, num_shares * power_ratio2 + num_shares2 * power_ratio2);

            // successively remove the shares
            let res = remove_token_group_shares(storage, key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                token_group.to_string(), num_shares2, power_ratio2);
            assert!(res.is_ok(), "Error removing token group shares: {res:?}");
            let (shares, total_power) = get_shares_and_power_for_proposal(storage, key, token_group.to_string());

            // Check if shares and power are correct after removing
            assert_eq!(shares, num_shares);
            assert_eq!(total_power, num_shares * power_ratio2);

            // set the power ratio
            let tokens_ratio_changes = vec![TokenGroupRatioChange {
                token_group_id: token_group.to_string(),
                old_ratio: power_ratio2,
                new_ratio: power_ratio,
            }];
            let res = update_power_ratio_for_proposal(storage, key, &tokens_ratio_changes);
            assert!(res.is_ok(), "Error updating power ratio: {res:?}");

            let (_, total_power) = get_shares_and_power_for_proposal(storage, key, token_group.to_string());

            // Check that the power ratio is updated correctly
            assert_eq!(total_power, num_shares * power_ratio);

            let res = remove_token_group_shares(storage, key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                token_group.to_string(), num_shares, power_ratio);
            assert!(res.is_ok(), "Error removing token group shares: {res:?}");
            let (shares, total_power) = get_shares_and_power_for_proposal(storage, key, token_group.to_string());

            // Check if shares and power are zero after removing the second batch of shares
            assert_eq!(shares, Decimal::zero());
            assert_eq!(total_power, Decimal::zero());
        }

        #[test]
        fn proptest_update_power_ratio(num_shares in 1u128..1_000_000u128, old_power_ratio in 1u128..1_000u128, new_power_ratio in 1u128..1_000u128) {
            let mut deps = mock_dependencies(no_op_grpc_query_mock());
            let storage = deps.as_mut().storage;

            let key = 8;
            let token_group = "validator1";
            let num_shares = Decimal::from_ratio(num_shares, 1u128);
            let old_power_ratio = Decimal::from_ratio(old_power_ratio, 1u128);
            let new_power_ratio = Decimal::from_ratio(new_power_ratio, 1u128);

            // set the power ratio
            let tokens_ratio_changes = vec![TokenGroupRatioChange {
                token_group_id: token_group.to_string(),
                old_ratio: Decimal::zero(),
                new_ratio: old_power_ratio,
            }];
            let res = update_power_ratio_for_proposal(storage, key, &tokens_ratio_changes);
            assert!(res.is_ok(), "Error updating power ratio: {res:?}");

            let res = add_token_group_shares(storage, key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                token_group.to_string(), num_shares, old_power_ratio);
            assert!(res.is_ok(), "Error adding token group shares: {res:?}");

            // set the power ratio
            let tokens_ratio_changes = vec![TokenGroupRatioChange {
                token_group_id: token_group.to_string(),
                old_ratio: old_power_ratio,
                new_ratio: new_power_ratio,
            }];
            let res = update_power_ratio_for_proposal(storage, key, &tokens_ratio_changes);
            assert!(res.is_ok(), "Error updating power ratio: {res:?}");

            let (_, total_power) = get_shares_and_power_for_proposal(storage, key, token_group.to_string());

            // Check if total power is updated correctly
            assert_eq!(total_power, num_shares * new_power_ratio);
        }
    }

    #[test]
    fn test_remove_many_token_group_shares_from_proposal() {
        let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let round_id = 1;
        let prop_id = 1;

        // Setup initial state
        let token_group1 = "validator1".to_string();
        let token_group2 = "validator2".to_string();
        let initial_shares1 = Decimal::percent(50);
        let initial_shares2 = Decimal::percent(30);
        let power_ratio1 = Decimal::percent(10);
        let power_ratio2 = Decimal::percent(20);

        // Mock the initial shares and power ratios
        SCALED_PROPOSAL_SHARES_MAP
            .save(
                &mut deps.storage,
                (prop_id, token_group1.clone()),
                &initial_shares1,
            )
            .unwrap();
        SCALED_PROPOSAL_SHARES_MAP
            .save(
                &mut deps.storage,
                (prop_id, token_group2.clone()),
                &initial_shares2,
            )
            .unwrap();
        set_validator_power_ratio(
            &mut deps.storage,
            round_id,
            token_group1.as_str(),
            power_ratio1,
        );
        set_validator_power_ratio(
            &mut deps.storage,
            round_id,
            token_group2.as_str(),
            power_ratio2,
        );

        TOKEN_INFO_PROVIDERS
            .save(
                &mut deps.storage,
                LSM_TOKEN_INFO_PROVIDER_ID.to_string(),
                &get_default_lsm_token_info_provider(),
            )
            .unwrap();
        let mut token_manager = TokenManager::new(&deps.as_ref());

        // Mock the total power
        let total_power = Decimal::percent(100);
        PROPOSAL_TOTAL_MAP
            .save(&mut deps.storage, prop_id, &total_power)
            .unwrap();

        // Remove shares
        let vals_and_shares = vec![
            (token_group1.clone(), Decimal::percent(10)),
            (token_group2.clone(), Decimal::percent(10)),
        ];

        remove_many_token_group_shares_from_proposal(
            &mut deps.as_mut(),
            &mut token_manager,
            round_id,
            prop_id,
            vals_and_shares,
        )
        .unwrap();

        // Check the updated shares
        let updated_shares1 = SCALED_PROPOSAL_SHARES_MAP
            .load(deps.as_ref().storage, (prop_id, token_group1))
            .unwrap();
        let updated_shares2 = SCALED_PROPOSAL_SHARES_MAP
            .load(deps.as_ref().storage, (prop_id, token_group2))
            .unwrap();
        assert_eq!(updated_shares1, Decimal::percent(40));
        assert_eq!(updated_shares2, Decimal::percent(20));

        // Check the updated total power
        let updated_total_power = PROPOSAL_TOTAL_MAP
            .load(deps.as_ref().storage, prop_id)
            .unwrap();
        let expected_total_power: Decimal = total_power
            - (Decimal::percent(10) * power_ratio1)
            - (Decimal::percent(10) * power_ratio2);
        assert_eq!(updated_total_power, expected_total_power);
    }

    #[test]
    fn test_remove_many_token_group_shares_from_proposal_insufficient_shares() {
        let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let round_id = 1;
        let prop_id = 1;

        // Setup initial state
        let token_group1 = "validator1".to_string();
        let initial_shares1 = Decimal::percent(5);
        let power_ratio1 = Decimal::percent(10);

        // Mock the initial shares and power ratios
        SCALED_PROPOSAL_SHARES_MAP
            .save(
                &mut deps.storage,
                (prop_id, token_group1.clone()),
                &initial_shares1,
            )
            .unwrap();
        set_validator_power_ratio(
            &mut deps.storage,
            round_id,
            token_group1.as_str(),
            power_ratio1,
        );

        // Mock the total power
        let total_power = Decimal::percent(100);
        PROPOSAL_TOTAL_MAP
            .save(&mut deps.storage, prop_id, &total_power)
            .unwrap();

        TOKEN_INFO_PROVIDERS
            .save(
                &mut deps.storage,
                LSM_TOKEN_INFO_PROVIDER_ID.to_string(),
                &get_default_lsm_token_info_provider(),
            )
            .unwrap();
        let mut token_manager = TokenManager::new(&deps.as_ref());

        // Attempt to remove more shares than available
        let vals_and_shares = vec![(token_group1.clone(), Decimal::percent(10))];
        let result = remove_many_token_group_shares_from_proposal(
            &mut deps.as_mut(),
            &mut token_manager,
            round_id,
            prop_id,
            vals_and_shares,
        );

        // Check for error
        match result {
            Err(StdError::GenericErr { msg, .. }) => {
                assert_eq!(msg, "Insufficient shares for the token group")
            }
            _ => panic!("Expected error, but got {result:?}"),
        }
    }
}
