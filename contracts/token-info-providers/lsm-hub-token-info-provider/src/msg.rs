use cosmwasm_schema::cw_serde;

use crate::state::Config;

#[cw_serde]
pub struct InstantiateMsg {
    // If this field value is set to None, then we assume that the msg sender is the Hydro contract itself.
    pub hydro_contract_address: Option<String>,
    // List of addresses that can execute permissioned actions on this contract.
    pub admins: Vec<String>,
    pub max_validator_shares_participating: u64,
}

#[cw_serde]
pub enum ExecuteMsg {
    // Permissionless: anyone can submit a list of CosmosHub validator addresses to update.
    // The contract queries each validator from the local staking module via gRPC, computes
    // the power ratio, and pushes any changes to the Hydro contract.
    UpdateValidatorsRatios { validators: Vec<String> },
}

pub struct ExecuteContext {
    pub current_round_id: u64,
    pub config: Config,
}
