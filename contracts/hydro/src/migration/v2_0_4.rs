use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// Message to migrate the contract from v2.0.2 to v2.0.4
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsgV2_0_4 {}
