use crate::contract::{CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::ContractError;
use crate::msg::MigrateMsg;
use crate::state::{Constants, CONSTANTS};
use cosmwasm_std::{entry_point, DepsMut, Env, Response, StdError};
use cw2::{get_contract_version, set_contract_version};
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;

use super::v2_0_0::{migrate_v1_1_1_to_v2_0_0, MigrateMsgV2_0_0};

pub const CONTRACT_VERSION_V1_1_1: &str = "1.1.1";
pub const CONTRACT_VERSION_V2_0_0: &str = "2.0.0";

/// In the first version of Hydro, we allow contract to be un-paused through the Cosmos Hub governance
/// by migrating contract to the same code ID. This will trigger the migrate() function where we set
/// the paused flag to false.
/// Additionally, any migration logic can be added here.
/// Those migrations should check the contract version and apply the necessary changes.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    _msg: MigrateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    let contract_version = get_contract_version(deps.storage)?;
    CONSTANTS.update(
        deps.storage,
        |mut constants| -> Result<Constants, ContractError> {
            constants.paused = false;
            Ok(constants)
        },
    )?;

    if contract_version.version == CONTRACT_VERSION {
        return Err(ContractError::Std(StdError::generic_err(
            "Contract is already migrated to the newest version.",
        )));
    }

    if contract_version.version == CONTRACT_VERSION_V1_1_1 {
        migrate_v1_1_1_to_v2_0_0(
            &mut deps,
            env,
            // TODO: change migrate() parameter type & fix tests
            MigrateMsgV2_0_0 {
                max_bid_duration: 12,
            },
        )?;
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::default())
}
