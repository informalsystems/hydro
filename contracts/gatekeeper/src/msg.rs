use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;

#[cw_serde]
pub struct InstantiateMsg {
    pub hydro_contract: String,
    pub admins: Vec<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    AddRootHash {
        timestamp: u64,
        root_hash: String,
    },
    AddAdmin {
        address: String,
    },
    RemoveAdmin {
        address: String,
    },
    Lock {
        amount: Uint128,
        merkle_proof: Vec<String>,
        root_id: u64,
    },
}

#[cw_serde]
pub struct RootHashQuery {
    pub timestamp: u64,
}
