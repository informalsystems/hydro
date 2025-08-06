use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;
use serde::{Deserialize, Serialize};

#[cw_serde]
pub struct InstantiateMsg {
    /// The denom of the token that can be deposited into the vault.
    pub deposit_denom: String,
    /// Inflow vault shares token subdenom. Used to derive the full token denom.
    /// E.g. if the subdenom is "hydro_inflow_uatom" then the full denom will be
    /// "factory/{inflow_contract_address}/hydro_inflow_uatom"
    pub subdenom: String,
    /// Additional metadata to be set for the newly created vault shares token.
    pub token_metadata: DenomMetadata,
    /// List of addresses allowed to execute permissioned actions.
    pub whitelist: Vec<String>,
}

#[cw_serde]
pub struct DenomMetadata {
    /// Number of decimals used for denom unit other than the base one.
    /// E.g. "uatom" as a base denom unit has 0 decimals, and "atom" would have 6.
    pub exponent: u32,
    /// Lowercase moniker to be displayed in clients, example: "atom"
    /// Also used as a denom for the non-base denom unit.
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
    WithdrawForDeployment { amount: Uint128 },
    AddToWhitelist { address: String },
    RemoveFromWhitelist { address: String },
}

#[derive(Serialize, Deserialize)]
pub enum ReplyPayload {
    CreateDenom {
        subdenom: String,
        metadata: DenomMetadata,
    },
}
