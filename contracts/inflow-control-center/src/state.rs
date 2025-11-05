use cosmwasm_std::{Addr, StdResult, Storage, Uint128};
use cw_storage_plus::{Item, Map, SnapshotItem, Strategy};
use interface::inflow_control_center::Config;

/// Configuration of the Control Center smart contract
pub const CONFIG: Item<Config> = Item::new("config");

/// Addresses that are allowed to execute permissioned actions on the smart contract.
pub const WHITELIST: Map<Addr, ()> = Map::new("whitelist");

/// Addresses of the sub-vaults managed by the Control Center.
pub const SUBVAULTS: Map<Addr, ()> = Map::new("subvaults");

/// Number of tokens (in terms of the base asset) currently deployed by the whitelisted addresses.
/// The value corresponds to the sum of principal user deposits and any yield earned through ongoing deployments.
/// It gets periodically updated by the whitelisted addresses.
pub const DEPLOYED_AMOUNT: SnapshotItem<Uint128> = SnapshotItem::new(
    "deployed_amount",
    "deployed_amount__checkpoints",
    "deployed_amount__changelog",
    Strategy::EveryBlock,
);

pub fn load_config(storage: &dyn Storage) -> StdResult<Config> {
    CONFIG.load(storage)
}
