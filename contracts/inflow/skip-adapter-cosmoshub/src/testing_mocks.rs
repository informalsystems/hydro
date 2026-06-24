use cosmwasm_std::{
    testing::{MockApi, MockQuerier, MockStorage},
    Empty, OwnedDeps,
};

/// Creates mock dependencies for Cosmos Hub (standard CosmWasm, no Neutron queries)
pub fn mock_dependencies() -> OwnedDeps<MockStorage, MockApi, MockQuerier, Empty> {
    let mut deps = cosmwasm_std::testing::mock_dependencies();
    deps.api = MockApi::default().with_prefix("cosmos");
    deps
}
