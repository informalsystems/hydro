use cosmwasm_schema::cw_serde;
use cosmwasm_std::{entry_point, DepsMut, Env, Order, Response, StdError, Storage};
use cw2::get_contract_version;

use crate::{
    contract::CONTRACT_VERSION,
    error::ContractError,
    state::{CONFIG, VALIDATORS_INFO, VALIDATORS_PER_ROUND, VALIDATORS_STORE_INITIALIZED},
};

#[cw_serde]
pub struct MigrateMsg {
    pub max_validator_shares_participating: u64,
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, msg: MigrateMsg) -> Result<Response, ContractError> {
    // No new version, hence no version check and no need to update the contract version in the storage.

    update_max_validator_shares_participating(
        deps.storage,
        msg.max_validator_shares_participating,
    )?;

    Ok(Response::new())
}

fn update_max_validator_shares_participating(
    storage: &mut dyn Storage,
    new_max_validator_shares_participating: u64,
) -> Result<(), ContractError> {
    let mut config = CONFIG.load(storage)?;

    if new_max_validator_shares_participating == 0
        || new_max_validator_shares_participating > config.max_validator_shares_participating
    {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "max_validator_shares_participating must be between 1 and the current value ({}), got {}",
            config.max_validator_shares_participating, new_max_validator_shares_participating
        ))));
    }

    config.max_validator_shares_participating = new_max_validator_shares_participating;
    CONFIG.save(storage, &config)?;

    let last_initialized_round = VALIDATORS_STORE_INITIALIZED
        .range(storage, None, None, Order::Descending)
        .filter_map(|entry| entry.ok().map(|(round_id, _)| round_id))
        .next();

    let Some(last_initialized_round) = last_initialized_round else {
        return Err(ContractError::Std(StdError::generic_err(
            "No rounds have been initialized yet, cannot update max_validator_shares_participating",
        )));
    };

    let validators_to_remove: Vec<(u128, String)> = VALIDATORS_PER_ROUND
        .sub_prefix(last_initialized_round)
        .range(storage, None, None, Order::Descending)
        .skip(new_max_validator_shares_participating as usize)
        .filter_map(|entry| entry.ok().map(|(key, _)| key))
        .collect();

    for (delegated_tokens, validator_address) in validators_to_remove {
        VALIDATORS_INFO.remove(storage, (last_initialized_round, validator_address.clone()));
        VALIDATORS_PER_ROUND.remove(
            storage,
            (last_initialized_round, delegated_tokens, validator_address),
        );
    }

    Ok(())
}

fn _check_contract_version(storage: &dyn cosmwasm_std::Storage) -> Result<(), ContractError> {
    let contract_version = get_contract_version(storage)?;

    if contract_version.version == CONTRACT_VERSION {
        return Err(ContractError::Std(StdError::generic_err(
            "Contract is already migrated to the newest version.",
        )));
    }

    Ok(())
}
