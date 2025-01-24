use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::GlobalConfig;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub hydro_contract: String,
    pub tribute_contract: String,
    pub global_config: GlobalConfig,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, cw_orch::ExecuteFns)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {}
