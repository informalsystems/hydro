use crate::contract::{CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::{new_generic_error, ContractError};
use crate::migration::v3_5_0::ConstantsV3_5_0;
use crate::state::{Constants, CONSTANTS};
// entry_point is being used but for some reason clippy doesn't see that, hence the allow attribute here
use cosmwasm_schema::cw_serde;
use cosmwasm_std::Order;
#[allow(unused_imports)]
use cosmwasm_std::{entry_point, DepsMut, Env, Response, StdError};
use cw2::{get_contract_version, set_contract_version};
use cw_storage_plus::Map;
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;

pub const CONTRACT_VERSION_V1_1_0: &str = "1.1.0";
pub const CONTRACT_VERSION_V2_0_1: &str = "2.0.1";
pub const CONTRACT_VERSION_V2_0_2: &str = "2.0.2";
pub const CONTRACT_VERSION_V2_1_0: &str = "2.1.0";
pub const CONTRACT_VERSION_V3_0_0: &str = "3.0.0";
pub const CONTRACT_VERSION_V3_1_0: &str = "3.1.0";
pub const CONTRACT_VERSION_V3_1_1: &str = "3.1.1";
pub const CONTRACT_VERSION_V3_2_0: &str = "3.2.0";
pub const CONTRACT_VERSION_V3_4_1: &str = "3.4.1";
pub const CONTRACT_VERSION_V3_4_2: &str = "3.4.2";

pub const LOCK_EXPIRY_DURATION_SECONDS: u64 = 60 * 60 * 24 * 180; // 180 days
pub const LOCK_DEPTH_LIMIT: u64 = 50;

#[cw_serde]
pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    mut deps: DepsMut<NeutronQuery>,
    _env: Env,
    _msg: MigrateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    check_contract_version(deps.storage)?;

    migrate_constants(&mut deps)?;

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new())
}

fn check_contract_version(storage: &dyn cosmwasm_std::Storage) -> Result<(), ContractError> {
    let contract_version = get_contract_version(storage)?;

    if contract_version.version == CONTRACT_VERSION {
        return Err(ContractError::Std(StdError::generic_err(
            "Contract is already migrated to the newest version.",
        )));
    }

    Ok(())
}

pub fn migrate_constants(deps: &mut DepsMut<NeutronQuery>) -> Result<(), ContractError> {
    const OLD_CONSTANTS: Map<u64, ConstantsV3_5_0> = Map::new("constants");

    let old_constants_entries = OLD_CONSTANTS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|result| result.ok())
        .collect::<Vec<(u64, ConstantsV3_5_0)>>();

    if old_constants_entries.is_empty() {
        return Err(new_generic_error(
            "Couldn't find any Constants in the store.",
        ));
    }

    let mut updated_constants_entries = vec![];

    for (timestamp, old_constants) in old_constants_entries {
        let new_constants = Constants {
            round_length: old_constants.round_length,
            lock_epoch_length: old_constants.lock_epoch_length,
            first_round_start: old_constants.first_round_start,
            max_locked_tokens: old_constants.max_locked_tokens,
            known_users_cap: old_constants.known_users_cap,
            paused: old_constants.paused,
            max_deployment_duration: old_constants.max_deployment_duration,
            round_lock_power_schedule: old_constants.round_lock_power_schedule,
            cw721_collection_info: old_constants.cw721_collection_info,
            lock_expiry_duration_seconds: LOCK_EXPIRY_DURATION_SECONDS,
            lock_depth_limit: LOCK_DEPTH_LIMIT,
            slash_tokens_receiver_addr: String::new(),
        };

        updated_constants_entries.push((timestamp, new_constants));
    }

    for (timestamp, new_constants) in &updated_constants_entries {
        CONSTANTS.save(deps.storage, *timestamp, new_constants)?;
    }

    Ok(())
}
