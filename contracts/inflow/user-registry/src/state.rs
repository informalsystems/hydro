use cosmwasm_std::Addr;
use cw_storage_plus::Map;

pub const ADMINS: Map<Addr, ()> = Map::new("admins");

pub const USER_PROXIES: Map<String, Addr> = Map::new("user_proxies");
