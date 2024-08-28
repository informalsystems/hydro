use cosmwasm_std::{Binary, Decimal, Deps, Env, Order, StdError, StdResult, Storage};
use cw_storage_plus::Map;

use ibc_proto::ibc::{
    applications::transfer::v1::{QueryDenomTraceRequest, QueryDenomTraceResponse},
    apps::transfer::v1::DenomTrace,
};
use prost::Message;

use crate::{
    contract::compute_current_round_id,
    score_keeper::{
        get_total_power_for_proposal, update_power_ratio, update_power_ratio_for_proposal,
        update_power_ratio_for_round_total,
    },
    state::{Constants, Proposal, CONSTANTS, PROPOSAL_MAP, PROPS_BY_SCORE, TRANCHE_MAP},
};

pub const IBC_TOKEN_PREFIX: &str = "ibc/";
pub const DENOM_TRACE_GRPC: &str = "/ibc.applications.transfer.v1.Query/DenomTrace";
pub const TRANSFER_PORT: &str = "transfer";
pub const COSMOS_VALIDATOR_PREFIX: &str = "cosmosvaloper";
pub const COSMOS_VALIDATOR_ADDR_LENGTH: usize = 52; // e.g. cosmosvaloper15w6ra6m68c63t0sv2hzmkngwr9t88e23r8vtg5

// For each round, stores the list of validators whose shares are eligible to vote.
// We only store the top MAX_VALIDATOR_SHARES_PARTICIPATING validators by delegated tokens,
// to avoid DoS attacks where someone creates a large number of validators with very small amounts of shares.
// VALIDATORS_PER_ROUND: key(round_id) -> Vec<validator_address>
pub const VALIDATORS_PER_ROUND: Map<u64, Vec<String>> = Map::new("validators_per_round");

// VALIDATOR_POWER_PER_ROUND: key(round_id, validator_address) -> power_ratio
pub const VALIDATOR_POWER_PER_ROUND: Map<(u64, String), Decimal> =
    Map::new("validator_power_per_round");

// Returns the validators from the store for the round.
// If the validators have not been set for the round
// (presumably because the round has not gone for long enough for them to be updated)
// it will fall back to getting the validators for the previous round.
// If those are also not set, it will return an error.
pub fn get_validators_for_round(deps: Deps, round_id: u64) -> StdResult<Vec<String>> {
    // try to get the validators for the round id
    let validators = VALIDATORS_PER_ROUND.may_load(deps.storage, round_id)?;

    // if the validators are not set for this round, try to get the validators for the previous round
    let validators = match validators {
        Some(validators) => validators,
        None => {
            // if the round id is 0, we can't get the validators for the previous round
            if round_id == 0 {
                return Err(StdError::generic_err("Validators are not set"));
            }

            // get the validators for the previous round
            VALIDATORS_PER_ROUND
                .load(deps.storage, round_id - 1)
                .map_err(|_| {
                    StdError::generic_err(format!(
                        "Failed to load validators for rounds {} and {}",
                        round_id,
                        round_id - 1
                    ))
                })?
        }
    };

    Ok(validators)
}

// Sets the validators for the current round.
// This can be called multiple times in a round, and will overwrite the previous validators
// for this round.
pub fn set_current_validators(
    storage: &mut dyn Storage,
    env: Env,
    validators: Vec<String>,
) -> StdResult<()> {
    let round_id = compute_current_round_id(&env, &CONSTANTS.load(storage)?)?;
    VALIDATORS_PER_ROUND.save(storage, round_id, &validators)?;
    Ok(())
}

pub fn set_round_validators(
    storage: &mut dyn Storage,
    validators: Vec<String>,
    round_id: u64,
) -> StdResult<()> {
    VALIDATORS_PER_ROUND.save(storage, round_id, &validators)
}

// Returns OK if the denom is a valid IBC denom representing LSM
// tokenized share transferred directly from the Cosmos Hub
// of a validator that is also currently among the top
// max_validators validators, and returns the address of that validator.
pub fn validate_denom(
    deps: Deps,
    env: Env,
    constants: &Constants,
    denom: String,
) -> StdResult<String> {
    if !denom.starts_with(IBC_TOKEN_PREFIX) {
        return Err(StdError::generic_err("IBC token expected"));
    }

    let denom_trace = query_ibc_denom_trace(deps, denom)?;

    // valid path example: transfer/channel-1
    let path_parts: Vec<&str> = denom_trace.path.split("/").collect();
    if path_parts.len() != 2
        || path_parts[0] != TRANSFER_PORT
        || path_parts[1] != constants.hub_transfer_channel_id
    {
        return Err(StdError::generic_err(
            "Only LSTs transferred directly from the Cosmos Hub can be locked.",
        ));
    }

    // valid base_denom example: cosmosvaloper16k579jk6yt2cwmqx9dz5xvq9fug2tekvlu9qdv/22409
    let base_denom_parts: Vec<&str> = denom_trace.base_denom.split("/").collect();
    if base_denom_parts.len() != 2
        || base_denom_parts[0].len() != COSMOS_VALIDATOR_ADDR_LENGTH
        || !base_denom_parts[0].starts_with(COSMOS_VALIDATOR_PREFIX)
        || base_denom_parts[1].parse::<u64>().is_err()
    {
        return Err(StdError::generic_err(
            "Only LSTs from the Cosmos Hub can be locked.",
        ));
    }

    let validator = base_denom_parts[0].to_string();
    let round_id = compute_current_round_id(&env, constants)?;
    let max_validators = constants.max_validator_shares_participating;

    let validators = get_validators_for_round(deps, round_id)?;
    if validators.contains(&validator) {
        Ok(validator)
    } else {
        Err(StdError::generic_err(format!(
            "Validator {} is not present; possibly they are not part of the top {} validators by delegated tokens",
            validator,
            max_validators
        )))
    }
}

// TODO: if round is in the future, use current powers (needed to compute the total power for the round, which
// accesses future rounds)
// TODO: if currrent round is not fully initialized, use previous round's powers
// TODO: if previous round is not fully initialized, return an error (should only happen if relaying breaks)
// TODO: docstring
pub fn get_validator_power_ratio_for_round(
    storage: &dyn Storage,
    round_id: u64,
    validator: String,
) -> StdResult<Decimal> {
    VALIDATOR_POWER_PER_ROUND.load(storage, (round_id, validator))
}

pub fn set_new_validator_power_ratio_for_round(
    storage: &mut dyn Storage,
    round_id: u64,
    validator: String,
    new_power_ratio: Decimal,
) -> StdResult<()> {
    let old_power_ratio = VALIDATOR_POWER_PER_ROUND.load(storage, (round_id, validator.clone()))?;

    VALIDATOR_POWER_PER_ROUND.save(storage, (round_id, validator.clone()), &new_power_ratio)?;

    // update the power ratio for the validator in the score keepers
    update_scores_due_to_power_ratio_change(
        storage,
        &validator,
        round_id,
        old_power_ratio,
        new_power_ratio,
    )
}

pub fn query_ibc_denom_trace(deps: Deps, denom: String) -> StdResult<DenomTrace> {
    let query_denom_trace_resp = deps.querier.query_grpc(
        String::from(DENOM_TRACE_GRPC),
        Binary::new(QueryDenomTraceRequest { hash: denom }.encode_to_vec()),
    )?;

    QueryDenomTraceResponse::decode(query_denom_trace_resp.as_slice())
        .map_err(|err| StdError::generic_err(format!("Failed to obtain IBC denom trace: {}", err)))?
        .denom_trace
        .ok_or(StdError::generic_err("Failed to obtain IBC denom trace"))
}

// Applies the new power ratio for the validator to score keepers.
// It updates:
// * all proposals of that round
// * the total power for the round
// For each proposal and for the total power,
// it will recompute the new sum by subtracting the old power ratio*that validators shares and
// adding the new power ratio*that validators shares.
pub fn update_scores_due_to_power_ratio_change(
    storage: &mut dyn Storage,
    validator: &str,
    round_id: u64,
    old_power_ratio: Decimal,
    new_power_ratio: Decimal,
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
            // update the power ratio for the proposal
            update_power_ratio_for_proposal(
                storage,
                proposal.proposal_id,
                validator.to_string(),
                old_power_ratio,
                new_power_ratio,
            )?;

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

    // update the total power in the round
    update_power_ratio_for_round_total(
        storage,
        round_id,
        validator.to_string(),
        old_power_ratio,
        new_power_ratio,
    )?;

    Ok(())
}
