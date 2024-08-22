use std::{collections::HashMap, marker::PhantomData};

use cosmwasm_std::{
    from_json,
    testing::{MockApi, MockQuerier as BaseMockQuerier, MockStorage},
    Binary, ContractResult, Empty, GrpcQuery, OwnedDeps, Querier, QuerierResult, QueryRequest,
    SystemError, SystemResult,
};
use ibc_proto::ibc::apps::transfer::v1::{
    DenomTrace, QueryDenomTraceRequest, QueryDenomTraceResponse,
};
use prost::Message;

use crate::lsm_integration::DENOM_TRACE_GRPC;

pub type GrpcQueryFunc = dyn Fn(GrpcQuery) -> QuerierResult;

pub fn mock_dependencies(
    grpc_query_mock: Box<GrpcQueryFunc>,
) -> OwnedDeps<MockStorage, MockApi, MockQuerier, Empty> {
    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
        querier: MockQuerier::new(Box::new(BaseMockQuerier::default()), grpc_query_mock),
        custom_query_type: PhantomData,
    }
}

pub struct MockQuerier {
    base_querier: Box<dyn Querier>,
    grpc_query_mock: Box<GrpcQueryFunc>,
}

impl MockQuerier {
    pub fn new(base_querier: Box<dyn Querier>, grpc_query_mock: Box<GrpcQueryFunc>) -> Self {
        Self {
            base_querier,
            grpc_query_mock,
        }
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
    Box::new(|_query| grpc_query_result_from(vec![]))
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
            _ => panic!("unexpected input token"),
        };

        grpc_query_result_from(
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

pub fn grpc_query_result_from(input: Vec<u8>) -> QuerierResult {
    SystemResult::Ok(ContractResult::Ok(Binary::new(input)))
}
