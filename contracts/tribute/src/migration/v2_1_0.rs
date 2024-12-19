use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// Message to migrate the contract from v2.0.4 to v2.1.0
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsgV2_1_0 {}