use std::collections::HashMap;

use cosmwasm_std::{
    Addr, BankMsg, Coin, Decimal, Deps, DepsMut, Env, MessageInfo, Order, Response, StdError,
    StdResult, Uint128,
};
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

use crate::{
    contract::{
        compute_current_round_id, compute_round_end, get_current_lock_composition,
        process_votes_and_apply_proposal_changes, validate_sender_is_whitelist_admin,
    },
    cw721,
    error::ContractError,
    msg::ProposalToLockups,
    score_keeper::get_token_group_shares_for_round,
    state::{
        Constants, LockEntryV2, Vote, LOCKED_TOKENS, LOCKS_MAP_V2, LOCKS_PENDING_SLASHES,
        PROPOSAL_MAP, SCALED_ROUND_POWER_SHARES_MAP, TOTAL_VOTING_POWER_PER_ROUND, TRANCHE_MAP,
        VOTE_MAP_V2, VOTING_ALLOWED_ROUND,
    },
    token_manager::TokenManager,
    utils::{
        get_highest_known_height_for_round_id, get_slice_as_attribute, load_current_constants,
        scale_lockup_power, update_user_locks,
    },
    vote::process_unvotes,
};

// SlashProposalVoters():
//     - Validate that the caller is a whitelist admin.
//     - Determine the lockups that voted on the given proposal.
//     - For each of the voted lockups, get the current lock composition. This could be the same lockup
//     that voted, or an array of successor lockups that were created from the original lockup
//     through a sequence of splitting and merging actions.
//     - For each of the lockups from the current lock composition, determine the amount to be slashed.
//     - If the amount to be slashed is greater than or equal to the slashing percentage threshold,
//     apply the slashing. Otherwise, attach the pending slash to the lockup.
//     - If there were any lockups that were slashed, update the current round votes to reflect the lockup
//     voting power change, and also update the round powers for the current and future rounds.
//     - Send the slashed tokens to the configured address.
#[allow(clippy::too_many_arguments)]
pub fn slash_proposal_voters(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    constants: &Constants,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
    slash_percent: Decimal,
    start_from: u64,
    limit: u64,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_whitelist_admin(&deps, &info)?;

    // Extract lock ids that voted on the given proposal. Note that some of those lockups
    // might not exist anymore due to splitting, merging and potentially unlocking.
    let voted_locks: Vec<(u64, Vote)> = VOTE_MAP_V2
        .prefix((round_id, tranche_id))
        .range(deps.storage, None, None, Order::Ascending)
        // If slashing is done in batches, then `start_from` and `limit` are used to paginate the votes.
        // We need to iterate over all votes to determine which lockups voted for the given proposal.
        // Batching is done before filter_map() call in order to save gas, because this way we don't need
        // to load all votes and then use only a chunk of them.
        .skip(start_from as usize)
        .take(limit as usize)
        .filter_map(|vote| match vote {
            Ok((lock_id, vote)) => {
                if vote.prop_id == proposal_id {
                    Some((lock_id, vote))
                } else {
                    None
                }
            }
            Err(_) => None,
        })
        .collect();

    let mut token_manager = TokenManager::new(&deps.as_ref());
    let current_round_id = compute_current_round_id(&env, constants)?;

    // Get latest known height for the voting round in order to be able to load the lockups that should be slashed.
    // It could happen that the last TX in round was lockups merge/split, hence the plus 1, in order not to miss
    // out on slashing those newly created lockups.
    let voting_round_latest_height =
        get_highest_known_height_for_round_id(deps.storage, round_id)? + 1;

    let mut context = SlashingContext::new();

    let tranche_ids = TRANCHE_MAP
        .keys(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<u64>>>()?;

    for voted_lock in voted_locks {
        if voted_lock.1.time_weighted_shares.1.is_zero() {
            // During split/merge of the lockups we are inserting 0-power votes into the store, if the lockup
            // being split/merged voted on some proposal in previous round. Such votes should be skipped during
            // slashing, since the associated lockups didn't actually exist when the voting happened. Note that
            // the later call to LOCKS_MAP_V2.may_load_at_height() would also return None for such lockups, so
            // this check here is just to make it more explicit and resilient to potential code changes.
            continue;
        }

        let voted_lockup = match LOCKS_MAP_V2.may_load_at_height(
            deps.storage,
            voted_lock.0,
            voting_round_latest_height,
        )? {
            None => {
                // It should never happen that a lock that voted on a proposal in the given round
                // couldn't be found, but if it does happen the only thing we can do is skip it.
                context.skipped_lockups.push(voted_lock.0);
                continue;
            }
            Some(lockup) => lockup,
        };

        for (lock_id, fraction) in get_current_lock_composition(&deps.as_ref(), voted_lock.0)? {
            let mut lockup_to_slash = match LOCKS_MAP_V2.may_load(deps.storage, lock_id)? {
                None => {
                    // Lock returned by `get_current_lock_composition()` should always exist
                    context.skipped_lockups.push(lock_id);
                    continue;
                }
                Some(lockup) => lockup,
            };

            let (amount_to_slash, _) = into_amount_to_slash(
                &deps.as_ref(),
                &mut token_manager,
                &voted_lockup,
                &lockup_to_slash,
                fraction,
                slash_percent,
                round_id,
                current_round_id,
            )?;

            // This can happen if user did vote on the given proposal, but that vote power dropped to 0 afterwards,
            // or if we cannot obtain the ratio for the token held by the given lockup.
            if amount_to_slash.is_zero() {
                context.skipped_lockups.push(lock_id);
                continue;
            }

            // Calculate the amount to be slashed by adding newly computed value to already known pending slashes.
            // If the amount to be slashed ends up being greater than total amount held by the lockup (highly unlikely),
            // then use the total lockup amount.
            let amount_to_slash = LOCKS_PENDING_SLASHES
                .may_load(deps.storage, lockup_to_slash.lock_id)?
                .unwrap_or_default()
                .checked_add(amount_to_slash)?
                .min(lockup_to_slash.funds.amount);

            // If the percentage of the lockup to be slashed is greater than or equal to the
            // slashing percentage threshold, perform the actual slashing. Otherwise, just store
            // information about the pending slash and proceed to the next lockup to be checked.
            if Decimal::from_ratio(amount_to_slash, lockup_to_slash.funds.amount)
                .ge(&constants.slash_percentage_threshold)
            {
                // Subtract the amount to slash from the lockup.
                lockup_to_slash.funds.amount =
                    lockup_to_slash.funds.amount.checked_sub(amount_to_slash)?;

                // If the amount left on the lockup is 0, the lockup should be removed from the store.
                if lockup_to_slash.funds.amount == Uint128::zero() {
                    LOCKS_MAP_V2.remove(deps.storage, lockup_to_slash.lock_id, env.block.height)?;
                    cw721::maybe_remove_token_id(deps.storage, lockup_to_slash.lock_id);

                    context
                        .users_removed_locks
                        .entry(lockup_to_slash.owner.clone())
                        .and_modify(|user_locks| user_locks.push(lockup_to_slash.lock_id))
                        .or_insert(vec![lockup_to_slash.lock_id]);

                    // `process_unvotes()` would only remove voting_allowed_round info if there is a vote in the
                    // current round. Removing this info here unconditionally will not cause any issues.
                    for tranche_id in &tranche_ids {
                        VOTING_ALLOWED_ROUND
                            .remove(deps.storage, (*tranche_id, lockup_to_slash.lock_id));
                    }
                } else {
                    LOCKS_MAP_V2.save(
                        deps.storage,
                        lockup_to_slash.lock_id,
                        &lockup_to_slash,
                        env.block.height,
                    )?;
                }

                // Remove pending slash info, since we are going to slash the lockup now.
                LOCKS_PENDING_SLASHES.remove(deps.storage, lockup_to_slash.lock_id);

                context.slashed_lockups.insert(
                    lockup_to_slash.lock_id,
                    (lockup_to_slash.clone(), amount_to_slash),
                );

                context
                    .slashed_amounts
                    .entry(lockup_to_slash.funds.denom.clone())
                    .and_modify(|value| *value += amount_to_slash)
                    .or_insert(amount_to_slash);
            } else {
                LOCKS_PENDING_SLASHES.save(
                    deps.storage,
                    lockup_to_slash.lock_id,
                    &amount_to_slash,
                )?;

                context.pending_slashes_added.push(lockup_to_slash.lock_id);
            }
        }
    }

    let response = Response::new()
        .add_attribute("action", "slash_proposal_voters")
        .add_attribute("sender", info.sender)
        .add_attribute("round_id", round_id.to_string())
        .add_attribute("tranche_id", tranche_id.to_string())
        .add_attribute("proposal_id", proposal_id.to_string())
        .add_attribute("slash_percent", slash_percent.to_string())
        .add_attribute("slashed_lockups", context.get_slashed_lockup_ids_attr())
        .add_attribute(
            "skipped_lockups",
            get_slice_as_attribute(&context.skipped_lockups),
        )
        .add_attribute(
            "pending_slashes_added",
            get_slice_as_attribute(&context.pending_slashes_added),
        );

    if context.slashed_lockups.is_empty() {
        return Ok(response);
    }

    // Reflect lockups power changes in current round votes.
    update_current_round_votes(
        &mut deps,
        &env,
        constants,
        &mut token_manager,
        current_round_id,
        &context.slashed_lockups,
        tranche_ids,
    )?;

    // For completely removed lockups, also remove them from the USER_LOCKS store
    for user_removed_locks in &context.users_removed_locks {
        update_user_locks(
            deps.storage,
            &env,
            user_removed_locks.0,
            vec![],
            user_removed_locks.1.to_owned(),
        )?;
    }

    // Update total round powers for current and all affected future rounds.
    update_rounds_powers_and_scaled_shares(
        &mut deps,
        &env,
        constants,
        &mut token_manager,
        current_round_id,
        &context.slashed_lockups,
    )?;

    let mut slashed_tokens_num: u128 = 0;
    let coins_to_send: Vec<Coin> = context
        .slashed_amounts
        .iter()
        .map(|slashed_tokens| {
            slashed_tokens_num += slashed_tokens.1.u128();

            Coin::new(*slashed_tokens.1, slashed_tokens.0)
        })
        .collect();

    // Update the number of total locked tokens, since slashing opens up new locking capacity.
    LOCKED_TOKENS.update(
        deps.storage,
        |current_locked_tokens| -> Result<u128, ContractError> {
            Ok(current_locked_tokens.saturating_sub(slashed_tokens_num))
        },
    )?;

    let response = response
        .add_attribute("slashed_amounts", get_slice_as_attribute(&coins_to_send))
        .add_attribute("total_tokens_slashed", slashed_tokens_num.to_string())
        .add_message(BankMsg::Send {
            to_address: constants.slash_tokens_receiver_addr.clone(),
            amount: coins_to_send,
        });

    Ok(response)
}

// Calculate the amount to slash in the resulting lockup entry as following:
//      amount_to_slash = voted_lockup.funds.amount * fraction * slash_percent
//
// The amount to slash is represented in the denom of the tokens that the `lockup_to_slash` holds.
// If the lockup being slashed holds tokens of a different denom than the voted lockup,
// the amount to slash is calculated by converting the `voted_lockup` token amount into
// the `lockup_to_slash` token amount.
// Returns the amount to slash denominated in the slashed lockup denom and the ratio of that token
// towards the base token (e.g. ATOM).
#[allow(clippy::too_many_arguments)]
pub fn into_amount_to_slash(
    deps: &Deps<NeutronQuery>,
    token_manager: &mut TokenManager,
    voted_lockup: &LockEntryV2,
    lockup_to_slash: &LockEntryV2,
    fraction: Decimal,
    slash_percent: Decimal,
    voting_round: u64,
    slashing_round: u64,
) -> Result<(Uint128, Decimal), ContractError> {
    // Get the voted lockup token ratio for the round in which it voted on a proposal we want to slash.
    let vote_token_ratio =
        token_manager.get_token_denom_ratio(deps, voting_round, voted_lockup.funds.denom.clone());

    // If the ratio of the token droped to 0 after user had voted, we should not slash such voters,
    // since their vote didn't contribute to the proposal voting power, nor did they receive any tribute.
    if vote_token_ratio.is_zero() {
        return Ok((Uint128::zero(), Decimal::zero()));
    }

    // Get the token ratio of the lockup being slashed, for the round in which the slashing is performed.
    // Note that we are not trying to obtain the ratio for the round in which the voting happend, since
    // the given token might not even be allowed to be locked in that round.
    let slash_token_ratio = token_manager.get_token_denom_ratio(
        deps,
        slashing_round,
        lockup_to_slash.funds.denom.clone(),
    );

    // If the token ratio of the lockup being slashed droped to 0 in the current round, we will not slash it.
    if slash_token_ratio.is_zero() {
        return Ok((Uint128::zero(), Decimal::zero()));
    }

    // If the lockup denoms are the same, calculate slashable amount just as a `fraction` of the `voted_lockup`
    // multiplied by the `slash_percent`. If we were to apply the rest of the logic, especially in case of LSTs,
    // we would end up with smaller amount to slash, since the ratio changes over time (e.g. if the lockup in
    // round 0 had 100 dATOM, and the ratio was 1.15, if we are slashing in round 3 when the ratio is 1.2,
    // the amount to slash would be a `fraction` of ~96 dATOM, while it should actually be a `fraction` of 100 dATOM).
    if voted_lockup.funds.denom == lockup_to_slash.funds.denom {
        let amount_to_slash = Decimal::from_ratio(voted_lockup.funds.amount, Uint128::one())
            .checked_mul(fraction)?
            .checked_mul(slash_percent)?
            .to_uint_floor();

        if amount_to_slash > lockup_to_slash.funds.amount {
            return Ok((lockup_to_slash.funds.amount, slash_token_ratio));
        } else {
            return Ok((amount_to_slash, slash_token_ratio));
        }
    }

    // Calculate the slashable amount converted into number of base tokens (e.g. ATOM).
    // Take into account only the fraction that ended up being part of the lockup currently being slashed.
    let amount_to_slash_base_tokens =
        Decimal::from_ratio(voted_lockup.funds.amount, Uint128::one())
            .checked_mul(fraction)?
            .checked_mul(slash_percent)?
            .checked_mul(vote_token_ratio)?;

    let amount_to_slash = amount_to_slash_base_tokens
        .checked_div(slash_token_ratio)?
        .to_uint_floor();

    if amount_to_slash > lockup_to_slash.funds.amount {
        return Ok((lockup_to_slash.funds.amount, slash_token_ratio));
    }

    Ok((amount_to_slash, slash_token_ratio))
}

// Remove potential current round votes for all lockups that were affected by slashing.
// Then re-add previously removed votes, but only for those lockups that were partially slashed.
fn update_current_round_votes(
    deps: &mut DepsMut<NeutronQuery>,
    env: &Env,
    constants: &Constants,
    token_manager: &mut TokenManager,
    current_round_id: u64,
    slashed_lockups: &HashMap<u64, (LockEntryV2, Uint128)>,
    tranche_ids: Vec<u64>,
) -> Result<(), ContractError> {
    // Remove all votes for the slashed lockups in the current round.
    let target_votes: HashMap<u64, Option<u64>> = HashMap::from_iter(
        slashed_lockups
            .iter()
            .map(|slash_info| (*slash_info.0, None)),
    );

    for tranche_id in tranche_ids {
        let unvotes_result =
            process_unvotes(deps.storage, current_round_id, tranche_id, &target_votes)?;

        // Prepare votes to be re-added in the current round and tranche, for partially slashed lockups
        let votes = slashed_lockups
            .iter()
            .filter_map(|slashed_lockup_info| {
                let slashed_lockup_id = slashed_lockup_info.1 .0.lock_id;

                // Votes should be re-added only for those lockups that were not entirely removed
                if slashed_lockup_info.1 .0.funds.amount == Uint128::zero() {
                    return None;
                }

                unvotes_result
                    .removed_votes
                    // Together with `filter_map()` this call will filter out lockups that didn't vote
                    .get(&slashed_lockup_id)
                    .map(|vote| (vote.prop_id, slashed_lockup_id))
            })
            .fold(
                HashMap::new(),
                |mut acc: HashMap<u64, Vec<u64>>, (prop_id, lock_id)| {
                    acc.entry(prop_id).or_default().push(lock_id);

                    acc
                },
            )
            .iter()
            .map(|(proposal_id, lock_ids)| ProposalToLockups {
                proposal_id: *proposal_id,
                lock_ids: lock_ids.clone(),
            })
            .collect::<Vec<ProposalToLockups>>();

        // Prepare lock entries to be used in `process_votes_and_apply_proposal_changes()`
        // by filtering out those lockups that were entirely slashed and removed.
        let lock_entries: HashMap<u64, LockEntryV2> = slashed_lockups
            .iter()
            .filter_map(|slashed_lockup_info| {
                if slashed_lockup_info.1 .0.funds.amount == Uint128::zero() {
                    return None;
                }

                Some((
                    slashed_lockup_info.1 .0.lock_id,
                    slashed_lockup_info.1 .0.clone(),
                ))
            })
            .collect();

        process_votes_and_apply_proposal_changes(
            deps,
            env,
            token_manager,
            constants,
            current_round_id,
            tranche_id,
            &votes,
            &lock_entries,
            unvotes_result,
        )?;
    }

    Ok(())
}

fn update_rounds_powers_and_scaled_shares(
    deps: &mut DepsMut<NeutronQuery>,
    env: &Env,
    constants: &Constants,
    token_manager: &mut TokenManager,
    current_round_id: u64,
    slashed_lockups: &HashMap<u64, (LockEntryV2, Uint128)>,
) -> Result<(), ContractError> {
    let highest_round_id_with_power = current_round_id
        + constants
            .round_lock_power_schedule
            .get_maximum_rounds_to_lock();

    let rounds_to_update: Vec<(u64, u64)> = (current_round_id..=highest_round_id_with_power)
        .filter_map(|round_id| match compute_round_end(constants, round_id) {
            Err(_) => None,
            Ok(round_end) => Some((round_id, round_end.nanos())),
        })
        .collect();

    let mut round_scaled_shares_changes: HashMap<u64, HashMap<String, Decimal>> = HashMap::new();

    // Iterate over all slashed lockups and calculate the scaled shares changes for each round
    for slashed_lockup_info in slashed_lockups.values() {
        let slashed_lockup = &slashed_lockup_info.0;
        let slashed_amount = slashed_lockup_info.1;

        // `slashed_lockup` already contains updated amount of tokens
        let old_lockup_amount = slashed_lockup.funds.amount.checked_add(slashed_amount)?;
        let new_lockup_amount = slashed_lockup.funds.amount;
        let lockup_end = slashed_lockup.lock_end.nanos();

        for round_info in &rounds_to_update {
            let round_id = round_info.0;
            let round_end = round_info.1;

            // If the lockup is expired in this round, it will also be expired in all subsequent rounds
            if lockup_end < round_end {
                break;
            }

            let lockup_length = lockup_end - round_end;

            let old_lockup_power = scale_lockup_power(
                &constants.round_lock_power_schedule,
                constants.lock_epoch_length,
                lockup_length,
                old_lockup_amount,
            );

            let new_lockup_power = scale_lockup_power(
                &constants.round_lock_power_schedule,
                constants.lock_epoch_length,
                lockup_length,
                new_lockup_amount,
            );

            let scaled_shares_diff = Decimal::from_ratio(
                old_lockup_power.checked_sub(new_lockup_power)?,
                Uint128::one(),
            );

            let token_group_id = match token_manager.validate_denom(
                &deps.as_ref(),
                // Token ratios are populated only until the current round
                current_round_id,
                slashed_lockup.funds.denom.clone(),
            ) {
                Err(_) => break,
                Ok(token_group_id) => token_group_id,
            };

            // Accumulate scaled shares changes per (round_id, token_group_id) in order to apply them later
            round_scaled_shares_changes
                .entry(round_id)
                .and_modify(|round_scaled_shares_updates| {
                    round_scaled_shares_updates
                        .entry(token_group_id.clone())
                        .and_modify(|scaled_shares_change| {
                            *scaled_shares_change += scaled_shares_diff
                        })
                        .or_insert(scaled_shares_diff);
                })
                .or_insert(HashMap::from_iter([(
                    token_group_id.clone(),
                    scaled_shares_diff,
                )]));
        }
    }

    // Update scaled round power shares only once per each (round_id, token_group_id) pair.
    // Also update the total voting power for each round, since the scaled shares have changed.
    for (round_id, token_groups_changes) in round_scaled_shares_changes {
        let mut round_total_power_change = Decimal::zero();

        for token_group_changes in token_groups_changes {
            // Update this map even if the token group ratio is 0, since it might become greater than 0 in the future
            let old_shares = get_token_group_shares_for_round(
                deps.storage,
                round_id,
                token_group_changes.0.clone(),
            )?;

            let new_shares = if old_shares > token_group_changes.1 {
                old_shares.checked_sub(token_group_changes.1)?
            } else {
                Decimal::zero()
            };

            SCALED_ROUND_POWER_SHARES_MAP.save(
                deps.storage,
                (round_id, token_group_changes.0.clone()),
                &new_shares,
            )?;

            // If the token group ratio cannot be obtained (or it has 0 value), it means that the given token
            // group is already excluded from the round total power, so there is nothing to be updated here.
            let token_group_ratio = match token_manager.get_token_group_ratio(
                &deps.as_ref(),
                // Token ratios are populated only until the current round
                current_round_id,
                token_group_changes.0.clone(),
            ) {
                Err(_) => continue,
                Ok(token_group_ratio) => {
                    if token_group_ratio == Decimal::zero() {
                        continue;
                    }

                    token_group_ratio
                }
            };

            let old_shares_power = old_shares.checked_mul(token_group_ratio)?;
            let new_shares_power = new_shares.checked_mul(token_group_ratio)?;
            let power_change = old_shares_power.checked_sub(new_shares_power)?;

            round_total_power_change = round_total_power_change.checked_add(power_change)?;
        }

        TOTAL_VOTING_POWER_PER_ROUND.update(
            deps.storage,
            round_id,
            env.block.height,
            |old_total_power| -> Result<Uint128, StdError> {
                let old_total_power = match old_total_power {
                    None => Decimal::zero(),
                    Some(total_power_before) => {
                        Decimal::from_ratio(total_power_before, Uint128::one())
                    }
                };

                let new_total_power = if old_total_power > round_total_power_change {
                    old_total_power.checked_sub(round_total_power_change)?
                } else {
                    Decimal::zero()
                };

                Ok(new_total_power.to_uint_ceil())
            },
        )?;
    }

    Ok(())
}

/// Returns the maximum number of tokens held by the lockups that can be slashed for voting
/// on the given proposal. The amount returned is denominated in the base token (e.g. ATOM).
pub fn query_slashable_token_num_for_voting_on_proposal(
    deps: Deps<NeutronQuery>,
    env: Env,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> StdResult<Uint128> {
    let constants = load_current_constants(&deps, &env)?;
    let current_round_id = compute_current_round_id(&env, &constants)?;

    if round_id > current_round_id {
        return Err(StdError::generic_err(
            "cannot query slashable tokens number for the future round",
        ));
    }

    if !PROPOSAL_MAP.has(deps.storage, (round_id, tranche_id, proposal_id)) {
        return Err(StdError::generic_err(format!("proposal with id {proposal_id} in round {round_id} and tranche {tranche_id} does not exist")));
    }

    let voted_locks: Vec<(u64, Vote)> = VOTE_MAP_V2
        .prefix((round_id, tranche_id))
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|vote| match vote {
            Ok((lock_id, vote)) => {
                if vote.prop_id == proposal_id {
                    Some((lock_id, vote))
                } else {
                    None
                }
            }
            Err(_) => None,
        })
        .collect();

    let mut token_manager = TokenManager::new(&deps);

    let voting_round_latest_height =
        get_highest_known_height_for_round_id(deps.storage, round_id)? + 1;

    let mut max_amount_to_slash_base_tokens = Uint128::zero();

    for voted_lock in voted_locks {
        if voted_lock.1.time_weighted_shares.1.is_zero() {
            continue;
        }

        let Some(voted_lockup) = LOCKS_MAP_V2.may_load_at_height(
            deps.storage,
            voted_lock.0,
            voting_round_latest_height,
        )?
        else {
            continue;
        };

        for (lock_id, fraction) in get_current_lock_composition(&deps, voted_lock.0)? {
            let Some(lockup_to_slash) = LOCKS_MAP_V2.may_load(deps.storage, lock_id)? else {
                continue;
            };

            // pretend as if we are going to slash 100% of the lockup,
            // in order to calculate the maximum that can be slashed.
            let (max_amount_to_slash, ratio_to_base_token) = into_amount_to_slash(
                &deps,
                &mut token_manager,
                &voted_lockup,
                &lockup_to_slash,
                fraction,
                Decimal::percent(100),
                round_id,
                current_round_id,
            )
            .map_err(|e| {
                StdError::generic_err(format!(
                    "failed to compute maximum value that can be slashed for lockup {}, error: {e}",
                    lockup_to_slash.lock_id
                ))
            })?;

            if max_amount_to_slash.is_zero() {
                continue;
            }

            max_amount_to_slash_base_tokens = max_amount_to_slash_base_tokens.checked_add(
                Decimal::from_ratio(max_amount_to_slash, Uint128::one())
                    .checked_mul(ratio_to_base_token)?
                    .to_uint_floor(),
            )?;
        }
    }

    Ok(max_amount_to_slash_base_tokens)
}

struct SlashingContext {
    // Used to collect all lockups that were actually slashed (i.e. some amount of tokens was taken from the lockup)
    // and the corresponding slashed amounts. This info is later used to prepare the inputs for unvoting and revoting
    // in the current round.
    pub slashed_lockups: HashMap<u64, (LockEntryV2, Uint128)>,

    // Lockup IDs that had pending slashes attached to them during execution.
    pub pending_slashes_added: Vec<u64>,

    // Used to collect all lockups that should have been slashed but were skipped due to them being
    // related to 0 power votes or the token power ratio couldn't be obtained in order to slash them.
    pub skipped_lockups: Vec<u64>,

    // The number of tokens that were actualy slashed during the execution, per token denom.
    pub slashed_amounts: HashMap<String, Uint128>,

    // Keep track of user lockups removed during slashing in order to remove them all at once from USER_LOCKS.
    pub users_removed_locks: HashMap<Addr, Vec<u64>>,
}

impl SlashingContext {
    pub fn new() -> Self {
        SlashingContext {
            slashed_lockups: HashMap::new(),
            pending_slashes_added: vec![],
            skipped_lockups: vec![],
            slashed_amounts: HashMap::new(),
            users_removed_locks: HashMap::new(),
        }
    }

    pub fn get_slashed_lockup_ids_attr(&self) -> String {
        self.slashed_lockups
            .keys()
            .map(|lock_id| lock_id.to_string())
            .collect::<Vec<String>>()
            .join(", ")
    }
}
