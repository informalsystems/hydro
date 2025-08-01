use cosmwasm_schema::cw_serde;
use serde::{Deserialize, Serialize};

// TODO: Fields description
#[cw_serde]
pub struct InstantiateMsg {
    pub deposit_denom: String,
    pub subdenom: String,
    pub token_metadata: DenomMetadata,
    pub whitelist: Vec<String>,
}

#[cw_serde]
pub struct DenomMetadata {
    /// Number of decimals used for token other than the base one (e.g. uatom has 0 decimals, atom has 6)
    pub exponent: u32,
    /// Lowercase moniker to be displayed in clients, example: "atom"
    pub display: String,
    /// Descriptive token name, example: "Cosmos Hub Atom"
    pub name: String,
    /// Even longer description, example: "The native staking token of the Cosmos Hub"
    pub description: String,
    /// Symbol is the token symbol usually shown on exchanges (eg: ATOM). This can be the same as the display.
    pub symbol: String,
    /// URI to a document (on or off-chain) that contains additional information.
    pub uri: Option<String>,
    /// URIHash is a sha256 hash of a document pointed by URI. It's used to verify that the document didn't change.
    pub uri_hash: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    Deposit {},
}

#[derive(Serialize, Deserialize)]
pub enum ReplyPayload {
    CreateDenom {
        subdenom: String,
        metadata: DenomMetadata,
    },
}
