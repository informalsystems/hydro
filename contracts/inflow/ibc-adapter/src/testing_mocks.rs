use cosmwasm_std::{
    testing::{MockApi, MockQuerier, MockStorage},
    to_json_binary, Coin, OwnedDeps, SystemResult, Uint128,
};
use neutron_sdk::bindings::msg::IbcFee;
use neutron_sdk::bindings::query::NeutronQuery;
use neutron_sdk::query::min_ibc_fee::MinIbcFeeResponse;

use crate::ibc::LOCAL_DENOM;

/// Creates mock dependencies with custom Neutron query handler
pub fn mock_dependencies(
) -> OwnedDeps<MockStorage, MockApi, MockQuerier<NeutronQuery>, NeutronQuery> {
    let custom_querier: MockQuerier<NeutronQuery> =
        MockQuerier::new(&[]).with_custom_handler(|query| match query {
            NeutronQuery::MinIbcFee {} => {
                let response = MinIbcFeeResponse {
                    min_fee: IbcFee {
                        recv_fee: vec![],
                        ack_fee: vec![Coin {
                            denom: LOCAL_DENOM.to_string(),
                            amount: Uint128::new(1000),
                        }],
                        timeout_fee: vec![Coin {
                            denom: LOCAL_DENOM.to_string(),
                            amount: Uint128::new(1000),
                        }],
                    },
                };
                SystemResult::Ok(cosmwasm_std::ContractResult::Ok(
                    to_json_binary(&response).unwrap(),
                ))
            }
            _ => SystemResult::Err(cosmwasm_std::SystemError::UnsupportedRequest {
                kind: "unsupported neutron query".to_string(),
            }),
        });

    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
        querier: custom_querier,
        custom_query_type: std::marker::PhantomData,
    }
}
