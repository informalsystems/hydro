use cosmwasm_schema::cw_serde;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::{Config, Tribute};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    ProposalTributes {
        round_id: u64,
        tranche_id: u64,
        proposal_id: u64,
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
