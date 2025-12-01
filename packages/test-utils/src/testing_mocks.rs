use std::{cell::RefCell, collections::HashMap, rc::Rc};

use cosmwasm_std::{
    testing::{MockApi, MockQuerier, MockStorage},
    Addr, Binary, ContractResult, CustomQuery, OwnedDeps, QuerierResult, StdResult, SystemError,
    SystemResult, WasmQuery,
};

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

pub fn update_contract_mock<C: CustomQuery>(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, C>,
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

pub fn setup_contract_smart_query_mock<T>(
    contract: Addr,
    smart_query_handler: T,
) -> (String, WasmQueryFunc)
where
    T: Fn(&Binary) -> StdResult<Binary> + 'static,
{
    let contract_addr = contract.to_string();

    let response = Box::new(move |query: &WasmQuery| match query {
        WasmQuery::Smart { contract_addr, msg } => {
            if contract_addr != &contract.to_string() {
                return SystemResult::Err(SystemError::UnsupportedRequest {
                    kind: "unexpected contract address in smart query contract mock".to_string(),
                });
            }

            let response = match smart_query_handler(msg) {
                Ok(response) => response,
                Err(e) => {
                    return SystemResult::Err(SystemError::UnsupportedRequest {
                        kind: format!("error returned by contract mock: {e}").to_string(),
                    });
                }
            };

            SystemResult::Ok(ContractResult::Ok(response))
        }
        _ => SystemResult::Err(SystemError::UnsupportedRequest {
            kind: "only smart queries are supported in this mock".to_string(),
        }),
    });

    (contract_addr, response)
}
