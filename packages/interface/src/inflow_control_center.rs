use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Decimal, Uint128};

#[cw_serde]
pub struct Config {
    /// Maximum number of tokens that can be deposited into the vault, denominated in base tokens (e.g. ATOM).
    pub deposit_cap: Uint128,
}

#[cw_serde]
pub struct UpdateConfigData {
    pub deposit_cap: Option<Uint128>,
}

/// Configuration for performance fee tracking, used during instantiation or migration.
/// Fees are enabled when fee_rate > 0 and disabled when fee_rate = 0.
#[cw_serde]
pub struct FeeConfigInit {
    /// Fee rate as a decimal (e.g., 0.2 for 20%). Set to 0 to disable fees.
    pub fee_rate: Decimal,
    /// Address where fee shares are minted to
    pub fee_recipient: String,
}

/// Stored fee configuration.
/// Fees are enabled when fee_rate > 0 and disabled when fee_rate = 0.
#[cw_serde]
pub struct FeeConfig {
    /// Fee rate as a decimal (e.g., 0.2 for 20%). Set to 0 to disable fees.
    pub fee_rate: Decimal,
    /// Address where fee shares are minted to
    pub fee_recipient: Addr,
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
    UpdateDeployedAmount {
        amount: Uint128,
        direction: DeploymentDirection,
    },

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

    /// Accrues performance fees based on yield since last accrual.
    /// This is a permissionless operation - anyone can call it.
    AccrueFees {},

    /// Updates the fee configuration. Only whitelisted addresses can call this.
    /// Set fee_rate to 0 to disable fee accrual.
    UpdateFeeConfig {
        fee_rate: Option<Decimal>,
        fee_recipient: Option<String>,
    },
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

    /// Returns the current fee configuration.
    #[returns(FeeConfigResponse)]
    FeeConfig {},

    /// Returns information about pending fee accrual.
    #[returns(FeeAccrualInfoResponse)]
    FeeAccrualInfo {},
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

#[cw_serde]
pub struct FeeConfigResponse {
    pub fee_rate: Decimal,
    pub fee_recipient: Addr,
}

#[cw_serde]
pub struct FeeAccrualInfoResponse {
    pub high_water_mark_price: Decimal,
    pub current_share_price: Decimal,
    /// Pending yield amount (in base tokens) since last accrual
    pub pending_yield: Uint128,
    /// Pending fee amount (in base tokens) based on current yield
    pub pending_fee: Uint128,
}
