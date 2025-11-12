use cosmwasm_std::{
    from_json, testing::MockApi, to_json_binary, Coin, ContractResult, MessageInfo, SystemResult,
    Timestamp, WasmQuery,
};
use interface::hydro::{CurrentRoundResponse, QueryMsg};

use crate::{
    msg::InstantiateMsg,
    testing_mocks::{system_result_err_from, WasmQueryFunc},
};

pub const VALIDATOR_1: &str = "cosmosvaloper157v7tczs40axfgejp2m43kwuzqe0wsy0rv8puv";
pub const VALIDATOR_2: &str = "cosmosvaloper140l6y2gp3gxvay6qtn70re7z2s0gn57zfd832j";
pub const VALIDATOR_3: &str = "cosmosvaloper14upntdx8lf0f49t987mj99zksxnluanvu6x4lu";

pub fn get_default_instantiate_msg(mock_api: &MockApi) -> InstantiateMsg {
    let user_address = get_address_as_str(mock_api, "addr0000");

    InstantiateMsg {
        hydro_contract_address: None,
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
        icq_managers: vec![user_address.clone()],
        admins: vec![user_address.clone()],
    }
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

pub fn hydro_current_round_mock(current_round: u64) -> WasmQueryFunc {
    Box::new(move |query| match query {
        WasmQuery::Smart {
            contract_addr: _,
            msg,
        } => {
            let response = match from_json(msg).unwrap() {
                QueryMsg::CurrentRound {} => to_json_binary(&CurrentRoundResponse {
                    round_id: current_round,
                    round_end: Timestamp::from_seconds(0),
                }),
            };

            SystemResult::Ok(ContractResult::Ok(response.unwrap()))
        }
        _ => system_result_err_from("unsupported query type".to_string()),
    })
}
