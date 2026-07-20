use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal};
use cw_storage_plus::{Item, Map};

pub const CONFIG: Item<Config> = Item::new("config");

pub const WHITELIST: Map<Addr, ()> = Map::new("whitelist");

// Maps round_id -> token ratio (derivative_token:base_token).
pub const TOKEN_RATIO: Map<u64, Decimal> = Map::new("token_ratio");

#[cw_serde]
pub struct Config {
    pub hydro_contract_address: Addr,
    pub derivative_token_denom: String,
    pub token_group_id: String,
    /// Maximum allowed absolute difference between two consecutive submitted ratios.
    /// Not enforced for the very first ratio submission (see `first_submission`).
    pub max_ratio_diff: Decimal,
    /// True until the first ratio-changing `SubmitTokenRatio` call succeeds; while true,
    /// `max_ratio_diff` is not enforced, allowing the initial ratio to be set freely.
    pub first_submission: bool,
}
