use std::{collections::HashMap, marker::PhantomData};

use cosmwasm_std::{
    from_json,
    testing::{MockApi, MockQuerier as BaseMockQuerier, MockStorage},
    to_json_binary, Binary, GrpcQuery, OwnedDeps, Querier, QuerierResult, QueryRequest,
    SystemError, SystemResult, WasmQuery,
};
use cosmwasm_std::{ContractResult, Empty};
use ibc_proto::ibc::apps::transfer::v1::{
    DenomTrace, QueryDenomTraceRequest, QueryDenomTraceResponse,
};
use interface::{
    lsm::ValidatorInfo,
    token_info_provider::{DenomInfoResponse, TokenInfoProviderQueryMsg, ValidatorsInfoResponse},
};
use prost::Message;

use crate::lsm_integration::DENOM_TRACE_GRPC;

pub type GrpcQueryFunc = dyn Fn(GrpcQuery) -> QuerierResult;
pub type WasmQueryFunc = Box<dyn Fn(&WasmQuery) -> QuerierResult>;

pub fn mock_dependencies(
    grpc_query_mock: Box<GrpcQueryFunc>,
) -> OwnedDeps<MockStorage, MockApi, MockQuerier, Empty> {
    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
        querier: MockQuerier::new(BaseMockQuerier::new(&[]), grpc_query_mock),
        custom_query_type: PhantomData,
    }
}

pub struct MockQuerier {
    base_querier: BaseMockQuerier,
    grpc_query_mock: Box<GrpcQueryFunc>,
}

impl MockQuerier {
    pub fn new(base_querier: BaseMockQuerier, grpc_query_mock: Box<GrpcQueryFunc>) -> Self {
        Self {
            base_querier,
            grpc_query_mock,
        }
    }

    pub fn update_wasm<WH>(&mut self, handler: WH)
    where
        WH: Fn(&WasmQuery) -> QuerierResult + 'static,
    {
        self.base_querier.update_wasm(handler);
    }
}

// Overrides raw_query() to support gRPC queries. If the QueryRequest is
// not Grpc variant, then it forwards the call to the underlying querier.
impl Querier for MockQuerier {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        let request: QueryRequest = match from_json(bin_request) {
            Ok(v) => v,
            Err(e) => {
                return SystemResult::Err(SystemError::InvalidRequest {
                    error: format!("Parsing query request: {e}"),
                    request: bin_request.into(),
                })
            }
        };

        match request {
            QueryRequest::Grpc(grpc_query) => (self.grpc_query_mock)(grpc_query),
            _ => self.base_querier.raw_query(bin_request),
        }
    }
}

pub struct MockWasmQuerier {
    inner_handler: WasmQueryFunc,
}

impl MockWasmQuerier {
    pub fn new(inner_handler: WasmQueryFunc) -> Self {
        Self { inner_handler }
    }

    pub fn handler(&self, query: &WasmQuery) -> QuerierResult {
        (self.inner_handler)(query)
    }
}

pub fn no_op_grpc_query_mock() -> Box<GrpcQueryFunc> {
    Box::new(|_query| system_result_ok_from(vec![]))
}

pub fn denom_trace_grpc_query_mock(
    denom_trace_path: String,
    in_out_denom_map: HashMap<String, String>,
) -> Box<GrpcQueryFunc> {
    Box::new(move |query: GrpcQuery| {
        if query.path != DENOM_TRACE_GRPC {
            panic!("unexpected gRPC query path");
        }

        let request = QueryDenomTraceRequest::decode(query.data.as_slice()).unwrap();
        let resolved_denom = match in_out_denom_map.get(request.hash.as_str()) {
            Some(denom) => denom.clone(),
            _ => {
                let expected_tokens: Vec<&String> = in_out_denom_map.keys().collect();
                let err_msg = format!(
                    "unexpected input token: '{}'. Expected one of: {:?}",
                    request.hash, expected_tokens
                );

                return system_result_err_from(err_msg);
            }
        };

        system_result_ok_from(
            QueryDenomTraceResponse {
                denom_trace: Some(DenomTrace {
                    path: denom_trace_path.clone(),
                    base_denom: resolved_denom,
                }),
            }
            .encode_to_vec(),
        )
    })
}

pub fn grpc_query_diff_paths_mock(
    in_out_denom_map: HashMap<String, HashMap<String, String>>,
) -> Box<GrpcQueryFunc> {
    Box::new(move |query: GrpcQuery| {
        match query.path.as_str() {
            DENOM_TRACE_GRPC => {
                let request = QueryDenomTraceRequest::decode(query.data.as_slice()).unwrap();
                // Find the trace path and denom for the given hash
                let (trace_path, denom) = in_out_denom_map
                    .iter()
                    .find_map(|(trace_path, map)| {
                        map.get(request.hash.as_str())
                            .map(|resolved| (trace_path.clone(), resolved.clone()))
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "unexpected input token hash '{}' for path {}",
                            request.hash, DENOM_TRACE_GRPC
                        )
                    });

                system_result_ok_from(
                    QueryDenomTraceResponse {
                        denom_trace: Some(DenomTrace {
                            path: trace_path,
                            base_denom: denom,
                        }),
                    }
                    .encode_to_vec(),
                )
            }
            _ => system_result_ok_from(vec![]), // no-op
        }
    })
}

pub type RoundsValidators = HashMap<u64, HashMap<String, ValidatorInfo>>;

pub fn token_info_providers_mock(
    derivative_providers: HashMap<String, HashMap<u64, DenomInfoResponse>>,
    lsm_provider: Option<(String, RoundsValidators)>,
) -> WasmQueryFunc {
    Box::new(move |query| match query {
        WasmQuery::Smart { contract_addr, msg } => {
            let token_info_provider_request: TokenInfoProviderQueryMsg = match from_json(msg) {
                Err(_) => {
                    return system_result_err_from(
                        "not a token info provider query type".to_string(),
                    )
                }
                Ok(msg) => msg,
            };

            let response = match token_info_provider_request {
                TokenInfoProviderQueryMsg::DenomInfo { round_id } => {
                    let contract_map = match derivative_providers.get(contract_addr.as_str()) {
                        Some(map) => map,
                        None => {
                            return SystemResult::Err(SystemError::NoSuchContract {
                                addr: contract_addr.to_string(),
                            })
                        }
                    };
                    let response = match contract_map.get(&round_id) {
                        Some(denom_info) => denom_info.clone(),
                        None => {
                            return system_result_err_from(format!(
                                "No mock DenomInfo for contract {contract_addr} round_id {round_id}"
                            ))
                        }
                    };
                    to_json_binary(&response)
                }
                TokenInfoProviderQueryMsg::ValidatorsInfo { round_id } => {
                    let (lsm_addr, validators_map) = match lsm_provider.clone() {
                        Some(data) => data,
                        None => {
                            return SystemResult::Err(SystemError::NoSuchContract {
                                addr: contract_addr.to_string(),
                            })
                        }
                    };
                    if lsm_addr != *contract_addr {
                        return SystemResult::Err(SystemError::NoSuchContract {
                            addr: contract_addr.to_string(),
                        });
                    }
                    let validators = match validators_map.get(&round_id) {
                        Some(validators) => validators.clone(),
                        None => {
                            return system_result_err_from(format!(
                                "No mock ValidatorsInfo for contract {contract_addr} round_id {round_id}"
                            ))
                        }
                    };
                    to_json_binary(&ValidatorsInfoResponse {
                        round_id,
                        validators,
                    })
                }
                _ => {
                    return system_result_err_from("Unexpected mock contract call".to_owned());
                }
            };

            SystemResult::Ok(ContractResult::Ok(response.unwrap()))
        }
        _ => system_result_err_from("unsupported query type".to_string()),
    })
}

pub fn contract_info_mock(existing_contract_addr: String) -> WasmQueryFunc {
    Box::new(move |query| match query {
        WasmQuery::ContractInfo { contract_addr } => {
            if *contract_addr != existing_contract_addr.clone() {
                return SystemResult::Err(SystemError::NoSuchContract {
                    addr: contract_addr.to_string(),
                });
            }

            let binary = Binary::from(
                br#"{
                    "code_id": 1,
                    "creator": "creator",
                    "admin": null,
                    "pinned": false,
                    "ibc_port": null
                }"#,
            );
            SystemResult::Ok(ContractResult::Ok(binary))
        }
        _ => SystemResult::Err(SystemError::UnsupportedRequest {
            kind: "unsupported query type".to_string(),
        }),
    })
}

pub fn system_result_ok_from(input: Vec<u8>) -> QuerierResult {
    SystemResult::Ok(ContractResult::Ok(Binary::new(input)))
}

pub fn system_result_err_from(input: String) -> QuerierResult {
    SystemResult::Err(SystemError::UnsupportedRequest { kind: input })
}
