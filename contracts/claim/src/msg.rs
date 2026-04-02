use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Timestamp, Uint128};

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub treasury: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Admin creates a new distribution. Must send funds along with the message.
    /// Each recipient gets a share of the sent funds proportional to their weight.
    /// The distribution expires at the given timestamp, after which unclaimed funds
    /// can be swept to the treasury.
    CreateDistribution {
        claims: Vec<ClaimEntry>,
        expiry: Timestamp,
    },
    /// Claim all pending funds across all non-expired distributions for the sender.
    Claim {},
    /// Sweep unclaimed funds from an expired distribution to the treasury.
    /// Anyone can call this.
    SweepExpired { distribution_id: u64 },
    /// Admin-only: update config fields.
    UpdateConfig {
        admin: Option<String>,
        treasury: Option<String>,
    },
}

#[cw_serde]
pub struct ClaimEntry {
    pub address: String,
    pub weight: Uint128,
}
