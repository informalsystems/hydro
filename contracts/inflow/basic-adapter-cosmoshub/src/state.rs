use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

/// Depositor information — no capabilities, any registered depositor can withdraw
#[cw_serde]
pub struct Depositor {
    pub enabled: bool,
}

/// List of admin addresses who can manage the adapter
pub const ADMINS: Item<Vec<Addr>> = Item::new("admins");

/// Maps depositor address to their enabled status
pub const WHITELISTED_DEPOSITORS: Map<Addr, Depositor> = Map::new("whitelisted_depositors");
