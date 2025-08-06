use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, StdResult, Storage, Uint128};
use cw_storage_plus::{Item, Map, SnapshotItem, Strategy};

/// Configuration of the Inflow smart contract
pub const CONFIG: Item<Config> = Item::new("config");

/// Addresses that are allowed to execute permissioned actions on the smart contract.
pub const WHITELIST: Map<Addr, ()> = Map::new("whitelist");

/// Denom of the vault shares token that is issued to users when they deposit tokens into the vault.
pub const VAULT_SHARES_DENOM: Item<String> = Item::new("vault_shares_denom");

/// Number of tokens (in terms of the deposit asset) currently deployed by the whitelisted addresses.
/// The value corresponds to the sum of principal user deposits and any yield earned through ongoing deployments.
/// It gets periodically updated by the whitelisted addresses.
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
