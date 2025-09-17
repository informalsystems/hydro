use std::fmt::Display;
use std::fmt::Formatter;

use cosmwasm_schema::cw_serde;
use cosmwasm_schema::QueryResponses;
use cosmwasm_std::Decimal;
use cosmwasm_std::Timestamp;

// When compiling for wasm32 platform, compiler doesn't recognize that this type is used in one of the queries.
#[allow(unused_imports)]
use crate::token_info_provider::ValidatorsInfoResponse;

// This module contains query and execute messages, as well as the types from the Hydro
// contract that are used by other contracts in our workspace. They are placed here so
// that we don't need to reference the Hydro contract directly from the consumer contracts.
// Also, if we modify some of these messages, it should be clear that the changes should
// be reflected in both Hydro and the consumer contracts.

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(CurrentRoundResponse)]
    CurrentRound {},

    // TODO: remove after LSM migration is done
    #[returns(ValidatorsInfoResponse)]
    ValidatorsInfo { round_id: u64 },
}

#[cw_serde]
pub struct CurrentRoundResponse {
    pub round_id: u64,
    pub round_end: Timestamp,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateTokenGroupsRatios { changes: Vec<TokenGroupRatioChange> },
}

#[cw_serde]
pub struct TokenGroupRatioChange {
    pub token_group_id: String,
    pub old_ratio: Decimal,
    pub new_ratio: Decimal,
}

impl Display for TokenGroupRatioChange {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "(token_group_id: {}, old_ratio: {}, new_ratio: {})",
            self.token_group_id, self.old_ratio, self.new_ratio
        )
    }
}
