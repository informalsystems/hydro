use cosmwasm_std::{Addr, DepsMut, Response, StdResult};
use cw_storage_plus::Map;

use crate::error::ContractError;

pub fn prune_icq_managers_store(deps: &mut DepsMut) -> Result<Response, ContractError> {
    const ICQ_MANAGERS: Map<Addr, bool> = Map::new("icq_managers");

    let icq_managers = ICQ_MANAGERS
        .keys(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .collect::<StdResult<Vec<Addr>>>()?;

    for manager in icq_managers {
        ICQ_MANAGERS.remove(deps.storage, manager);
    }

    Ok(Response::new().add_attribute("action", "prune_icq_managers_store"))
}
