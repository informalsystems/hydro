use std::{marker::PhantomData, str::FromStr};

use cosmwasm_std::{
    from_json,
    testing::{
        mock_env, MockApi, MockQuerier as BaseMockQuerier, MockQuerierCustomHandlerResult,
        MockStorage,
    },
    to_json_binary, to_json_vec, BankMsg, Binary, Coin, ContractResult, CosmosMsg, Decimal,
    MessageInfo, MsgResponse, OwnedDeps, Querier, QuerierResult, Reply, SubMsgResponse,
    SubMsgResult, SystemResult, Timestamp, Uint128, WasmMsg, WasmQuery,
};
use interface::token_info_provider::DenomInfoResponse;
use neutron_sdk::{
    bindings::{
        msg::NeutronMsg,
        query::{NeutronQuery, QueryRegisteredQueryResponse, QueryRegisteredQueryResultResponse},
        types::{Height, InterchainQueryResult, RegisteredQuery, StorageValue},
    },
    interchain_queries::types::{QueryType, QUERY_TYPE_KV_VALUE},
    sudo::msg::SudoMsg,
};

use prost::Message;
use serde_json_wasm::to_string;

use crate::{
    contract::{
        execute, instantiate, query, reply, sudo, HostZone, ReplyPayload, DENOMINATOR,
        NATIVE_TOKEN_DENOM, STRIDE_STAKEIBC_STORE_KEY,
    },
    msg::{ExecuteMsg, HydroExecuteMsg, InstantiateMsg},
    query::{HydroCurrentRoundResponse, QueryMsg},
    state::{InterchainQueryInfo, INTERCHAIN_QUERY_INFO},
};

pub const COSMOS_HUB_CHAIN_ID: &str = "cosmoshub-4";
pub const HYDRO_ADDRESS: &str = "hydro";
pub const USER_ADDRESS_1: &str = "addr0000";
pub const USER_ADDRESS_2: &str = "addr0001";

pub type CustomQueryFunc = dyn Fn(&NeutronQuery) -> QuerierResult;

pub fn mock_dependencies() -> OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery> {
    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
        querier: MockQuerier::new(BaseMockQuerier::new(&[])),
        custom_query_type: PhantomData,
    }
}

pub fn get_default_instantiate_msg() -> InstantiateMsg {
    InstantiateMsg {
        st_token_denom: "ibc/B7864B03E1B9FD4F049243E92ABD691586F682137037A9F3FCA5222815620B3C"
            .to_string(),
        token_group_id: "stATOM".to_string(),
        stride_connection_id: "connection-0".to_string(),
        icq_update_period: 100,
        stride_host_zone_id: COSMOS_HUB_CHAIN_ID.to_string(),
    }
}

pub fn get_message_info(api: &MockApi, sender: &str, funds: &[Coin]) -> MessageInfo {
    MessageInfo {
        sender: api.addr_make(sender),
        funds: funds.to_vec(),
    }
}

fn build_reply_msg(payload_data: Vec<u8>, encoded_msg_response_data: Vec<u8>) -> Reply {
    Reply {
        id: 0,
        gas_used: 0,
        payload: Binary::new(payload_data),
        // `data` field is deprecated, but it must be set because otherwise the compiler gives an error
        #[allow(deprecated)]
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![],
            msg_responses: vec![MsgResponse {
                type_url: String::new(), // not used in the test
                value: Binary::from(encoded_msg_response_data),
            }],
            data: None,
        }),
    }
}

pub struct MockQuerier {
    base_querier: BaseMockQuerier<NeutronQuery>,
}

impl MockQuerier {
    pub fn new(base_querier: BaseMockQuerier<NeutronQuery>) -> Self {
        Self { base_querier }
    }

    pub fn with_custom_handler<CH>(mut self, handler: CH) -> Self
    where
        CH: Fn(&NeutronQuery) -> MockQuerierCustomHandlerResult + 'static,
    {
        self.base_querier = self.base_querier.with_custom_handler(Box::from(handler));

        self
    }

    pub fn update_wasm<WH>(&mut self, handler: WH)
    where
        WH: Fn(&WasmQuery) -> QuerierResult + 'static,
    {
        self.base_querier.update_wasm(handler);
    }
}

impl Querier for MockQuerier {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        self.base_querier.raw_query(bin_request)
    }
}

pub fn custom_interchain_query_mock(
    host_zone_query_id: u64,
    query_result: StorageValue,
) -> Box<CustomQueryFunc> {
    Box::new(move |query: &NeutronQuery| match *query {
        NeutronQuery::RegisteredInterchainQuery { query_id } => {
            if query_id != host_zone_query_id {
                panic!("no mock data for interchain query with id: {}", query_id);
            }

            system_result_ok_from(
                to_string(&QueryRegisteredQueryResponse {
                    registered_query: build_registered_kv_query(query_id, QueryType::KV),
                })
                .unwrap()
                .into_bytes(),
            )
        }
        NeutronQuery::InterchainQueryResult { query_id } => {
            if query_id != host_zone_query_id {
                panic!("no mock data for interchain query with id: {}", query_id);
            }

            let registered_query_result_response = QueryRegisteredQueryResultResponse {
                result: InterchainQueryResult {
                    revision: 0,
                    height: 0,
                    kv_results: vec![query_result.clone()],
                },
            };

            system_result_ok_from(
                to_string(&registered_query_result_response)
                    .unwrap()
                    .into_bytes(),
            )
        }
        _ => panic!("unexpected custom query type"),
    })
}

pub fn system_result_ok_from(input: Vec<u8>) -> QuerierResult {
    SystemResult::Ok(ContractResult::Ok(Binary::new(input)))
}

fn build_registered_kv_query(id: u64, query_type: QueryType) -> RegisteredQuery {
    RegisteredQuery {
        id,
        owner: "".to_string(),
        keys: vec![],
        query_type,
        transactions_filter: "".to_string(),
        connection_id: "".to_string(),
        update_period: 0,
        last_submitted_result_local_height: 0,
        last_submitted_result_remote_height: Height {
            revision_number: 0,
            revision_height: 0,
        },
        deposit: vec![],
        submit_timeout: 0,
        registered_at_height: 0,
    }
}

#[test]
fn register_and_remove_host_zone_query_test() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let hydro_info = get_message_info(&deps.api, HYDRO_ADDRESS, &[]);
    let user1_info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);
    let user2_info = get_message_info(&deps.api, USER_ADDRESS_2, &[]);

    let init_msg = get_default_instantiate_msg();

    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        hydro_info.clone(),
        init_msg.clone(),
    );
    assert!(res.is_ok());

    // Register host zone ICQ
    let msg = ExecuteMsg::RegisterHostZoneICQ {};
    let res = execute(deps.as_mut(), env.clone(), user1_info.clone(), msg.clone());
    assert!(res.is_ok());

    let submsgs = res.unwrap().messages;
    assert_eq!(submsgs.len(), 1);

    match submsgs[0].msg.clone() {
        CosmosMsg::Custom(neutron_msg) => match neutron_msg {
            NeutronMsg::RegisterInterchainQuery {
                query_type,
                keys,
                transactions_filter,
                connection_id,
                update_period,
            } => {
                assert_eq!(query_type, QUERY_TYPE_KV_VALUE.to_string());
                assert_eq!(keys.len(), 1);
                assert_eq!(keys[0].path, STRIDE_STAKEIBC_STORE_KEY.to_string());
                assert!(transactions_filter.is_empty());
                assert_eq!(connection_id, init_msg.stride_connection_id);
                assert_eq!(update_period, init_msg.icq_update_period);
            }
            _ => panic!("Unexpected Custom message type!"),
        },
        _ => panic!("Unexpected SubMsg type!"),
    }

    // Save Interchain Query info into the store by executing the reply() handler.
    let registered_query_id = 159;
    let deposit = Coin {
        denom: NATIVE_TOKEN_DENOM.to_string(),
        amount: Uint128::new(1000),
    };

    let mut encoded_data = Vec::new();
    prost::encoding::uint64::encode(1, &registered_query_id, &mut encoded_data);

    let reply_msg = build_reply_msg(
        to_json_vec(&ReplyPayload::RegisterHostZoneICQ {
            creator: user1_info.sender.to_string(),
            funds: vec![deposit.clone()],
        })
        .unwrap(),
        encoded_data,
    );

    let res = reply(deps.as_mut(), env.clone(), reply_msg.clone());
    assert!(res.is_ok());

    // Try to register another Interchain query and verify the error returned
    let msg = ExecuteMsg::RegisterHostZoneICQ {};
    let res = execute(deps.as_mut(), env.clone(), user1_info.clone(), msg.clone());
    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("host zone interchain query is already registered"));

    // Try to remove Interchain query created by another user and verify the error returned
    let msg = ExecuteMsg::RemoveHostZoneICQ {};
    let res = execute(deps.as_mut(), env.clone(), user2_info.clone(), msg.clone());
    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("unauthorized"));

    // Remove Interchain query created by the user and verify that the deposit is refunded
    let msg = ExecuteMsg::RemoveHostZoneICQ {};
    let res = execute(deps.as_mut(), env.clone(), user1_info.clone(), msg.clone());
    assert!(res.is_ok());

    let submsgs = res.unwrap().messages;
    assert_eq!(submsgs.len(), 2);

    match submsgs[0].msg.clone() {
        CosmosMsg::Custom(neutron_msg) => match neutron_msg {
            NeutronMsg::RemoveInterchainQuery { query_id } => {
                assert_eq!(query_id, registered_query_id);
            }
            _ => panic!("Unexpected Custom message type!"),
        },
        _ => panic!("Unexpected SubMsg type!"),
    }

    match submsgs[1].msg.clone() {
        CosmosMsg::Bank(bank_msg) => match bank_msg {
            BankMsg::Send { to_address, amount } => {
                assert_eq!(to_address, user1_info.sender.to_string());
                assert_eq!(amount.len(), 1);
                assert_eq!(amount[0].denom, deposit.denom);
                assert_eq!(amount[0].amount, deposit.amount);
            }
            _ => panic!("Unexpected Bank message type!"),
        },
        _ => panic!("Unexpected SubMsg type!"),
    }

    // Remove Interchain Query info from the store on reply() callback handler.
    let reply_msg = build_reply_msg(
        to_json_vec(&ReplyPayload::RemoveHostZoneICQ {
            query_id: registered_query_id,
        })
        .unwrap(),
        Vec::new(),
    );

    let res = reply(deps.as_mut(), env.clone(), reply_msg.clone());
    assert!(res.is_ok());

    // Verify that a new host zone ICQ can be created, this time by a different user
    let msg = ExecuteMsg::RegisterHostZoneICQ {};
    let res = execute(deps.as_mut(), env.clone(), user2_info.clone(), msg.clone());
    assert!(res.is_ok());

    let submsgs = res.unwrap().messages;
    assert_eq!(submsgs.len(), 1);

    match submsgs[0].msg.clone() {
        CosmosMsg::Custom(neutron_msg) => match neutron_msg {
            NeutronMsg::RegisterInterchainQuery {
                query_type,
                keys,
                transactions_filter,
                connection_id,
                update_period,
            } => {
                assert_eq!(query_type, QUERY_TYPE_KV_VALUE.to_string());
                assert_eq!(keys.len(), 1);
                assert_eq!(keys[0].path, STRIDE_STAKEIBC_STORE_KEY.to_string());
                assert!(transactions_filter.is_empty());
                assert_eq!(connection_id, init_msg.stride_connection_id);
                assert_eq!(update_period, init_msg.icq_update_period);
            }
            _ => panic!("Unexpected Custom message type!"),
        },
        _ => panic!("Unexpected SubMsg type!"),
    }
}

#[test]
fn token_ratio_update_test() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let hydro_info = get_message_info(&deps.api, HYDRO_ADDRESS, &[]);
    let user1_info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);

    let hydro_sender = hydro_info.sender.clone();

    let init_msg = get_default_instantiate_msg();

    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        hydro_info.clone(),
        init_msg.clone(),
    );
    assert!(res.is_ok());

    let current_round_id = 7;
    let registered_query_id = 159;
    let deposit = Coin {
        denom: NATIVE_TOKEN_DENOM.to_string(),
        amount: Uint128::new(1000),
    };
    let token_ratio = Decimal::from_str("1.601").unwrap();

    // Mock this information as if it was previously created through another transaction
    INTERCHAIN_QUERY_INFO
        .save(
            &mut deps.storage,
            &InterchainQueryInfo {
                creator: user1_info.sender.to_string(),
                query_id: registered_query_id,
                deposit_paid: vec![deposit.clone()],
            },
        )
        .unwrap();

    // Query the ratio for the current round before submitting the Interchain Query result
    let query_msg = QueryMsg::DenomInfo {
        round_id: current_round_id,
    };
    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();

    assert_eq!(denom_info.ratio, Decimal::zero());

    deps.querier
        .update_wasm(move |query: &WasmQuery| match query {
            WasmQuery::Smart {
                contract_addr,
                msg: _,
            } => {
                if contract_addr != &hydro_sender.to_string() {
                    panic!("Unexpected contract address: {}", contract_addr);
                }

                let response = to_json_binary(&HydroCurrentRoundResponse {
                    round_id: current_round_id,
                    round_end: Timestamp::from_seconds(1),
                })
                .unwrap();

                SystemResult::Ok(ContractResult::Ok(response))
            }
            _ => {
                panic!("Unexpected Wasm query type: {:?}", query);
            }
        });

    let host_zone_result = HostZone {
        chain_id: COSMOS_HUB_CHAIN_ID.to_string(),
        redemption_rate: token_ratio
            .checked_mul(Decimal::from_ratio(DENOMINATOR, Uint128::one()))
            .unwrap()
            .to_string(),
    };

    deps.querier = deps
        .querier
        .with_custom_handler(custom_interchain_query_mock(
            registered_query_id,
            StorageValue {
                storage_prefix: STRIDE_STAKEIBC_STORE_KEY.to_string(),
                key: Binary::default(),
                value: Binary::from(host_zone_result.encode_to_vec()),
            },
        ));

    let submsgs = sudo(
        deps.as_mut(),
        env.clone(),
        SudoMsg::KVQueryResult {
            query_id: registered_query_id,
        },
    )
    .unwrap()
    .messages;

    // Since the token ratio was updated, verify that the Response contains SubMsg to update it in Hydro as well
    assert_eq!(submsgs.len(), 1);
    match submsgs[0].msg.clone() {
        CosmosMsg::Wasm(wasm_msg) => match wasm_msg {
            WasmMsg::Execute {
                contract_addr,
                msg,
                funds: _,
            } => {
                assert_eq!(contract_addr, hydro_info.sender.to_string());
                match from_json(msg).unwrap() {
                    HydroExecuteMsg::UpdateTokenGroupRatio {
                        token_group_id,
                        old_ratio,
                        new_ratio,
                    } => {
                        assert_eq!(token_group_id, init_msg.token_group_id);
                        assert_eq!(old_ratio, Decimal::zero());
                        assert_eq!(new_ratio, token_ratio);
                    }
                }
            }
            _ => panic!("Unexpected Wasm message type!"),
        },
        _ => panic!("Unexpected SubMsg type!"),
    }

    // Query the ratio for the previous round after submitting the Interchain Query result
    // and verify that it remained unchanged.
    let query_msg = QueryMsg::DenomInfo {
        round_id: current_round_id - 1,
    };
    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(denom_info.ratio, Decimal::zero());

    // Query the ratio for the current round after submitting the Interchain Query result
    let query_msg = QueryMsg::DenomInfo {
        round_id: current_round_id,
    };
    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(denom_info.ratio, token_ratio);

    // If the token ratio hasn't been updated, there should be no SubMsg to update it in Hydro
    let submsgs = sudo(
        deps.as_mut(),
        env.clone(),
        SudoMsg::KVQueryResult {
            query_id: registered_query_id,
        },
    )
    .unwrap()
    .messages;
    assert_eq!(submsgs.len(), 0);

    // Perform another Interchain query result submission and verify the results
    let updated_token_ratio = Decimal::from_str("1.602").unwrap();

    let host_zone_result = HostZone {
        chain_id: COSMOS_HUB_CHAIN_ID.to_string(),
        redemption_rate: updated_token_ratio
            .checked_mul(Decimal::from_ratio(DENOMINATOR, Uint128::one()))
            .unwrap()
            .to_string(),
    };

    deps.querier = deps
        .querier
        .with_custom_handler(custom_interchain_query_mock(
            registered_query_id,
            StorageValue {
                storage_prefix: STRIDE_STAKEIBC_STORE_KEY.to_string(),
                key: Binary::default(),
                value: Binary::from(host_zone_result.encode_to_vec()),
            },
        ));

    let submsgs = sudo(
        deps.as_mut(),
        env.clone(),
        SudoMsg::KVQueryResult {
            query_id: registered_query_id,
        },
    )
    .unwrap()
    .messages;

    // Since the token ratio was updated, verify that the Response contains SubMsg to update it in Hydro as well
    assert_eq!(submsgs.len(), 1);
    match submsgs[0].msg.clone() {
        CosmosMsg::Wasm(wasm_msg) => match wasm_msg {
            WasmMsg::Execute {
                contract_addr,
                msg,
                funds: _,
            } => {
                assert_eq!(contract_addr, hydro_info.sender.to_string());
                match from_json(msg).unwrap() {
                    HydroExecuteMsg::UpdateTokenGroupRatio {
                        token_group_id,
                        old_ratio,
                        new_ratio,
                    } => {
                        assert_eq!(token_group_id, init_msg.token_group_id);
                        assert_eq!(old_ratio, token_ratio);
                        assert_eq!(new_ratio, updated_token_ratio);
                    }
                }
            }
            _ => panic!("Unexpected Wasm message type!"),
        },
        _ => panic!("Unexpected SubMsg type!"),
    }

    // Verify that the denom info query returns the updated token ratio
    let query_msg = QueryMsg::DenomInfo {
        round_id: current_round_id,
    };
    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(denom_info.ratio, updated_token_ratio);
}
