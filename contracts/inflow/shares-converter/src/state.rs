use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

pub const ADMIN: Item<Addr> = Item::new("admin");

/// neutron_shares_denom → cosmos_hub_shares_denom
pub const PAIRS: Map<&str, String> = Map::new("pairs");
