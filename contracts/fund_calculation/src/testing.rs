use crate::msg::InstantiateMsg;
use cosmwasm_std::Coin;
use cosmwasm_std::{testing::MockApi, MessageInfo};

pub fn get_instantiate_msg(hydro_contract: String) -> InstantiateMsg {
    InstantiateMsg { hydro_contract }
}

pub fn get_message_info(mock_api: &MockApi, sender: &str, funds: &[Coin]) -> MessageInfo {
    MessageInfo {
        sender: mock_api.addr_make(sender),
        funds: funds.to_vec(),
    }
}

pub fn get_address_as_str(mock_api: &MockApi, addr: &str) -> String {
    mock_api.addr_make(addr).to_string()
}
