use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Coin, Uint128};

use crate::state::{Config, Distribution};

#[derive(QueryResponses)]
#[cw_serde]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},
    #[returns(DistributionResponse)]
    Distribution { id: u64 },
    #[returns(PendingClaimsResponse)]
    PendingClaims { user: String },
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct DistributionResponse {
    pub distribution: Distribution,
}

#[cw_serde]
pub struct PendingClaimInfo {
    pub distribution_id: u64,
    pub weight: Uint128,
    pub estimated_funds: Vec<Coin>,
}

#[cw_serde]
pub struct PendingClaimsResponse {
    pub claims: Vec<PendingClaimInfo>,
}
