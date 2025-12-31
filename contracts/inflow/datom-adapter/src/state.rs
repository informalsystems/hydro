use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::Item;

#[cw_serde]
pub struct Config {
    pub drop_staking_core: Addr,
    pub drop_voucher: Addr,
    pub drop_withdrawal_manager: Addr,

    /// Vault that ultimately receives ATOM
    pub vault_contract: Addr,

    pub datom_denom: String,
    pub atom_denom: String,
}

pub const ADMINS: Item<Vec<Addr>> = Item::new("admins");
pub const EXECUTORS: Item<Vec<Addr>> = Item::new("executors");
pub const CONFIG: Item<Config> = Item::new("config");
