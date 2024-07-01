use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
