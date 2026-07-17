use cosmwasm_schema::cw_serde;
use cosmwasm_std::Decimal;

#[cw_serde]
pub struct InstantiateMsg {
    /// Address of the Hydro contract on this chain. Defaults to the message sender if not provided.
    pub hydro_contract_address: Option<String>,
    pub derivative_token_denom: String,
    pub token_group_id: String,
    /// Maximum allowed absolute difference between two consecutive ratio submissions,
    /// enforced starting from the second submission.
    pub max_ratio_diff: Decimal,
    /// Initial set of addresses allowed to submit ratio updates and manage the whitelist.
    /// Must contain at least one address.
    pub initial_whitelist: Vec<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Submit a new derivative token ratio. Caller must be in the whitelist.
    SubmitTokenRatio { ratio: Decimal },
    /// Add an address to the whitelist. Caller must be in the whitelist.
    AddToWhitelist { address: String },
    /// Remove an address from the whitelist. Caller must be in the whitelist.
    /// Fails if removing the address would leave the whitelist empty.
    RemoveFromWhitelist { address: String },
}
