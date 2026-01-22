use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::Item;

#[cw_serde]
pub struct Config {
    pub drop_staking_core: Addr,
    pub drop_voucher: Addr,
    pub drop_withdrawal_manager: Addr,

    /// Vault that ultimately receives the base asset
    pub vault_contract: Addr,

    pub liquid_asset_denom: String,
    pub base_asset_denom: String,
}

pub const ADMINS: Item<Vec<Addr>> = Item::new("admins");
pub const EXECUTORS: Item<Vec<Addr>> = Item::new("executors");
pub const CONFIG: Item<Config> = Item::new("config");
