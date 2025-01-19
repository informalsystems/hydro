use crate::contract::{
    can_lock_vote_for_proposal, compute_round_end, get_lock_time_weighted_shares,
};
use crate::error::ContractError;
use crate::lsm_integration::validate_denom;
use crate::msg::ProposalToLockups;
use crate::score_keeper::ProposalPowerUpdate;
use crate::state::{
    Constants, LockEntry, Vote, LOCKS_MAP, PROPOSAL_MAP, VOTE_MAP, VOTING_ALLOWED_ROUND,
};
use cosmwasm_std::{Addr, Decimal, DepsMut, Env, StdError, Storage, Uint128};
use neutron_sdk::bindings::query::NeutronQuery;
use std::collections::{HashMap, HashSet};

// Validate input proposals and locks
pub fn validate_proposals_and_locks(
    storage: &dyn Storage,
    sender: &Addr,
    proposals_votes: &Vec<ProposalToLockups>,
) -> Result<(HashMap<u64, Option<u64>>, HashMap<u64, LockEntry>), ContractError> {
    if proposals_votes.is_empty() {
        return Err(ContractError::Std(StdError::generic_err(
            "Must provide at least one proposal and lockup to vote",
        )));
    }

    let mut proposal_ids = HashSet::new();
    let mut lock_ids = HashSet::new();
    let mut target_votes = HashMap::new();
    let mut lock_entries = HashMap::new();

    for proposal_votes in proposals_votes {
        if !proposal_ids.insert(proposal_votes.proposal_id) {
            return Err(ContractError::Std(StdError::generic_err(format!(
                "Duplicate proposal ID {} provided",
                proposal_votes.proposal_id
            ))));
        }

        if proposal_votes.lock_ids.is_empty() {
            return Err(ContractError::Std(StdError::generic_err(format!(
                "No lock IDs provided to vote for proposal ID {}",
                proposal_votes.proposal_id
            ))));
        }

        for &lock_id in &proposal_votes.lock_ids {
            if !lock_ids.insert(lock_id) {
                return Err(ContractError::Std(StdError::generic_err(format!(
                    "Duplicate lock ID {} provided",
                    lock_id
                ))));
            }

            // Validate lock belongs to sender and store entry
            let lock_entry = LOCKS_MAP.load(storage, (sender.clone(), lock_id))?;
            lock_entries.insert(lock_id, lock_entry);
            target_votes.insert(lock_id, Some(proposal_votes.proposal_id));
        }
    }

    if proposal_ids.is_empty() || lock_ids.is_empty() {
        return Err(ContractError::Std(StdError::generic_err(
            "Must provide at least one proposal and lockup to vote",
        )));
    }

    Ok((target_votes, lock_entries))
}

#[derive(Debug)]
pub struct ProcessUnvotesResult {
    pub power_changes: HashMap<u64, ProposalPowerUpdate>,
    pub removed_votes: HashMap<u64, Vote>, // (lock_id) -> Previous vote
    pub unvoted_proposals: HashSet<u64>,
    pub locks_skipped: Vec<u64>,
}
pub fn process_unvotes(
    storage: &mut dyn Storage,
    sender: &Addr,
    round_id: u64,
    tranche_id: u64,
    target_votes: &HashMap<u64, Option<u64>>,
) -> Result<ProcessUnvotesResult, ContractError> {
    let mut power_changes: HashMap<u64, ProposalPowerUpdate> = HashMap::new();
    let mut removed_votes: HashMap<u64, Vote> = HashMap::new();
    let mut unvoted_proposals: HashSet<u64> = HashSet::new();
    let mut locks_skipped = Vec::new();

    for (&lock_id, &target_proposal_id) in target_votes {
        if let Some(existing_vote) =
            VOTE_MAP.may_load(storage, ((round_id, tranche_id), sender.clone(), lock_id))?
        {
            // Skip if we have a target proposal and it matches the current vote
            if let Some(target_id) = target_proposal_id {
                if existing_vote.prop_id == target_id {
                    locks_skipped.push(lock_id);
                    continue;
                }
            }

            let change = power_changes.entry(existing_vote.prop_id).or_default();

            // Subtract validator shares
            let entry = change
                .validator_shares
                .entry(existing_vote.time_weighted_shares.0.clone())
                .or_default();
            *entry += existing_vote.time_weighted_shares.1;

            // Store removed vote only if there was no target proposal (pure unvote)
            if target_proposal_id.is_none() {
                removed_votes.insert(lock_id, existing_vote.clone());
            }

            // Remove vote
            VOTE_MAP.remove(storage, ((round_id, tranche_id), sender.clone(), lock_id));

            // Remove voting round allowed info
            VOTING_ALLOWED_ROUND.remove(storage, (tranche_id, lock_id));

            unvoted_proposals.insert(existing_vote.prop_id);
        }
    }

    Ok(ProcessUnvotesResult {
        power_changes,
        removed_votes,
        unvoted_proposals,
        locks_skipped,
    })
}

#[derive(Debug)]
pub struct ProcessVotesResult {
    pub power_changes: HashMap<u64, ProposalPowerUpdate>,
    pub new_votes: Vec<(((u64, u64), Addr, u64), Vote)>, // ((round_id, tranche_id), sender, lock_id) -> Vote
    pub voting_allowed_rounds: Vec<((u64, u64), u64)>,   // (tranche_id, lock_id) -> round
    pub voted_proposals: Vec<u64>,
    pub locks_voted: Vec<u64>,
    pub locks_skipped: Vec<u64>,
}

pub fn process_votes(
    deps: &DepsMut<NeutronQuery>,
    env: &Env,
    constants: &Constants,
    sender: &Addr,
    round_id: u64,
    tranche_id: u64,
    proposals_votes: &[ProposalToLockups],
    lock_entries: &HashMap<u64, LockEntry>,
    mut locks_skipped: Vec<u64>,
) -> Result<ProcessVotesResult, ContractError> {
    let round_end = compute_round_end(constants, round_id)?;
    let lock_epoch_length = constants.lock_epoch_length;

    let mut locks_voted = vec![];
    let mut voted_proposals = vec![];
    let mut power_changes: HashMap<u64, ProposalPowerUpdate> = HashMap::new();
    let mut new_votes = vec![];
    let mut voting_allowed_rounds = vec![];

    for proposal_to_lockups in proposals_votes {
        let proposal_id = proposal_to_lockups.proposal_id;
        let proposal = PROPOSAL_MAP.load(deps.storage, (round_id, tranche_id, proposal_id))?;

        for &lock_id in &proposal_to_lockups.lock_ids {
            if locks_skipped.contains(&lock_id) {
                continue;
            }

            let lock_entry = &lock_entries[&lock_id];

            // If user didn't yet vote with the given lock in the given round and tranche, check
            // if they voted in previous rounds for some proposal that spans multiple rounds.
            // This means that users can change their vote during a round, because we don't
            // check this if users already voted in the current round.
            let voting_allowed_round =
                VOTING_ALLOWED_ROUND.may_load(deps.storage, (tranche_id, lock_id))?;

            if let Some(voting_allowed_round) = voting_allowed_round {
                if voting_allowed_round > round_id {
                    return Err(ContractError::Std(
                    StdError::generic_err(format!(
                        "Not allowed to vote with lock_id {} in tranche {}. Cannot vote again with this lock_id until round {}.",
                        lock_id, tranche_id, voting_allowed_round))));
                }
            }

            // Validate and get validator
            let validator = match validate_denom(
                deps.as_ref(),
                env.clone(),
                constants,
                lock_entry.clone().funds.denom,
            ) {
                Ok(validator) => validator,
                Err(_) => {
                    deps.api.debug(&format!(
                        "Denom {} is not a valid validator denom",
                        lock_entry.funds.denom
                    ));
                    locks_skipped.push(lock_id);
                    continue;
                }
            };

            let scaled_shares = Decimal::from_ratio(
                get_lock_time_weighted_shares(
                    &constants.round_lock_power_schedule,
                    round_end,
                    lock_entry.clone(),
                    lock_epoch_length,
                ),
                Uint128::one(),
            );

            if scaled_shares.is_zero() {
                locks_skipped.push(lock_id);
                continue;
            }

            if !can_lock_vote_for_proposal(round_id, constants, lock_entry, &proposal)? {
                locks_skipped.push(lock_id);
                continue;
            }

            // Create new vote
            let vote = Vote {
                prop_id: proposal_id,
                time_weighted_shares: (validator.clone(), scaled_shares),
            };

            // Store vote to be saved later
            new_votes.push((((round_id, tranche_id), sender.clone(), lock_id), vote));

            // Store voting allowed round to be saved later
            let voting_allowed_round = round_id + proposal.deployment_duration;
            voting_allowed_rounds.push(((tranche_id, lock_id), voting_allowed_round));

            // Add to power changes
            let change = power_changes.entry(proposal_id).or_default();
            let entry = change.validator_shares.entry(validator).or_default();
            *entry += scaled_shares;

            locks_voted.push(lock_id);
        }

        voted_proposals.push(proposal_id);
    }

    Ok(ProcessVotesResult {
        power_changes,
        new_votes,
        voting_allowed_rounds,
        voted_proposals,
        locks_voted,
        locks_skipped,
    })
}
