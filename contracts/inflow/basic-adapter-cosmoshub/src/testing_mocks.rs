use cosmwasm_std::{
    testing::{mock_env, MockApi, MockQuerier, MockStorage},
    Addr, Empty, MessageInfo, OwnedDeps,
};

use crate::contract::instantiate;
use crate::msg::InstantiateMsg;

pub fn mock_dependencies() -> OwnedDeps<MockStorage, MockApi, MockQuerier, Empty> {
    let mut deps = cosmwasm_std::testing::mock_dependencies();
    deps.api = MockApi::default().with_prefix("cosmos");
    deps
}

pub struct TestSetupData {
    pub admin: Addr,
    pub admin2: Addr,
    pub depositor: Addr,
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
        non_admin: deps.api.addr_make("non_admin"),
        non_depositor: deps.api.addr_make("non_depositor"),
    }
}

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
        initial_depositors: vec![test_data.depositor.to_string()],
    };

    instantiate(deps.as_mut(), env, info, msg).unwrap();
    (deps, test_data)
}
