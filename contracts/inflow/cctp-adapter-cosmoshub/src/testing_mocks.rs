use cosmwasm_std::{
    testing::{mock_env, MockApi, MockQuerier, MockStorage},
    Addr, Empty, MessageInfo, OwnedDeps,
};

use crate::contract::instantiate;
use crate::msg::{InitialChainConfig, InitialDepositor, InitialExecutor, InstantiateMsg};
use crate::state::{BridgingConfig, ChainConfig};

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
    }
}

pub fn create_test_chain_config(chain_id: &str) -> ChainConfig {
    ChainConfig {
        chain_id: chain_id.to_string(),
        bridging_config: BridgingConfig {
            // Valid noble bech32 addresses (from noble.rs tests)
            noble_receiver: "noble15xt7kx5mles58vkkfxvf0lq78sw04jajvfgd4d".to_string(),
            noble_fee_recipient: "noble1dyw0geqa2cy0ppdjcxfpzusjpwmq85r5a35hqe".to_string(),
            destination_domain: 1,
            evm_destination_caller: "0x1234567890123456789012345678901234567890".to_string(),
        },
    }
}

/// Setup contract with depositors and executors
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
        denom: "ibc/usdc".to_string(),
        noble_transfer_channel_id: "channel-0".to_string(),
        ibc_default_timeout_seconds: 600,
        initial_depositors: vec![InitialDepositor {
            address: test_data.depositor.to_string(),
            capabilities: None, // Default capabilities (can_withdraw: true)
        }],
        initial_chains: vec![],
        initial_executors: vec![InitialExecutor {
            address: test_data.executor.to_string(),
        }],
    };

    instantiate(deps.as_mut(), env, info, msg).unwrap();
    (deps, test_data)
}

/// Setup contract with chain and destination addresses
pub fn setup_contract_with_chain() -> (
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

    let chain_config = create_test_chain_config("ethereum");

    let msg = InstantiateMsg {
        admins: vec![test_data.admin.to_string()],
        denom: "ibc/usdc".to_string(),
        noble_transfer_channel_id: "channel-0".to_string(),
        ibc_default_timeout_seconds: 600,
        initial_depositors: vec![InitialDepositor {
            address: test_data.depositor.to_string(),
            capabilities: None,
        }],
        initial_chains: vec![InitialChainConfig {
            chain_config,
            initial_allowed_destination_addresses: vec![
                "0xabcd1234abcd1234abcd1234abcd1234abcd1234".to_string(),
            ],
        }],
        initial_executors: vec![InitialExecutor {
            address: test_data.executor.to_string(),
        }],
    };

    instantiate(deps.as_mut(), env, info, msg).unwrap();
    (deps, test_data)
}
