use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Coin;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::{Config, Tribute};

#[derive(
    Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, QueryResponses, cw_orch::QueryFns,
)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},

    #[returns(ProposalTributesResponse)]
    ProposalTributes {
        round_id: u64,
        proposal_id: u64,
        start_from: u32,
        limit: u32,
    },
    // Returns all the tributes a certain user address has claimed.
    #[returns(HistoricalTributeClaimsResponse)]
    HistoricalTributeClaims {
        user_address: String,
        start_from: u32,
        limit: u32,
    },

    #[returns(RoundTributesResponse)]
    RoundTributes {
        round_id: u64,
        start_from: u32,
        limit: u32,
    },
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct ProposalTributesResponse {
    pub tributes: Vec<Tribute>,
}

#[cw_serde]
pub struct TributeClaim {
    pub round_id: u64,
    pub tranche_id: u64,
    pub proposal_id: u64,
    pub tribute_id: u64,
    pub amount: Coin,
}

#[cw_serde]
pub struct HistoricalTributeClaimsResponse {
    pub claims: Vec<TributeClaim>,
}

#[cw_serde]
pub struct RoundTributesResponse {
    pub tributes: Vec<Tribute>,
}
