use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, StdResult, Storage};
use cw_storage_plus::Item;

#[cw_serde]
pub struct Config {
    pub admins: Vec<Addr>,
    pub control_centers: Vec<Addr>,
}

pub const CONFIG: Item<Config> = Item::new("config");

pub fn load_config(storage: &dyn Storage) -> StdResult<Config> {
    CONFIG.load(storage)
}
