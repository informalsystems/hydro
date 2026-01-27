use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

/// Configuration for a single dAsset token
/// Stored in TOKEN_REGISTRY, keyed by symbol (e.g., "datom")
#[cw_serde]
pub struct DAssetConfig {
    pub enabled: bool,
    /// Full denom (e.g., "factory/.../dATOM")
    pub denom: String,
    pub drop_staking_core: Addr,
    pub drop_voucher: Addr,
    pub drop_withdrawal_manager: Addr,
    /// Output denom (e.g., "ibc/.../uatom")
    pub base_asset_denom: String,
}

/// Depositor entry in the whitelist
#[cw_serde]
pub struct Depositor {
    pub enabled: bool,
}

pub const ADMINS: Item<Vec<Addr>> = Item::new("admins");
pub const EXECUTORS: Item<Vec<Addr>> = Item::new("executors");

/// Token registry: maps symbol (e.g., "datom") to DAssetConfig
pub const TOKEN_REGISTRY: Map<&str, DAssetConfig> = Map::new("token_registry");

/// Depositor whitelist: maps depositor address to Depositor entry
pub const WHITELISTED_DEPOSITORS: Map<&Addr, Depositor> = Map::new("whitelisted_depositors");
