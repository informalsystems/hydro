use crate::msg::InstantiateMsg;
use crate::state::{GlobalConfig, VenueType};
use cosmwasm_std::Coin;
use cosmwasm_std::{testing::MockApi, MessageInfo};

// pub fn get_instantiate_msg(hydro_contract: String, tribute_contract: String) -> InstantiateMsg {
//     InstantiateMsg {
//         hydro_contract,
//         tribute_contract,
//         global_config: GlobalConfig {
//             bootstrap_limit: 10000,
//             total_allocated: 100000,
//             venue_type_to_existing_tvl_factor: vec![(VenueType, 0.3), ("Lending".to_string(), 0.5)],
//         },
//     }
// }

pub fn get_message_info(mock_api: &MockApi, sender: &str, funds: &[Coin]) -> MessageInfo {
    MessageInfo {
        sender: mock_api.addr_make(sender),
        funds: funds.to_vec(),
    }
}

pub fn get_address_as_str(mock_api: &MockApi, addr: &str) -> String {
    mock_api.addr_make(addr).to_string()
}
