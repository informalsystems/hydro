use cosmwasm_std::{Decimal, Deps, Order, StdError, StdResult, Storage};

use neutron_sdk::bindings::query::NeutronQuery;
use neutron_std::types::ibc::applications::transfer::v1::{DenomTrace, TransferQuerier};

use crate::state::{
    ValidatorInfo, VALIDATORS_INFO, VALIDATORS_PER_ROUND, VALIDATORS_STORE_INITIALIZED,
};

pub const IBC_TOKEN_PREFIX: &str = "ibc/";
pub const DENOM_TRACE_GRPC: &str = "/ibc.applications.transfer.v1.Query/DenomTrace";
pub const INTERCHAINQUERIES_PARAMS_GRPC: &str = "/neutron.interchainqueries.Query/Params";
pub const TRANSFER_PORT: &str = "transfer";
pub const COSMOS_VALIDATOR_PREFIX: &str = "cosmosvaloper";
pub const COSMOS_VALIDATOR_ADDR_LENGTH: usize = 52; // e.g. cosmosvaloper15w6ra6m68c63t0sv2hzmkngwr9t88e23r8vtg5

pub fn resolve_validator_from_denom(
    deps: &Deps<NeutronQuery>,
    hub_transfer_channel_id: &str,
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
        || path_parts[1] != hub_transfer_channel_id
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
pub fn get_round_validators(deps: &Deps<NeutronQuery>, round_id: u64) -> Vec<ValidatorInfo> {
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
// This will return an error if there is an issue with parsing
// the store or the given validator is not found in the store.
pub fn get_validator_power_ratio_for_round(
    storage: &dyn Storage,
    round_id: u64,
    validator: String,
) -> StdResult<Decimal> {
    Ok(VALIDATORS_INFO
        .load(storage, (round_id, validator))?
        .power_ratio)
}

pub fn query_ibc_denom_trace(deps: &Deps<NeutronQuery>, denom: String) -> StdResult<DenomTrace> {
    TransferQuerier::new(&deps.querier)
        .denom_trace(denom)
        .map_err(|err| StdError::generic_err(format!("Failed to obtain IBC denom trace: {}", err)))?
        .denom_trace
        .ok_or(StdError::generic_err("Failed to obtain IBC denom trace"))
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
    let val_infos = load_validators_infos(storage, round_id - 1);

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
pub fn load_validators_infos(storage: &dyn Storage, round_id: u64) -> Vec<ValidatorInfo> {
    VALIDATORS_INFO
        .prefix(round_id)
        .range(storage, None, None, Order::Ascending)
        .filter_map(|val_info| match val_info {
            Err(_) => None,
            Ok(val_info) => Some(val_info.1),
        })
        .collect()
}
