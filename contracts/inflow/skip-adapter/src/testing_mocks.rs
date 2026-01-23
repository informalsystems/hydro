use cosmwasm_std::{
    testing::{MockApi, MockQuerier, MockStorage},
    OwnedDeps, SystemResult,
};
use neutron_sdk::bindings::query::NeutronQuery;

/// Creates mock dependencies with custom Neutron query handler
pub fn mock_dependencies(
) -> OwnedDeps<MockStorage, MockApi, MockQuerier<NeutronQuery>, NeutronQuery> {
    let custom_querier: MockQuerier<NeutronQuery> =
        MockQuerier::new(&[]).with_custom_handler(|_| {
            SystemResult::Err(cosmwasm_std::SystemError::UnsupportedRequest {
                kind: "unsupported neutron query".to_string(),
            })
        });

    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default().with_prefix("neutron"),
        querier: custom_querier,
        custom_query_type: std::marker::PhantomData,
    }
}
