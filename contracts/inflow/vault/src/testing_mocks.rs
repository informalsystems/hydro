use std::{cell::RefCell, collections::HashMap, rc::Rc};

use cosmwasm_std::{
    from_json,
    testing::{MockApi, MockQuerier, MockStorage},
    to_json_binary, Addr, ContractResult, Decimal, OwnedDeps, QuerierResult, SystemError,
    SystemResult, Uint128, WasmQuery,
};
use interface::{
    adapter::{AdapterQueryMsg, AvailableAmountResponse, InflowDepositResponse},
    inflow_control_center::{
        Config, ConfigResponse, PoolInfoResponse, QueryMsg as ControlCenterQueryMsg,
    },
    token_info_provider::TokenInfoProviderQueryMsg,
};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::testing::{CONTROL_CENTER_ADDR, DEFAULT_DEPOSIT_CAP};

pub type WasmQueryFunc = Box<dyn Fn(&WasmQuery) -> QuerierResult>;

#[derive(Clone)]
pub struct MockWasmQuerier {
    contract_mocks: Rc<RefCell<HashMap<String, WasmQueryFunc>>>,
}

impl MockWasmQuerier {
    pub fn new(contract_mocks: HashMap<String, WasmQueryFunc>) -> Self {
        Self {
            contract_mocks: Rc::new(RefCell::new(contract_mocks)),
        }
    }

    pub fn insert_mock(&self, mock: (String, WasmQueryFunc)) {
        self.contract_mocks.borrow_mut().insert(mock.0, mock.1);
    }

    pub fn handler(&self, query: &WasmQuery) -> QuerierResult {
        let contract_addr = match query {
            WasmQuery::Smart {
                contract_addr,
                msg: _,
            } => contract_addr.clone(),
            WasmQuery::Raw {
                contract_addr,
                key: _,
            } => contract_addr.clone(),
            WasmQuery::ContractInfo { contract_addr } => contract_addr.clone(),
            _ => panic!("unsupported query type"),
        };

        let contract_mocks = self.contract_mocks.borrow();
        let handler = contract_mocks
            .get(&contract_addr)
            .expect("no mock handler for the provided contract address");

        (handler)(query)
    }
}

pub fn update_contract_mock(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
    wasm_querier: &MockWasmQuerier,
    mock: (String, WasmQueryFunc),
) {
    // Cloning allows us to have a single instance of the `contract_mocks` referenced by multiple MockWasmQueriers.
    // Since `contract_mocks` is Rc struct, this way we can update only those mocks that we need to change, without
    // needing to re-instantiate the ones that didn't change.
    let querier_for_deps = wasm_querier.clone();
    querier_for_deps.insert_mock(mock);
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));
}

pub fn setup_control_center_mock(
    contract: Addr,
    deposit_cap: Uint128,
    total_pool_value: Uint128,
    total_shares_issued: Uint128,
) -> (String, WasmQueryFunc) {
    let contract_addr = contract.to_string();

    let response = Box::new(move |query: &WasmQuery| match query {
        WasmQuery::Smart { contract_addr, msg } => {
            if contract_addr != &contract.to_string() {
                return SystemResult::Err(SystemError::UnsupportedRequest {
                    kind: "unexpected contract address in control center mock".to_string(),
                });
            }

            let response = match from_json(msg).unwrap() {
                ControlCenterQueryMsg::PoolInfo {} => to_json_binary(&PoolInfoResponse {
                    total_pool_value,
                    total_shares_issued,
                }),
                ControlCenterQueryMsg::Config {} => to_json_binary(&ConfigResponse {
                    config: Config { deposit_cap },
                }),
                _ => {
                    return SystemResult::Err(SystemError::UnsupportedRequest {
                        kind: "unsupported query type in control center mock".to_string(),
                    });
                }
            };

            SystemResult::Ok(ContractResult::Ok(response.unwrap()))
        }
        _ => SystemResult::Err(SystemError::UnsupportedRequest {
            kind: "only smart queries are supported in control center mock".to_string(),
        }),
    });

    (contract_addr, response)
}

pub fn setup_token_info_provider_mock(
    contract: Addr,
    token_denom: String,
    token_ratio: Decimal,
) -> (String, WasmQueryFunc) {
    let contract_addr = contract.to_string();

    let response = Box::new(move |query: &WasmQuery| match query {
        WasmQuery::Smart { contract_addr, msg } => {
            if contract_addr != &contract.to_string() {
                return SystemResult::Err(SystemError::UnsupportedRequest {
                    kind: "unexpected contract address in token info provider mock".to_string(),
                });
            }

            let response = match from_json(msg).unwrap() {
                TokenInfoProviderQueryMsg::RatioToBaseToken { denom } => {
                    if denom != token_denom {
                        return SystemResult::Err(SystemError::UnsupportedRequest {
                            kind: "unexpected token denom in token info provider mock".to_string(),
                        });
                    }

                    to_json_binary(&token_ratio)
                }
                _ => {
                    return SystemResult::Err(SystemError::UnsupportedRequest {
                        kind: "unsupported query type in token info provider mock".to_string(),
                    });
                }
            };

            SystemResult::Ok(ContractResult::Ok(response.unwrap()))
        }
        _ => SystemResult::Err(SystemError::UnsupportedRequest {
            kind: "only smart queries are supported in token info provider mock".to_string(),
        }),
    });

    (contract_addr, response)
}

pub fn setup_default_control_center_mock(
    total_pool_value: Uint128,
    total_shares_issued: Uint128,
) -> (String, WasmQueryFunc) {
    setup_control_center_mock(
        Addr::unchecked(CONTROL_CENTER_ADDR),
        DEFAULT_DEPOSIT_CAP,
        total_pool_value,
        total_shares_issued,
    )
}

/// Configuration for a mock adapter
#[derive(Clone, Debug)]
pub struct MockAdapterConfig {
    pub available_for_deposit: Uint128,
    pub available_for_withdraw: Uint128,
    pub current_deposit: Uint128,
    pub should_fail_queries: bool,
}

impl MockAdapterConfig {
    pub fn new(deposit_capacity: u128, withdraw_capacity: u128, current_deposit: u128) -> Self {
        Self {
            available_for_deposit: Uint128::new(deposit_capacity),
            available_for_withdraw: Uint128::new(withdraw_capacity),
            current_deposit: Uint128::new(current_deposit),
            should_fail_queries: false,
        }
    }

    pub fn failing() -> Self {
        Self {
            available_for_deposit: Uint128::zero(),
            available_for_withdraw: Uint128::zero(),
            current_deposit: Uint128::zero(),
            should_fail_queries: true,
        }
    }
}

pub fn setup_adapter_mock(contract: Addr, config: MockAdapterConfig) -> (String, WasmQueryFunc) {
    let contract_addr = contract.to_string();

    let response = Box::new(move |query: &WasmQuery| match query {
        WasmQuery::Smart { contract_addr, msg } => {
            if contract_addr != &contract.to_string() {
                return SystemResult::Err(SystemError::UnsupportedRequest {
                    kind: "unexpected contract address in adapter mock".to_string(),
                });
            }

            if config.should_fail_queries {
                return SystemResult::Err(SystemError::InvalidRequest {
                    error: "Mock adapter query failure".to_string(),
                    request: msg.clone(),
                });
            }

            let response = match from_json(msg).unwrap() {
                AdapterQueryMsg::AvailableForDeposit { .. } => {
                    to_json_binary(&AvailableAmountResponse {
                        amount: config.available_for_deposit,
                    })
                }
                AdapterQueryMsg::AvailableForWithdraw { .. } => {
                    to_json_binary(&AvailableAmountResponse {
                        amount: config.available_for_withdraw,
                    })
                }
                AdapterQueryMsg::InflowDeposit { .. } => to_json_binary(&InflowDepositResponse {
                    amount: config.current_deposit,
                }),
                _ => {
                    return SystemResult::Err(SystemError::UnsupportedRequest {
                        kind: "unsupported query type in adapter mock".to_string(),
                    });
                }
            };

            SystemResult::Ok(ContractResult::Ok(response.unwrap()))
        }
        _ => SystemResult::Err(SystemError::UnsupportedRequest {
            kind: "only smart queries are supported in adapter mock".to_string(),
        }),
    });

    (contract_addr, response)
}
