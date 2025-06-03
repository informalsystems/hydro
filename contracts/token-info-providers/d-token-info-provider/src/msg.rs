use cosmwasm_std::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub d_token_denom: String,
    pub token_group_id: String,
    pub drop_staking_core_contract: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    UpdateTokenRatio {},
}

// TODO: The following data structure should be replaced with the modified one from the interface package once the
// LSM integration PR that introduces it gets merged.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HydroExecuteMsg {
    UpdateTokenGroupRatio {
        token_group_id: String,
        old_ratio: Decimal,
        new_ratio: Decimal,
    },
}
