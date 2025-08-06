use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, StdResult, Storage, Uint128};
use cw_storage_plus::{Item, Map, SnapshotItem, Strategy};

pub const CONFIG: Item<Config> = Item::new("config");

pub const WHITELIST: Map<Addr, ()> = Map::new("whitelist");

pub const VAULT_SHARES_DENOM: Item<String> = Item::new("vault_shares_denom");

pub const TOKENS_DEPOSITED: Item<Uint128> = Item::new("tokens_deposited");

pub const DEPLOYED_AMOUNT: SnapshotItem<Uint128> = SnapshotItem::new(
    "deployed_amount",
    "deployed_amount__checkpoints",
    "deployed_amount__changelog",
    Strategy::EveryBlock,
);

#[cw_serde]
pub struct Config {
    pub deposit_denom: String,
}

pub fn load_config(storage: &dyn Storage) -> StdResult<Config> {
    CONFIG.load(storage)
}
