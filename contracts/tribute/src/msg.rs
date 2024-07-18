use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub hydro_contract: String,
    pub top_n_props_count: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    AddTribute {
        tranche_id: u64,
        proposal_id: u64,
    },
    ClaimTribute {
        round_id: u64,
        tranche_id: u64,
        tribute_id: u64,
        voter_address: String,
    },
    RefundTribute {
        round_id: u64,
        tranche_id: u64,
        proposal_id: u64,
        tribute_id: u64,
    },
}
