use cosmwasm_std::{Addr, Decimal, StdResult, Storage, Uint128};
use cw_storage_plus::{Item, Map, SnapshotItem, Strategy};
use interface::inflow_control_center::{Config, FeeConfig};

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

/// Fee configuration for performance fee tracking
pub const FEE_CONFIG: Item<FeeConfig> = Item::new("fee_config");

/// High-water mark: the share price at the last fee accrual.
/// Fees are only charged when the current share price exceeds this value.
pub const HIGH_WATER_MARK_PRICE: Item<Decimal> = Item::new("high_water_mark_price");

pub fn load_config(storage: &dyn Storage) -> StdResult<Config> {
    CONFIG.load(storage)
}

pub fn load_fee_config(storage: &dyn Storage) -> StdResult<FeeConfig> {
    FEE_CONFIG.load(storage)
}
