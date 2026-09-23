use cosmwasm_std::{Addr, Deps, DepsMut, Order, StdError, StdResult, Storage};
use cw_storage_plus::Bound;
use interface::hydro::{CurrentRoundResponse, QueryMsg as HydroQueryMsg};
use interface::lsm::ValidatorInfo;

use crate::error::ContractError;
use crate::state::{VALIDATORS_INFO, VALIDATORS_PER_ROUND, VALIDATORS_STORE_INITIALIZED};

pub const COSMOS_VALIDATOR_PREFIX: &str = "cosmosvaloper";

pub fn run_on_each_transaction(deps: &mut DepsMut, current_round: u64) -> StdResult<()> {
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

pub fn is_validator_store_initialized(storage: &dyn Storage, round_id: u64) -> bool {
    VALIDATORS_STORE_INITIALIZED
        .load(storage, round_id)
        .unwrap_or(false)
}

// Initializes the validator store for the given round by lazily copying from previous rounds.
pub fn initialize_validator_store(storage: &mut dyn Storage, current_round: u64) -> StdResult<()> {
    let mut last_initialized_round = current_round;

    while !is_validator_store_initialized(storage, last_initialized_round) {
        if last_initialized_round == 0 {
            return Err(StdError::generic_err(
                "Cannot initialize store for the first round because it has not been initialized yet",
            ));
        }
        last_initialized_round -= 1;
    }

    initialize_validator_store_helper(storage, last_initialized_round, current_round)?;

    Ok(())
}

pub fn initialize_validator_store_helper(
    storage: &mut dyn Storage,
    last_initialized_round: u64,
    end_round: u64,
) -> StdResult<()> {
    if last_initialized_round >= end_round {
        return Ok(());
    }

    if !is_validator_store_initialized(storage, last_initialized_round) {
        return Err(StdError::generic_err(format!(
            "Cannot initialize store for next rounds because store for round {} has not been initialized yet",
            last_initialized_round,
        )));
    }

    // Load the validator information from the last initialized round to copy over to the next rounds.
    let val_infos = load_validators_infos(storage, last_initialized_round);

    // Start from the first uninitialized round and initialize all rounds up to end_round.
    let mut round_id = last_initialized_round + 1;

    while round_id <= end_round {
        for val_info in val_infos.iter() {
            let address = val_info.clone().address;

            VALIDATORS_INFO
                .save(storage, (round_id, address.clone()), val_info)
                .unwrap();

            VALIDATORS_PER_ROUND
                .save(
                    storage,
                    (round_id, val_info.delegated_tokens.u128(), address.clone()),
                    &address,
                )
                .unwrap();
        }

        VALIDATORS_STORE_INITIALIZED.save(storage, round_id, &true)?;

        round_id += 1;
    }

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

pub fn query_current_round_id(deps: &Deps, hydro_contract: &Addr) -> Result<u64, ContractError> {
    let current_round_resp: CurrentRoundResponse = deps
        .querier
        .query_wasm_smart(hydro_contract, &HydroQueryMsg::CurrentRound {})?;

    Ok(current_round_resp.round_id)
}
