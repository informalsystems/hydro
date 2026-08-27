use std::collections::HashMap;

use cosmos_sdk_proto::cosmos::staking::v1beta1::{
    QueryValidatorRequest, QueryValidatorResponse, Validator,
};
use cosmwasm_std::{
    from_json, to_json_binary, Binary, ContractResult, GrpcQuery, SystemResult, Timestamp, Uint128,
    WasmQuery,
};
use interface::hydro::{CurrentRoundResponse, QueryMsg};
use prost::Message;

use crate::testing_mocks::{system_result_err_from, GrpcQueryFunc, WasmQueryFunc};

const STAKING_VALIDATOR_GRPC: &str = "/cosmos.staking.v1beta1.Query/Validator";

// A validator address whose bech32 encoding is valid and decodes to the "cosmosvaloper" HRP,
// so it passes the validation in validators::validate_and_deduplicate().
pub const VALIDATOR_1: &str = "cosmosvaloper157v7tczs40axfgejp2m43kwuzqe0wsy0rv8puv";

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

// Mocks the /cosmos.staking.v1beta1.Query/Validator gRPC query used by
// query_staking_validator() in validators.rs. `validators` maps a validator address to its
// (tokens, delegator_shares) on the staking module.
pub fn staking_validator_grpc_mock(
    validators: HashMap<String, (Uint128, Uint128)>,
) -> Box<GrpcQueryFunc> {
    Box::new(move |query: GrpcQuery| {
        if query.path != STAKING_VALIDATOR_GRPC {
            panic!("unexpected gRPC query path: {}", query.path);
        }

        let request = QueryValidatorRequest::decode(query.data.as_slice())
            .expect("failed to decode QueryValidatorRequest");

        match validators.get(&request.validator_addr) {
            None => system_result_err_from(format!(
                "no mock data for validator {}",
                request.validator_addr
            )),
            Some((tokens, shares)) => {
                let response = QueryValidatorResponse {
                    validator: Some(Validator {
                        operator_address: request.validator_addr.clone(),
                        tokens: tokens.to_string(),
                        delegator_shares: shares.to_string(),
                        ..Default::default()
                    }),
                };

                SystemResult::Ok(ContractResult::Ok(Binary::new(response.encode_to_vec())))
            }
        }
    })
}
