use cosmwasm_std::{
    testing::{mock_env, MockApi, MockQuerier, MockStorage},
    Addr, Coin, MessageInfo, OwnedDeps, SystemResult, Uint128,
};
use neutron_sdk::bindings::msg::IbcFee;
use neutron_sdk::bindings::query::NeutronQuery;
use neutron_sdk::query::min_ibc_fee::MinIbcFeeResponse;

use crate::contract::instantiate;
use crate::msg::{InitialChainConfig, InitialDepositor, InitialExecutor, InstantiateMsg};
use crate::state::{ChainConfig, TokenConfig};

pub fn mock_dependencies(
) -> OwnedDeps<MockStorage, MockApi, MockQuerier<NeutronQuery>, NeutronQuery> {
    let custom_querier: MockQuerier<NeutronQuery> =
        MockQuerier::new(&[]).with_custom_handler(|query| match query {
            NeutronQuery::MinIbcFee {} => {
                let min_fee = IbcFee {
                    recv_fee: vec![],
                    ack_fee: vec![Coin {
                        denom: "untrn".to_string(),
                        amount: Uint128::new(1000),
                    }],
                    timeout_fee: vec![Coin {
                        denom: "untrn".to_string(),
                        amount: Uint128::new(1000),
                    }],
                };
                SystemResult::Ok(
                    cosmwasm_std::to_json_binary(&MinIbcFeeResponse { min_fee }).into(),
                )
            }
            _ => SystemResult::Err(cosmwasm_std::SystemError::UnsupportedRequest {
                kind: "unsupported neutron query".to_string(),
            }),
        });

    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default().with_prefix("neutron"),
        querier: custom_querier,
        custom_query_type: std::marker::PhantomData,
    }
}

pub struct TestSetupData {
    pub admin: Addr,
    pub depositor: Addr,
    pub executor: Addr,
    pub non_admin: Addr,
    #[allow(dead_code)]
    pub non_depositor: Addr,
}

pub fn default_test_setup(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier<NeutronQuery>, NeutronQuery>,
) -> TestSetupData {
    TestSetupData {
        admin: deps.api.addr_make("admin1"),
        depositor: deps.api.addr_make("depositor1"),
        executor: deps.api.addr_make("executor1"),
        non_admin: deps.api.addr_make("non_admin"),
        non_depositor: deps.api.addr_make("non_depositor"),
    }
}

pub const TEST_DENOM: &str = "ibc/0E293A7622DC9A6439DB60E6D234B5AF446962E27CA3AB44D0590603DFF6968E";
pub const TEST_HUB_DENOM: &str =
    "ibc/D742E8566B0B8CC8F569D950051C09CF57988A88F0E45574BFB3079D41DE6462";
pub const TEST_EVM_ADDR: &str = "fa82c937fc0f6fd3bc6c66f612cf5b539d489d21";
pub const TEST_RECOVER_ADDR: &str = "cosmos1k64ssp5pnkmwtndfzvgtnjmhx06w8mdvhpatyg";
pub const TEST_CHAIN_ID: &str = "ethereum-1";

pub fn create_test_chain_config() -> ChainConfig {
    ChainConfig {
        chain_id: TEST_CHAIN_ID.to_string(),
        eureka_source_channel: "08-wasm-1369".to_string(),
        eureka_fee_receiver: "cosmos1066ea436np9m6gf4q95q0nte2ctq84wuzahttk".to_string(),
        min_eureka_fee: Uint128::new(100),
        max_eureka_fee: Uint128::new(10_000),
    }
}

pub fn create_test_token_config() -> TokenConfig {
    TokenConfig {
        denom: TEST_DENOM.to_string(),
        hub_denom: TEST_HUB_DENOM.to_string(),
    }
}

pub fn setup_contract_with_chain() -> (
    OwnedDeps<MockStorage, MockApi, MockQuerier<NeutronQuery>, NeutronQuery>,
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
        skip_entry_point: "cosmos1clswlqlfm8gpn7n5wu0ypu0ugaj36urlhj7yz30hn7v7mkcm2tuqy9f8s5"
            .to_string(),
        skip_ibc_adapter: "cosmos1lqu9662kd4my6dww4gzp3730vew0gkwe0nl9ztjh0n5da0a8zc4swsvd22"
            .to_string(),
        neutron_to_hub_channel: "channel-1".to_string(),
        ibc_default_timeout_seconds: 600,
        initial_depositors: vec![InitialDepositor {
            address: test_data.depositor.to_string(),
            capabilities: None,
        }],
        initial_chains: vec![InitialChainConfig {
            chain_config: create_test_chain_config(),
            initial_allowed_destination_addresses: vec![format!("0x{}", TEST_EVM_ADDR)],
        }],
        initial_tokens: vec![create_test_token_config()],
        initial_executors: vec![InitialExecutor {
            address: test_data.executor.to_string(),
        }],
        initial_recover_addresses: vec![TEST_RECOVER_ADDR.to_string()],
    };

    instantiate(deps.as_mut(), env, info, msg).unwrap();
    (deps, test_data)
}
