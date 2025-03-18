use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Uint128};

use crate::state::Config;

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},
    #[returns(RootHashResponse)]
    GetRootHash { timestamp: u64 },
    #[returns(AdminsResponse)]
    GetAdmins {},
    #[returns(CanLockResponse)]
    CanLock {
        address: String,
        amount: Uint128,
        merkle_proof: Vec<String>,
        root_id: u64,
    },
    #[returns(UserLockInfoResponse)]
    UserLockInfo { address: String, root_id: u64 },
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct RootHashResponse {
    pub timestamp: u64,
    pub root_hash: String,
}

#[cw_serde]
pub struct AdminsResponse {
    pub admins: Vec<String>,
}

#[cw_serde]
pub struct CanLockResponse {
    pub can_lock: bool,
    pub available_amount: Option<Uint128>,
}

#[cw_serde]
pub struct UserLockInfoResponse {
    pub locked_amount: Uint128,
    pub eligible_amount: Option<Uint128>,
}
