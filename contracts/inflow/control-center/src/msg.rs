use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;
use interface::inflow_control_center::FeeConfigInit;

#[cw_serde]
pub struct InstantiateMsg {
    /// Maximum number of tokens that can be deposited into the vault.
    pub deposit_cap: Uint128,
    /// Addresses that are allowed to execute permissioned actions on the smart contract.
    pub whitelist: Vec<String>,
    /// Initial sub-vault smart contracts to be managed by the Control Center.
    pub subvaults: Vec<String>,
    /// Optional fee configuration. If None, fees are disabled by default.
    pub fee_config: Option<FeeConfigInit>,
}

#[cw_serde]
pub struct MigrateMsg {
    /// Fee configuration to initialize during migration.
    /// If None and FEE_CONFIG doesn't exist, fees are disabled by default.
    pub fee_config: Option<FeeConfigInit>,
}
