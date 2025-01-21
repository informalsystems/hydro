use cosmwasm_std::{Decimal, Deps, Env, Order, StdError, StdResult, Storage, Uint128};

use neutron_sdk::bindings::query::NeutronQuery;
use neutron_std::types::ibc::applications::transfer::v1::{DenomTrace, TransferQuerier};

use crate::state::{
    ValidatorInfo, SCALED_ROUND_POWER_SHARES_MAP, TOTAL_VOTING_POWER_PER_ROUND, VALIDATORS_INFO,
    VALIDATORS_PER_ROUND, VALIDATORS_STORE_INITIALIZED,
};
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
    let validator = resolve_validator_from_denom(&deps, constants, denom)?;
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

pub fn resolve_validator_from_denom(
    deps: &Deps<NeutronQuery>,
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

    Ok(base_denom_parts[0].to_string())
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

fn query_ibc_denom_trace(deps: &Deps<NeutronQuery>, denom: String) -> StdResult<DenomTrace> {
    TransferQuerier::new(&deps.querier)
        .denom_trace(denom)
        .map_err(|err| StdError::generic_err(format!("Failed to obtain IBC denom trace: {}", err)))?
        .denom_trace
        .ok_or(StdError::generic_err("Failed to obtain IBC denom trace"))
}

/// Updates all the required stores each time some validator's power ratio is changed
pub fn update_stores_due_to_power_ratio_change(
    storage: &mut dyn Storage,
    current_height: u64,
    validator: &str,
    current_round_id: u64,
    old_power_ratio: Decimal,
    new_power_ratio: Decimal,
) -> StdResult<()> {
    update_scores_due_to_power_ratio_change(
        storage,
        validator,
        current_round_id,
        old_power_ratio,
        new_power_ratio,
    )?;

    update_total_power_due_to_power_ratio_change(
        storage,
        current_height,
        validator,
        current_round_id,
        old_power_ratio,
        new_power_ratio,
    )?;

    Ok(())
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

// Updates the total voting power for the current and future rounds when the given validator power ratio changes.
pub fn update_total_power_due_to_power_ratio_change(
    storage: &mut dyn Storage,
    current_height: u64,
    validator: &str,
    current_round_id: u64,
    old_power_ratio: Decimal,
    new_power_ratio: Decimal,
) -> StdResult<()> {
    let mut round_id = current_round_id;

    // Try to update the total voting power starting from the current round id and moving to next rounds until
    // we reach the round for which there is no entry in the TOTAL_VOTING_POWER_PER_ROUND. This implies the first
    // round in which no lock entry gives voting power, which also must be true for all rounds after that round,
    // so we break the loop at that point.
    loop {
        let old_total_voting_power =
            match TOTAL_VOTING_POWER_PER_ROUND.may_load(storage, round_id)? {
                None => break,
                Some(total_voting_power) => Decimal::from_ratio(total_voting_power, Uint128::one()),
            };

        let validator_shares =
            get_validator_shares_for_round(storage, round_id, validator.to_owned())?;
        if validator_shares == Decimal::zero() {
            continue;
        }

        let old_validator_shares_power = validator_shares * old_power_ratio;
        let new_validator_shares_power = validator_shares * new_power_ratio;

        let new_total_voting_power = old_total_voting_power
            .checked_add(new_validator_shares_power)?
            .checked_sub(old_validator_shares_power)?;

        TOTAL_VOTING_POWER_PER_ROUND.save(
            storage,
            round_id,
            &new_total_voting_power.to_uint_floor(),
            current_height,
        )?;

        round_id += 1;
    }

    Ok(())
}

pub fn get_total_power_for_round(deps: Deps<NeutronQuery>, round_id: u64) -> StdResult<Decimal> {
    Ok(
        match TOTAL_VOTING_POWER_PER_ROUND.may_load(deps.storage, round_id)? {
            None => Decimal::zero(),
            Some(total_voting_power) => Decimal::from_ratio(total_voting_power, Uint128::one()),
        },
    )
}

pub fn add_validator_shares_to_round_total(
    storage: &mut dyn Storage,
    current_height: u64,
    round_id: u64,
    validator: String,
    val_power_ratio: Decimal,
    num_shares: Decimal,
) -> StdResult<()> {
    // Update validator shares for the round
    let current_shares = get_validator_shares_for_round(storage, round_id, validator.clone())?;
    let new_shares = current_shares + num_shares;
    SCALED_ROUND_POWER_SHARES_MAP.save(storage, (round_id, validator.clone()), &new_shares)?;

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
                .checked_add(num_shares.checked_mul(val_power_ratio)?)?
                .to_uint_floor())
        },
    )?;

    Ok(())
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

// Checks whether the store for this round has already been initialized by copying over the information from the last round.
pub fn is_validator_store_initialized(storage: &dyn Storage, round_id: u64) -> bool {
    VALIDATORS_STORE_INITIALIZED
        .load(storage, round_id)
        .unwrap_or(false)
}

// Initializes the validator store for the given round.
// It will try to initialize the store by copying information from the last round.
// If that round has not been initialized yet, it will try to initialize *that* round
// from the round before it, and so on.
// If the validator store for the first round has not been initialized yet, it will return an error.
// (This should not happen, because the contract initializes with the first round set to initialized)
pub fn initialize_validator_store(storage: &mut dyn Storage, round_id: u64) -> StdResult<()> {
    // go back in time until we find a round that has been initialized
    let mut current_round = round_id;
    while !is_validator_store_initialized(storage, current_round) {
        if current_round == 0 {
            return Err(StdError::generic_err(
                "Cannot initialize store for the first round because it has not been initialized yet",
            ));
        }
        current_round -= 1;
    }

    // go forward and initialize the validator stores
    while current_round < round_id {
        current_round += 1;
        initialize_validator_store_helper(storage, current_round)?;
    }

    Ok(())
}

// If the store for this round has not been initialized yet, initialize_validator_store_helper copies the information from the last round
// to seed the store. This is only done starting in the second round.
// Explicitly, it initializes the VALIDATORS_INFO and the VALIDATORS_PER_ROUND
// for this round by copying the information from the previous round.
// If the store of the previous round has not been initialized yet, it returns an error.
// If the store for this round has already been initialized, or the round_id is for the first round, this function does nothing.
pub fn initialize_validator_store_helper(
    storage: &mut dyn Storage,
    round_id: u64,
) -> StdResult<()> {
    if round_id == 0 || is_validator_store_initialized(storage, round_id) {
        return Ok(());
    }

    // check that the previous round has been initialized
    if !is_validator_store_initialized(storage, round_id - 1) {
        return Err(StdError::generic_err(format!(
            "Cannot initialize store for round {} because store for round {} has not been initialized yet",
            round_id,
            round_id - 1
        )));
    }

    // copy the information from the previous round
    let val_infos = load_validators_infos(storage, round_id - 1)?;

    for val_info in val_infos {
        let address = val_info.clone().address;
        VALIDATORS_INFO
            .save(storage, (round_id, address.clone()), &val_info)
            .unwrap();

        VALIDATORS_PER_ROUND
            .save(
                storage,
                (round_id, val_info.delegated_tokens.u128(), address.clone()),
                &address,
            )
            .unwrap();
    }

    // store that we have initialized the store for this round
    VALIDATORS_STORE_INITIALIZED.save(storage, round_id, &true)?;

    Ok(())
}

// load_validators_infos needs to be its own function to borrow the storage
fn load_validators_infos(storage: &dyn Storage, round_id: u64) -> StdResult<Vec<ValidatorInfo>> {
    VALIDATORS_INFO
        .prefix(round_id)
        .range(storage, None, None, Order::Ascending)
        .map(|val_info_res| val_info_res.map(|val_info| val_info.1))
        .collect()
}
