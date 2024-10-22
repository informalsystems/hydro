use cosmwasm_std::{entry_point, DepsMut, Env, Response, StdError, Uint128};
use cw2::{get_contract_version, set_contract_version};
use cw_storage_plus::Item;

use crate::{
    contract::{CONTRACT_NAME, CONTRACT_VERSION},
    error::ContractError,
    msg::MigrateMsg,
    state::{Config, ConfigV1, CONFIG},
};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, msg: MigrateMsg) -> Result<Response, ContractError> {
    // V1 to V2 migration
    let contract_version = get_contract_version(deps.storage)?;
    if contract_version.version == CONTRACT_VERSION {
        return Err(ContractError::Std(StdError::generic_err(
            "Contract is already migrated to the newest version.",
        )));
    }

    if msg.min_prop_percent_for_claimable_tributes > Uint128::new(100) {
        return Err(ContractError::Std(StdError::generic_err(
            "Minimum proposal percentage for claimable tributes must be between 0 and 100.",
        )));
    }

    const OLD_CONFIG: Item<ConfigV1> = Item::new("config");
    let old_config = OLD_CONFIG.load(deps.storage)?;

    let new_config = Config {
        hydro_contract: old_config.hydro_contract,
        top_n_props_count: old_config.top_n_props_count,
        min_prop_percent_for_claimable_tributes: msg.min_prop_percent_for_claimable_tributes,
    };

    CONFIG.save(deps.storage, &new_config)?;

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::default())
}
