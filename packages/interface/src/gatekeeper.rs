use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Binary, Uint128};

// This struct is used by both Hydro and Gatekeeper smart contracts. It is placed here so that both
// contracts can use the same code without the need to reference each other. However, in order to keep
// this package as small as possible, any code that is not needed by both smart contracts will not be
// added here. For example, the SignatureInfo structure could also handle the logic for signature
// verification and address extraction, but since this requires some external crates to be referenced,
// and Hydro doesn't need this functions, we decided to keep that code in the Gatekeeper contract.
// This will also help reduce the size of the contract binaries, since it will not import crates that
// aren't needed for its work.
#[cw_serde]
pub struct ExecuteLockTokensMsg {
    // Address of the user that is trying to lock tokens
    pub user_address: String,
    // The amount user is trying to lock
    pub amount_to_lock: Uint128,
    // The maximum amount this user is allowed to lock. Used during proofs verification.
    pub maximum_amount: Uint128,
    // Proof is hex-encoded merkle proof.
    pub proof: Vec<String>,
    // Enables cross chain airdrops.
    // Target wallet proves identity by sending a signed claim message containing the recipient address.
    pub sig_info: Option<SignatureInfo>,
}

#[cw_serde]
pub struct SignatureInfo {
    pub claim_msg: Binary,
    pub signature: Binary,
}

#[cw_serde]
pub enum ExecuteMsg {
    LockTokens(ExecuteLockTokensMsg),
}
