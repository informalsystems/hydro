use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Uint128};

#[cw_serde]
pub struct Config {
    /// Maximum number of tokens that can be deposited into the vault, denominated in base tokens (e.g. ATOM).
    pub deposit_cap: Uint128,
}

#[cw_serde]
pub struct UpdateConfigData {
    pub deposit_cap: Option<Uint128>,
}

#[cw_serde]
pub enum DeploymentDirection {
    Add,
    Subtract,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Submits the total amount of tokens deployed by the whitelisted addresses.
    /// Action can be performed only by the whitelisted addresses.
    SubmitDeployedAmount { amount: Uint128 },

    /// Updates the total amount of tokens deployed by adding or subtracting the specified amount.
    /// Action can be performed only by the associated sub-vault smart contracts.
    UpdateDeployedAmount { amount: Uint128, direction: DeploymentDirection },

    /// Adds a new account address to the whitelist.
    AddToWhitelist { address: String },

    /// Removes an account address from the whitelist.
    RemoveFromWhitelist { address: String },

    /// Updates the configuration of the Control Center contract.
    UpdateConfig { config: UpdateConfigData },

    /// Adds a new sub-vault smart contract to be managed by the Control Center.
    AddSubvault { address: String },

    /// Removes a sub-vault smart contract from being managed by the Control Center.
    RemoveSubvault { address: String },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},

    #[returns(Uint128)]
    DeployedAmount {},

    #[returns(PoolInfoResponse)]
    PoolInfo {},

    #[returns(WhitelistResponse)]
    Whitelist {},

    #[returns(SubvaultsResponse)]
    Subvaults {},
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct PoolInfoResponse {
    pub total_pool_value: Uint128,
    pub total_shares_issued: Uint128,
}

#[cw_serde]
pub struct WhitelistResponse {
    pub whitelist: Vec<Addr>,
}

#[cw_serde]
pub struct SubvaultsResponse {
    pub subvaults: Vec<Addr>,
}
