use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Uint128};

#[cw_serde]
pub struct ConfigV1_1_1 {
    pub hydro_contract: Addr,
    pub top_n_props_count: u64,
    pub min_prop_percent_for_claimable_tributes: Uint128,
}

#[cw_serde]
pub struct TributeV1_1_1 {
    pub round_id: u64,
    pub tranche_id: u64,
    pub proposal_id: u64,
    pub tribute_id: u64,
    pub depositor: Addr,
    pub funds: Coin,
    pub refunded: bool,
}
