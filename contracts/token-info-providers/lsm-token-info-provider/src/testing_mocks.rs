use std::{collections::HashMap, marker::PhantomData};

use cosmos_sdk_proto::cosmos::base::v1beta1::Coin as CosmosCoin;
use cosmwasm_std::{
    from_json,
    testing::{
        MockApi, MockQuerier as BaseMockQuerier, MockQuerierCustomHandlerResult, MockStorage,
    },
    Binary, Coin, ContractResult, GrpcQuery, OwnedDeps, Querier, QuerierResult, QueryRequest,
    SystemError, SystemResult, WasmQuery,
};
use neutron_sdk::{
    bindings::{
        query::{NeutronQuery, QueryRegisteredQueryResponse, QueryRegisteredQueryResultResponse},
        types::{Height, InterchainQueryResult, RegisteredQuery, StorageValue},
    },
    interchain_queries::types::QueryType,
    proto_types::neutron::interchainqueries::{Params, QueryParamsResponse},
};
use prost::Message;
use serde_json_wasm::to_string;

pub const INTERCHAINQUERIES_PARAMS_GRPC: &str = "/neutron.interchainqueries.Query/Params";

pub type GrpcQueryFunc = dyn Fn(GrpcQuery) -> QuerierResult;
pub type CustomQueryFunc = dyn Fn(&NeutronQuery) -> QuerierResult;
pub type WasmQueryFunc = Box<dyn Fn(&WasmQuery) -> QuerierResult>;

pub fn mock_dependencies(
    grpc_query_mock: Box<GrpcQueryFunc>,
) -> OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery> {
    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
        querier: MockQuerier::new(BaseMockQuerier::new(&[]), grpc_query_mock),
        custom_query_type: PhantomData,
    }
}

pub struct MockQuerier {
    base_querier: BaseMockQuerier<NeutronQuery>,
    grpc_query_mock: Box<GrpcQueryFunc>,
}

impl MockQuerier {
    pub fn new(
        base_querier: BaseMockQuerier<NeutronQuery>,
        grpc_query_mock: Box<GrpcQueryFunc>,
    ) -> Self {
        Self {
            base_querier,
            grpc_query_mock,
        }
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

pub fn min_query_deposit_grpc_query_mock(mock_min_deposit: Coin) -> Box<GrpcQueryFunc> {
    Box::new(move |query: GrpcQuery| {
        if query.path != INTERCHAINQUERIES_PARAMS_GRPC {
            panic!("unexpected gRPC query path");
        }

        system_result_ok_from(
            QueryParamsResponse {
                params: Some(Params {
                    query_submit_timeout: 0,
                    query_deposit: vec![CosmosCoin {
                        denom: mock_min_deposit.denom.clone(),
                        amount: mock_min_deposit.amount.to_string(),
                    }],
                    tx_query_removal_limit: 0,
                }),
            }
            .encode_to_vec(),
        )
    })
}

pub struct ICQMockData {
    pub query_type: QueryType,
    pub should_query_return_error: bool,
    pub should_query_result_return_error: bool,
    pub kv_results: Vec<StorageValue>,
}

pub fn custom_interchain_query_mock(mock_data: HashMap<u64, ICQMockData>) -> Box<CustomQueryFunc> {
    Box::new(move |query: &NeutronQuery| match *query {
        NeutronQuery::RegisteredInterchainQuery { query_id } => match mock_data.get(&query_id) {
            None => panic!("no mock data for interchain query with id: {}", query_id),
            Some(mock_data) => {
                if mock_data.should_query_return_error {
                    system_result_err_from("mock error".to_string())
                } else {
                    let registered_query_response = QueryRegisteredQueryResponse {
                        registered_query: build_registered_kv_query(query_id, mock_data.query_type),
                    };

                    system_result_ok_from(
                        to_string(&registered_query_response).unwrap().into_bytes(),
                    )
                }
            }
        },
        NeutronQuery::InterchainQueryResult { query_id } => match mock_data.get(&query_id) {
            None => panic!("no mock data for interchain query with id: {}", query_id),
            Some(mock_data) => {
                if mock_data.should_query_result_return_error {
                    system_result_err_from("mock error".to_string())
                } else {
                    let registered_query_result_response = QueryRegisteredQueryResultResponse {
                        result: InterchainQueryResult {
                            revision: 0,
                            height: 0,
                            kv_results: mock_data.kv_results.to_owned(),
                        },
                    };

                    system_result_ok_from(
                        to_string(&registered_query_result_response)
                            .unwrap()
                            .into_bytes(),
                    )
                }
            }
        },
        _ => panic!("unexpected custom query type"),
    })
}

pub fn system_result_ok_from(input: Vec<u8>) -> QuerierResult {
    SystemResult::Ok(ContractResult::Ok(Binary::new(input)))
}

pub fn system_result_err_from(input: String) -> QuerierResult {
    SystemResult::Err(SystemError::UnsupportedRequest { kind: input })
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
