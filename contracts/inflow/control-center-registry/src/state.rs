use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

pub const ADMIN: Item<Addr> = Item::new("admin");
pub const CONTROL_CENTERS: Map<Addr, ()> = Map::new("control_centers");
