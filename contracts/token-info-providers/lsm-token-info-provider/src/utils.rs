use cosmwasm_std::{Addr, Deps, DepsMut, Order, StdError, StdResult, Storage};
use interface::hydro::{CurrentRoundResponse, QueryMsg as HydroQueryMsg};
use neutron_sdk::bindings::query::NeutronQuery;

use cw_storage_plus::Bound;
use interface::lsm::ValidatorInfo;

use crate::state::{VALIDATORS_INFO, VALIDATORS_PER_ROUND, VALIDATORS_STORE_INITIALIZED};

use crate::error::ContractError;

pub const COSMOS_VALIDATOR_PREFIX: &str = "cosmosvaloper";

pub fn run_on_each_transaction(
    deps: &mut DepsMut<NeutronQuery>,
    current_round: u64,
) -> StdResult<()> {
    initialize_validator_store(deps.storage, current_round)?;

    Ok(())
}

// For the provided round_id, finds the nearest round_id for which the store has been initialized.
// Returned round_id could also be the same as the provided round_id, which is the preferred case.
pub fn get_nearest_store_initialized_round(storage: &dyn Storage, round_id: u64) -> Option<u64> {
    VALIDATORS_STORE_INITIALIZED
        .range(
            storage,
            None,
            Some(Bound::inclusive(round_id)),
            Order::Descending,
        )
        .filter_map(|f| f.ok().map(|f| f.0))
        .next()
}

// Checks whether the store for this round has already been initialized.
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

    // save info that we have initialized the store for this round
    VALIDATORS_STORE_INITIALIZED.save(storage, round_id, &true)?;

    Ok(())
}

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

pub fn query_current_round_id(
    deps: &Deps<NeutronQuery>,
    hydro_contract: &Addr,
) -> Result<u64, ContractError> {
    let current_round_resp: CurrentRoundResponse = deps
        .querier
        .query_wasm_smart(hydro_contract, &HydroQueryMsg::CurrentRound {})?;

    Ok(current_round_resp.round_id)
}
