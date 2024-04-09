use cosmwasm_std::Uint128;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub denom: String,
    pub round_length: u64,
    pub total_pool: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    LockTokens { lock_duration: u64 },
    UnlockTokens {},
    CreateProposal { covenant_params: String },
    Vote { proposal_id: u64 },
    EndRound {},
    // ExecuteProposal { proposal_id: u64 },
}
