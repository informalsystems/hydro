use cosmwasm_std::{DepsMut, Env};
use neutron_sdk::bindings::query::NeutronQuery;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::ContractError;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsgUNRELEASED {}

pub fn migrate_v2_1_0_to_unreleased(
    _deps: &mut DepsMut<NeutronQuery>,
    _env: Env,
    _msg: MigrateMsgUNRELEASED,
) -> Result<(), ContractError> {
    // TODO:
    //      1) Migrate Constants from Item to Map; Make sure that the queries for past rounds keep working.
    Ok(())
}
