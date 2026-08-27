use std::marker::PhantomData;

use cosmwasm_std::{
    from_json,
    testing::{MockApi, MockQuerier as BaseMockQuerier, MockStorage},
    Binary, ContractResult, Empty, GrpcQuery, OwnedDeps, Querier, QuerierResult, QueryRequest,
    SystemError, SystemResult, WasmQuery,
};

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

    pub fn update_grpc(&mut self, grpc_query_mock: Box<GrpcQueryFunc>) {
        self.grpc_query_mock = grpc_query_mock;
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

pub fn no_op_grpc_query_mock() -> Box<GrpcQueryFunc> {
    Box::new(|_query| system_result_ok_from(vec![]))
}

pub fn system_result_ok_from(input: Vec<u8>) -> QuerierResult {
    SystemResult::Ok(ContractResult::Ok(Binary::new(input)))
}

pub fn system_result_err_from(input: String) -> QuerierResult {
    SystemResult::Err(SystemError::UnsupportedRequest { kind: input })
}
