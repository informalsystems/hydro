use cosmwasm_schema::cw_serde;
use cosmwasm_std::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cw_serde]
pub struct CommunityPoolConfig {
    // The percentage of the tribute that goes to the community pool.
    // The rest is distributed among voters.
    // Should be a number between 0 and 1.
    pub tax_percent: Decimal,
    // The channel ID to send the tokens to the community pool over.
    pub channel_id: String,
    // The address of the community pool *on the remote chain*.
    pub community_pool_address: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub hydro_contract: String,
    pub top_n_props_count: u64,
    pub community_pool_config: CommunityPoolConfig,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, cw_orch::ExecuteFns)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    #[cw_orch(payable)]
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
    ClaimCommunityPoolTribute {
        round_id: u64,
        tranche_id: u64,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsg {}
