use cosmwasm_std::{Addr, Deps, DepsMut, StdResult};
use interface::hydro::{CurrentRoundResponse, QueryMsg as HydroQueryMsg};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::error::ContractError;

pub fn run_on_each_transaction(
    _deps: &mut DepsMut<NeutronQuery>,
    _current_round: u64,
) -> StdResult<()> {
    // TODO: temporary disabled validators store initialization, until the migration process is completed.
    // initialize_validator_store(deps.storage, current_round)?;

    Ok(())
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
