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
    //      2) TOTAL_VOTING_POWER_PER_ROUND needs to be correctly populated regardless of the point in time
    //         we do the migration. Needs to be populated for future rounds as well. If we populate it for
    //         the past rounds as well, we can use that in our queries instead of on-the-fly computation
    //         e.g. query_round_total_power(), query_top_n_proposals().
    //      3) LOCKS_MAP needs to be migrated to SnapshotMap.
    //      4) Populate USER_LOCKS for existing lockups.
    //      4) Populate ROUND_TO_HEIGHT_RANGE and HEIGHT_TO_ROUND for previous rounds?
    Ok(())
}
