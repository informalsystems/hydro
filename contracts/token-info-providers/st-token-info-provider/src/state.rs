use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Decimal};
use cw_storage_plus::{Item, Map};

pub const CONFIG: Item<Config> = Item::new("config");

// Used to store information about the interchain query ID, creator and the paid deposit.
// This information is used to prevent multiple ICQs from being created, as well as to
// authorize the interchain query removal and to refund the deposit.
pub const INTERCHAIN_QUERY_INFO: Item<InterchainQueryInfo> = Item::new("interchain_query_info");

// TOKEN_RATIO: key(round_id) -> token_ratio
pub const TOKEN_RATIO: Map<u64, Decimal> = Map::new("token_ratio");

#[cw_serde]
pub struct Config {
    pub hydro_contract_address: Addr,
    pub st_token_denom: String,
    pub token_group_id: String,
    pub stride_connection_id: String,
    pub icq_update_period: u64,
    pub stride_host_zone_id: String,
}

#[cw_serde]
pub struct InterchainQueryInfo {
    pub creator: String,
    pub query_id: u64,
    pub deposit_paid: Vec<Coin>,
}
