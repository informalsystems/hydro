use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;

#[cw_serde]
pub struct ConfigV3_6_1 {
    /// Token denom that users can deposit into the vault.
    pub deposit_denom: String,
    /// Denom of the vault shares token that is issued to users when they deposit tokens into the vault.
    pub vault_shares_denom: String,
    /// Maximum number of pending withdrawal requests allowed per user.
    pub max_withdrawals_per_user: u64,
    /// Maximum number of tokens that can be deposited into the vault.
    pub deposit_cap: Uint128,
}
