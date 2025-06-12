use cosmwasm_schema::cw_serde;
use cosmwasm_std::Coin;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub collections: Vec<Collection>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Collection {
    pub address: String,
    pub config: CollectionConfig,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CollectionConfig {
    pub sell_denoms: Vec<String>,
    pub royalty_fee_bps: u16,
    pub royalty_fee_recipient: String,
}

// ExecuteMsg
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Buy {
        collection: String,
        token_id: String,
    },
    Unlist {
        collection: String,
        token_id: String,
    },
    List {
        collection: String,
        token_id: String,
        price: Coin,
    },
    AddOrUpdateCollection {
        collection_address: String,
        config: CollectionConfig,
    },

    RemoveCollection {
        collection: String,
    },
    ProposeNewAdmin {
        new_admin: Option<String>,
    },
    ClaimAdminRole {},
}
