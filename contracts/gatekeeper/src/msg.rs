use cosmwasm_schema::cw_serde;
use cosmwasm_std::Timestamp;
use interface::gatekeeper::ExecuteLockTokensMsg;

#[cw_serde]
pub struct InstantiateMsg {
    pub hydro_contract: Option<String>,
    pub admins: Vec<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    RegisterStage {
        // stage activation timestamp
        activate_at: Timestamp,
        // hex-encoded merkle root of a tree for the given stage that contains eligible addresses with amounts
        merkle_root: String,
        // whether the number of tokens that users already locked in previous stages should be reset from this stage.
        start_new_epoch: bool,
        // hrp is the bech32 parameter required for building external network address
        // from signature message during claim action. example "cosmos", "terra", "juno"
        hrp: Option<String>,
    },
    LockTokens(ExecuteLockTokensMsg),
    AddAdmin {
        admin: String,
    },
    RemoveAdmin {
        admin: String,
    },
}
