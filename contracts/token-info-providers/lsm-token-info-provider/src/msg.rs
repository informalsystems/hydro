use cosmwasm_std::Uint128;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::Config;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    // For the migration purpose, we will instantiate the contract manually, so we must provide
    // the address of the Hydro contract, which will be done through this field. If the field value
    // is set to None, then we assume that the msg sender is the Hydro contract itself.
    pub hydro_contract_address: Option<String>,
    // List of addresses that can manage the ICQ managers list.
    pub admins: Vec<String>,
    // Anyone can permissionlessly create ICQs, but addresses in this list can attempt
    // to create ICQs without paying, which will then implicitly be paid for by the contract;
    // and they can also withdraw funds in the *native token denom* from the contract;
    // they can however not withdraw user funds that were locked for voting.
    pub icq_managers: Vec<String>,
    // LSM specific parameters.
    pub max_validator_shares_participating: u64,
    pub hub_connection_id: String,
    pub hub_transfer_channel_id: String,
    pub icq_update_period: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    #[serde(rename = "create_icqs_for_validators")]
    CreateICQsForValidators {
        validators: Vec<String>,
    },

    AddICQManager {
        address: String,
    },

    RemoveICQManager {
        address: String,
    },

    WithdrawICQFunds {
        amount: Uint128,
    },

    /// Temporary for the purpose of LSM migration
    CopyRoundValidatorsData {
        round_id: u64,
    },
}

pub struct ExecuteContext {
    pub current_round_id: u64,
    pub config: Config,
}
