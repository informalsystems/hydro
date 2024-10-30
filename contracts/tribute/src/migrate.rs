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
    Ok(Response::default())
}
