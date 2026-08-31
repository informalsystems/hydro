use cosmwasm_std::{
    testing::{mock_env, MockApi, MockQuerier, MockStorage},
    Addr, Empty, MessageInfo, OwnedDeps,
};

use crate::contract::instantiate;
use crate::msg::{InitialDepositor, InitialExecutor, InstantiateMsg};

pub const TEST_DENOM: &str = "ibc/wbtc";
pub const TEST_DESTINATION_ADDRESS: &str = "0xabcd1234abcd1234abcd1234abcd1234abcd1234";

/// Creates mock dependencies for Cosmos Hub (standard CosmWasm, no Neutron queries)
pub fn mock_dependencies() -> OwnedDeps<MockStorage, MockApi, MockQuerier, Empty> {
    let mut deps = cosmwasm_std::testing::mock_dependencies();
    deps.api = MockApi::default().with_prefix("cosmos");
    deps
}

/// Test data structure
pub struct TestSetupData {
    pub admin: Addr,
    pub admin2: Addr,
    pub depositor: Addr,
    pub depositor2: Addr,
    pub executor: Addr,
    pub non_admin: Addr,
    pub non_depositor: Addr,
    pub skip_swap_entry_point_contract: Addr,
    pub eureka_fee_receiver: Addr,
}

pub fn default_test_setup(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, Empty>,
) -> TestSetupData {
    TestSetupData {
        admin: deps.api.addr_make("admin1"),
        admin2: deps.api.addr_make("admin2"),
        depositor: deps.api.addr_make("depositor1"),
        depositor2: deps.api.addr_make("depositor2"),
        executor: deps.api.addr_make("executor1"),
        non_admin: deps.api.addr_make("non_admin"),
        non_depositor: deps.api.addr_make("non_depositor"),
        skip_swap_entry_point_contract: deps.api.addr_make("skip_swap_entry_point_contract"),
        eureka_fee_receiver: deps.api.addr_make("eureka_fee_receiver"),
    }
}

/// Setup contract with depositors and executors, but no allowed denoms/destinations yet
pub fn setup_contract_with_defaults() -> (
    OwnedDeps<MockStorage, MockApi, MockQuerier, Empty>,
    TestSetupData,
) {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = default_test_setup(&mut deps);

    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };

    let msg = InstantiateMsg {
        admins: vec![test_data.admin.to_string()],
        skip_swap_entry_point_contract: test_data.skip_swap_entry_point_contract.to_string(),
        source_channel: "08-wasm-1369".to_string(),
        eureka_fee_receiver: test_data.eureka_fee_receiver.to_string(),
        ibc_transfer_timeout_seconds: 3600,
        eureka_fee_timeout_seconds: 600,
        initial_depositors: vec![InitialDepositor {
            address: test_data.depositor.to_string(),
            capabilities: None, // Default capabilities (can_withdraw: true)
        }],
        initial_denoms: vec![],
        initial_allowed_destination_addresses: vec![],
        initial_executors: vec![InitialExecutor {
            address: test_data.executor.to_string(),
        }],
    };

    instantiate(deps.as_mut(), env, info, msg).unwrap();
    (deps, test_data)
}

/// Setup contract with an allowed denom and destination address already registered
pub fn setup_contract_with_denom() -> (
    OwnedDeps<MockStorage, MockApi, MockQuerier, Empty>,
    TestSetupData,
) {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = default_test_setup(&mut deps);

    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };

    let msg = InstantiateMsg {
        admins: vec![test_data.admin.to_string()],
        skip_swap_entry_point_contract: test_data.skip_swap_entry_point_contract.to_string(),
        source_channel: "08-wasm-1369".to_string(),
        eureka_fee_receiver: test_data.eureka_fee_receiver.to_string(),
        ibc_transfer_timeout_seconds: 3600,
        eureka_fee_timeout_seconds: 600,
        initial_depositors: vec![InitialDepositor {
            address: test_data.depositor.to_string(),
            capabilities: None,
        }],
        initial_denoms: vec![TEST_DENOM.to_string()],
        initial_allowed_destination_addresses: vec![TEST_DESTINATION_ADDRESS.to_string()],
        initial_executors: vec![InitialExecutor {
            address: test_data.executor.to_string(),
        }],
    };

    instantiate(deps.as_mut(), env, info, msg).unwrap();
    (deps, test_data)
}
