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
use cosmwasm_std::{Addr, Decimal, DepsMut, Env, SignedDecimal, StdError, Storage, Uint128};
use neutron_sdk::bindings::query::NeutronQuery;
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;

type TargetVotes = HashMap<u64, Option<u64>>; // Maps lock IDs to their associated proposal IDs (or None)
type LockEntries = HashMap<u64, LockEntry>; // Maps lock IDs to their corresponding lock entries

// Validate input proposals and locks
// Returns target votes: lock_id -> proposal_id
// and lock entries: lock_id -> lock entry
pub fn validate_proposals_and_locks(
    storage: &dyn Storage,
    sender: &Addr,
    proposals_votes: &Vec<ProposalToLockups>,
) -> Result<(TargetVotes, LockEntries), ContractError> {
    // Return type aliases for improved readability

    if proposals_votes.is_empty() {
        return Err(ContractError::Std(StdError::generic_err(
            "Must provide at least one proposal and lockup to vote",
        )));
    }

    let mut proposal_ids = HashSet::new();
    let mut lock_ids = HashSet::new();
    let mut target_votes: TargetVotes = HashMap::new();
    let mut lock_entries: LockEntries = HashMap::new();

    for proposal_votes in proposals_votes {
        // Ensure each proposal ID is unique
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
            // Ensure each lock ID is unique
            if !lock_ids.insert(lock_id) {
                return Err(ContractError::Std(StdError::generic_err(format!(
                    "Duplicate lock ID {} provided",
                    lock_id
                ))));
            }

            // Validate lock belongs to sender and store entry
            let lock_entry = LOCKS_MAP.load(storage, (sender.clone(), lock_id))?;
            lock_entries.insert(lock_id, lock_entry);

            // Map lock ID to the proposal ID it votes for
            target_votes.insert(lock_id, Some(proposal_votes.proposal_id));
        }
    }

    // Ensure there is at least one valid proposal and lock ID
    if proposal_ids.is_empty() || lock_ids.is_empty() {
        return Err(ContractError::Std(StdError::generic_err(
            "Must provide at least one proposal and lockup to vote",
        )));
    }

    // Return the mapping of lock IDs to proposal IDs and the validated lock entries
    Ok((target_votes, lock_entries))
}

#[derive(Debug)]
pub struct ProcessUnvotesResult {
    pub power_changes: HashMap<u64, ProposalPowerUpdate>, // prop_id -> ProposalPowerUpdate
    pub removed_votes: HashMap<u64, Vote>,                // lock_id -> Previous vote
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
            *entry -=
                SignedDecimal::try_from(existing_vote.time_weighted_shares.1).map_err(|_| {
                    StdError::generic_err(
                        "Failed to convert Decimal to SignedDecimal for validator shares",
                    )
                })?;

            removed_votes.insert(lock_id, existing_vote.clone());

            // Always remove vote from Vote Map.
            // We cannot rely on it being overriden by the new vote (if any), as we don't know if it won't be skipped
            VOTE_MAP.remove(storage, ((round_id, tranche_id), sender.clone(), lock_id));

            // Remove voting round allowed info
            VOTING_ALLOWED_ROUND.remove(storage, (tranche_id, lock_id));
        }
    }

    Ok(ProcessUnvotesResult {
        power_changes,
        removed_votes,
        locks_skipped,
    })
}

type VoteMapKey = ((u64, u64), Addr, u64); // ((round_id, tranche_id), sender, lock_id)

#[derive(Debug)]
pub struct ProcessVotesResult {
    pub power_changes: HashMap<u64, ProposalPowerUpdate>, // prop_id -> ProposalPowerUpdate
    pub new_votes: Vec<(VoteMapKey, Vote)>, // ((round_id, tranche_id), sender, lock_id) -> Vote
    pub voting_allowed_rounds: Vec<((u64, u64), u64)>, // (tranche_id, lock_id) -> round
    pub voted_proposals: Vec<u64>,
    pub locks_voted: Vec<u64>,
    pub locks_skipped: Vec<u64>,
}

// Struct to encapsulate the context for vote processing
pub struct VoteProcessingContext<'a> {
    pub env: &'a Env,
    pub constants: &'a Constants,
    pub sender: &'a Addr,
    pub round_id: u64,
    pub tranche_id: u64,
}

pub fn process_votes(
    deps: &DepsMut<NeutronQuery>,
    context: VoteProcessingContext,
    proposals_votes: &[ProposalToLockups],
    lock_entries: &LockEntries,
    mut locks_skipped: Vec<u64>,
) -> Result<ProcessVotesResult, ContractError> {
    let round_end = compute_round_end(context.constants, context.round_id)?;
    let lock_epoch_length = context.constants.lock_epoch_length;

    let mut locks_voted = vec![];
    let mut voted_proposals = vec![];
    let mut power_changes: HashMap<u64, ProposalPowerUpdate> = HashMap::new();
    let mut new_votes = vec![];
    let mut voting_allowed_rounds = vec![];

    for proposal_to_lockups in proposals_votes {
        let proposal_id = proposal_to_lockups.proposal_id;
        let proposal = PROPOSAL_MAP.load(
            deps.storage,
            (context.round_id, context.tranche_id, proposal_id),
        )?;

        for &lock_id in &proposal_to_lockups.lock_ids {
            if locks_skipped.contains(&lock_id) {
                continue;
            }

            let lock_entry = &lock_entries[&lock_id];

            // If user didn't yet vote with the given lock in the given round and tranche, check
            let voting_allowed_round =
                VOTING_ALLOWED_ROUND.may_load(deps.storage, (context.tranche_id, lock_id))?;

            if let Some(voting_allowed_round) = voting_allowed_round {
                if voting_allowed_round > context.round_id {
                    return Err(ContractError::Std(StdError::generic_err(format!(
                        "Not allowed to vote with lock_id {} in tranche {}. Cannot vote again with this lock_id until round {}.",
                        lock_id, context.tranche_id, voting_allowed_round
                    ))));
                }
            }

            // Validate and get validator
            let validator = match validate_denom(
                deps.as_ref(),
                context.env.clone(),
                context.constants,
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
                    &context.constants.round_lock_power_schedule,
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

            if !can_lock_vote_for_proposal(
                context.round_id,
                context.constants,
                lock_entry,
                &proposal,
            )? {
                locks_skipped.push(lock_id);
                continue;
            }

            // Create new vote
            let vote = Vote {
                prop_id: proposal_id,
                time_weighted_shares: (validator.clone(), scaled_shares),
            };

            // Store vote to be saved later
            new_votes.push((
                (
                    (context.round_id, context.tranche_id),
                    context.sender.clone(),
                    lock_id,
                ),
                vote,
            ));

            // Store voting allowed round to be saved later
            let voting_allowed_round = context.round_id + proposal.deployment_duration;
            voting_allowed_rounds.push(((context.tranche_id, lock_id), voting_allowed_round));

            // Add to power changes
            let change = power_changes.entry(proposal_id).or_default();
            let entry = change.validator_shares.entry(validator).or_default();
            *entry += SignedDecimal::try_from(scaled_shares).map_err(|_| {
                StdError::generic_err(
                    "Failed to convert Decimal to SignedDecimal for validator shares",
                )
            })?;

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
