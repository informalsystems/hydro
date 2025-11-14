use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;

#[cw_serde]
pub struct InstantiateMsg {
    /// Maximum number of tokens that can be deposited into the vault.
    pub deposit_cap: Uint128,
    /// Addresses that are allowed to execute permissioned actions on the smart contract.
    pub whitelist: Vec<String>,
    /// Initial sub-vault smart contracts to be managed by the Control Center.
    pub subvaults: Vec<String>,
}
