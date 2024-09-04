use cosmwasm_std::{Decimal, Deps, Env, Order, StdError, StdResult, Storage};

use neutron_sdk::bindings::query::NeutronQuery;
use neutron_std::types::ibc::applications::transfer::v1::{DenomTrace, TransferQuerier};

use crate::state::{ValidatorInfo, SCALED_ROUND_POWER_SHARES_MAP, VALIDATORS_INFO};
use crate::{
    contract::compute_current_round_id,
    score_keeper::{get_total_power_for_proposal, update_power_ratio_for_proposal},
    state::{Constants, Proposal, PROPOSAL_MAP, PROPS_BY_SCORE, TRANCHE_MAP},
};

pub const IBC_TOKEN_PREFIX: &str = "ibc/";
pub const DENOM_TRACE_GRPC: &str = "/ibc.applications.transfer.v1.Query/DenomTrace";
pub const INTERCHAINQUERIES_PARAMS_GRPC: &str = "/neutron.interchainqueries.Query/Params";
pub const TRANSFER_PORT: &str = "transfer";
pub const COSMOS_VALIDATOR_PREFIX: &str = "cosmosvaloper";
pub const COSMOS_VALIDATOR_ADDR_LENGTH: usize = 52; // e.g. cosmosvaloper15w6ra6m68c63t0sv2hzmkngwr9t88e23r8vtg5

// Returns OK if the denom is a valid IBC denom representing LSM
// tokenized share transferred directly from the Cosmos Hub
// of a validator that is also currently among the top
// max_validators validators, and returns the address of that validator.
pub fn validate_denom(
    deps: Deps<NeutronQuery>,
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

    if is_active_round_validator(deps.storage, round_id, &validator) {
        Ok(validator)
    } else {
        Err(StdError::generic_err(format!(
            "Validator {} is not present; possibly they are not part of the top {} validators by delegated tokens",
            validator,
            max_validators
        )))
    }
}

pub fn is_active_round_validator(storage: &dyn Storage, round_id: u64, validator: &str) -> bool {
    VALIDATORS_INFO.has(storage, (round_id, validator.to_string()))
}

// Gets the current list of active validators for the given round
pub fn get_round_validators(deps: Deps<NeutronQuery>, round_id: u64) -> Vec<ValidatorInfo> {
    VALIDATORS_INFO
        .prefix(round_id)
        .range(deps.storage, None, None, Order::Ascending)
        .filter(|f| {
            let ok = f.is_ok();
            if !ok {
                // log an error
                deps.api.debug(&format!(
                    "failed to obtain validator info: {}",
                    f.as_ref().err().unwrap()
                ));
            }
            ok
        })
        .map(|val_res| {
            let val = val_res.unwrap();
            val.1
        })
        .collect()
}

// Gets the power of the given validator for the given round.
// This will return an error if there is an issue with parsing the store.
// Otherwise, it will return 0 if the validator is not an active validator in the round,
// and if the validator is active in the round, it will return its power ratio.
pub fn get_validator_power_ratio_for_round(
    storage: &dyn Storage,
    round_id: u64,
    validator: String,
) -> StdResult<Decimal> {
    let validator_info = VALIDATORS_INFO.may_load(storage, (round_id, validator))?;
    match validator_info {
        Some(info) => Ok(info.power_ratio),
        None => Ok(Decimal::zero()),
    }
}

pub fn query_ibc_denom_trace(deps: Deps<NeutronQuery>, denom: String) -> StdResult<DenomTrace> {
    TransferQuerier::new(&deps.querier)
        .denom_trace(denom)
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
    Ok(())
}

pub fn get_total_power_for_round(deps: Deps<NeutronQuery>, round_id: u64) -> StdResult<Decimal> {
    // get the current validators for that round
    let validators = get_round_validators(deps, round_id);

    // compute the total power
    let mut total = Decimal::zero();
    for validator in validators {
        let shares = SCALED_ROUND_POWER_SHARES_MAP
            .may_load(deps.storage, (round_id, validator.address.clone()))?
            .unwrap_or(Decimal::zero());
        total += shares * validator.power_ratio;
    }

    Ok(total)
}

pub fn add_validator_shares_to_round_total(
    storage: &mut dyn Storage,
    round_id: u64,
    validator: String,
    num_shares: Decimal,
) -> StdResult<()> {
    let current_shares = get_validator_shares_for_round(storage, round_id, validator.clone())?;
    let new_shares = current_shares + num_shares;
    SCALED_ROUND_POWER_SHARES_MAP.save(storage, (round_id, validator), &new_shares)
}

pub fn get_validator_shares_for_round(
    storage: &dyn Storage,
    round_id: u64,
    validator: String,
) -> StdResult<Decimal> {
    Ok(SCALED_ROUND_POWER_SHARES_MAP
        .may_load(storage, (round_id, validator))?
        .unwrap_or(Decimal::zero()))
}
