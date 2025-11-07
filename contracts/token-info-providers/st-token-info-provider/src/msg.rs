use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    // IBC token denom of the given stTOKEN on Neutron chain
    pub st_token_denom: String,
    // stTOKEN group identifier used in Hydro
    pub token_group_id: String,
    // Connection ID (Neutron side) to the Stride chain. Used when creating interchain query.
    pub stride_connection_id: String,
    // Number of blocks after which the ICQ result is updated
    pub icq_update_period: u64,
    // Identifier of the Stride host zone. This matches the chain ID of the blockchain for whose native token
    // the stTOKEN is issued (e.g. for stATOM, this is "cosmoshub-4", for stTIA it is "celestia", etc.)
    pub stride_host_zone_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    #[serde(rename = "register_host_zone_icq")]
    RegisterHostZoneICQ {},
    #[serde(rename = "remove_host_zone_icq")]
    RemoveHostZoneICQ {},
}
