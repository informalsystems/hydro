use cosmwasm_schema::cw_serde;
use cw_storage_plus::Item;

#[cw_serde]
pub struct Config {
    pub target_address: String,
    pub denom: String,
    pub inflow_contract: String,
    pub channel_id: String,
    pub ibc_timeout_seconds: u64,
}

pub const CONFIG: Item<Config> = Item::new("config");
