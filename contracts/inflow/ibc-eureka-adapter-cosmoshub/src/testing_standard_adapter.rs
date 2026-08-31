use cosmwasm_std::{coin, testing::mock_env, MessageInfo, Uint128};
use interface::inflow_adapter::{
    deserialize_adapter_interface_msg, serialize_adapter_interface_msg, AdapterInterfaceMsg,
    AdapterInterfaceQueryMsg, AdminsResponse, AvailableAmountResponse,
    RegisteredDepositorsResponse,
};

use crate::contract::{execute, instantiate, query};
use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InitialDepositor, InitialExecutor, InstantiateMsg, QueryMsg};
use crate::testing_mocks::{
    default_test_setup, mock_dependencies, setup_contract_with_defaults, setup_contract_with_denom,
    TEST_DENOM,
};

fn wrap_standard(msg: AdapterInterfaceMsg) -> ExecuteMsg {
    let deserialized =
        deserialize_adapter_interface_msg(&serialize_adapter_interface_msg(&msg).unwrap()).unwrap();
    ExecuteMsg::StandardAction(deserialized)
}

#[test]
fn test_instantiate_requires_at_least_one_admin() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };

    let msg = InstantiateMsg {
        admins: vec![],
        skip_swap_entry_point_contract: deps
            .api
            .addr_make("skip_swap_entry_point_contract")
            .to_string(),
        source_channel: "08-wasm-1369".to_string(),
        eureka_fee_receiver: deps.api.addr_make("eureka_fee_receiver").to_string(),
        ibc_transfer_timeout_seconds: 3600,
        eureka_fee_timeout_seconds: 600,
        initial_depositors: vec![],
        initial_denoms: vec![],
        initial_allowed_destination_addresses: vec![],
        initial_executors: vec![],
    };

    let err = instantiate(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(err, ContractError::AtLeastOneAdmin {});
}

#[test]
fn test_instantiate_rejects_empty_source_channel() {
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
        source_channel: "  ".to_string(),
        eureka_fee_receiver: test_data.eureka_fee_receiver.to_string(),
        ibc_transfer_timeout_seconds: 3600,
        eureka_fee_timeout_seconds: 600,
        initial_depositors: vec![],
        initial_denoms: vec![],
        initial_allowed_destination_addresses: vec![],
        initial_executors: vec![],
    };

    let err = instantiate(deps.as_mut(), env, info, msg).unwrap_err();
    assert!(matches!(err, ContractError::InvalidConfig { .. }));
}

#[test]
fn test_instantiate_rejects_duplicate_depositor() {
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
        initial_depositors: vec![
            InitialDepositor {
                address: test_data.depositor.to_string(),
                capabilities: None,
            },
            InitialDepositor {
                address: test_data.depositor.to_string(),
                capabilities: None,
            },
        ],
        initial_denoms: vec![],
        initial_allowed_destination_addresses: vec![],
        initial_executors: vec![InitialExecutor {
            address: test_data.executor.to_string(),
        }],
    };

    let err = instantiate(deps.as_mut(), env, info, msg).unwrap_err();
    assert!(matches!(
        err,
        ContractError::DepositorAlreadyRegistered { .. }
    ));
}

#[test]
fn test_deposit_success() {
    let (mut deps, test_data) = setup_contract_with_denom();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.depositor.clone(),
        funds: vec![coin(1_000_000, TEST_DENOM)],
    };

    let res = execute(
        deps.as_mut(),
        env,
        info,
        wrap_standard(AdapterInterfaceMsg::Deposit {}),
    )
    .unwrap();

    assert!(res
        .attributes
        .iter()
        .any(|a| a.key == "action" && a.value == "deposit"));
}

#[test]
fn test_deposit_rejects_disallowed_denom() {
    let (mut deps, test_data) = setup_contract_with_denom();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.depositor.clone(),
        funds: vec![coin(1_000_000, "ibc/unregistered")],
    };

    let err = execute(
        deps.as_mut(),
        env,
        info,
        wrap_standard(AdapterInterfaceMsg::Deposit {}),
    )
    .unwrap_err();

    assert!(matches!(err, ContractError::DenomNotAllowed { .. }));
}

#[test]
fn test_deposit_rejects_non_depositor() {
    let (mut deps, test_data) = setup_contract_with_denom();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.non_depositor.clone(),
        funds: vec![coin(1_000_000, TEST_DENOM)],
    };

    let err = execute(
        deps.as_mut(),
        env,
        info,
        wrap_standard(AdapterInterfaceMsg::Deposit {}),
    )
    .unwrap_err();

    assert!(matches!(err, ContractError::DepositorNotRegistered { .. }));
}

#[test]
fn test_withdraw_success() {
    let (mut deps, test_data) = setup_contract_with_denom();
    let env = mock_env();

    deps.querier.bank.update_balance(
        env.contract.address.clone(),
        vec![coin(1_000_000, TEST_DENOM)],
    );

    let info = MessageInfo {
        sender: test_data.depositor.clone(),
        funds: vec![],
    };

    let res = execute(
        deps.as_mut(),
        env,
        info,
        wrap_standard(AdapterInterfaceMsg::Withdraw {
            coin: coin(500_000, TEST_DENOM),
        }),
    )
    .unwrap();

    assert_eq!(res.messages.len(), 1);
}

#[test]
fn test_withdraw_insufficient_balance() {
    let (mut deps, test_data) = setup_contract_with_denom();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.depositor.clone(),
        funds: vec![],
    };

    let err = execute(
        deps.as_mut(),
        env,
        info,
        wrap_standard(AdapterInterfaceMsg::Withdraw {
            coin: coin(500_000, TEST_DENOM),
        }),
    )
    .unwrap_err();

    assert!(matches!(err, ContractError::InsufficientBalance { .. }));
}

#[test]
fn test_config_query() {
    let (deps, test_data) = setup_contract_with_denom();

    let res: cosmwasm_std::Binary = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::Config {}),
    )
    .unwrap();

    let response: crate::msg::ConfigResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(
        response.config.skip_swap_entry_point_contract,
        test_data.skip_swap_entry_point_contract.to_string()
    );
}

#[test]
fn test_available_for_deposit() {
    let (deps, test_data) = setup_contract_with_denom();

    let res: cosmwasm_std::Binary = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::AvailableForDeposit {
            depositor_address: test_data.depositor.to_string(),
            denom: TEST_DENOM.to_string(),
        }),
    )
    .unwrap();
    let response: AvailableAmountResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(response.amount, Uint128::MAX);

    let res: cosmwasm_std::Binary = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::AvailableForDeposit {
            depositor_address: test_data.depositor.to_string(),
            denom: "ibc/unregistered".to_string(),
        }),
    )
    .unwrap();
    let response: AvailableAmountResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(response.amount, Uint128::zero());
}

#[test]
fn test_add_and_remove_admin() {
    let (mut deps, test_data) = setup_contract_with_defaults();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.admin.clone(),
        funds: vec![],
    };

    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        wrap_standard(AdapterInterfaceMsg::AddAdmin {
            admin_address: test_data.admin2.to_string(),
        }),
    )
    .unwrap();

    let res: cosmwasm_std::Binary = query(
        deps.as_ref(),
        env.clone(),
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::Admins {}),
    )
    .unwrap();
    let response: AdminsResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(response.admins.len(), 2);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        wrap_standard(AdapterInterfaceMsg::RemoveAdmin {
            admin_address: test_data.admin2.to_string(),
        }),
    )
    .unwrap();

    let res: cosmwasm_std::Binary = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::Admins {}),
    )
    .unwrap();
    let response: AdminsResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(response.admins.len(), 1);
}

#[test]
fn test_cannot_remove_last_admin() {
    let (mut deps, test_data) = setup_contract_with_defaults();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.admin.clone(),
        funds: vec![],
    };

    let err = execute(
        deps.as_mut(),
        env,
        info,
        wrap_standard(AdapterInterfaceMsg::RemoveAdmin {
            admin_address: test_data.admin.to_string(),
        }),
    )
    .unwrap_err();

    assert_eq!(err, ContractError::CannotRemoveLastAdmin {});
}

#[test]
fn test_register_unregister_and_toggle_depositor() {
    let (mut deps, test_data) = setup_contract_with_defaults();
    let env = mock_env();

    let admin_info = MessageInfo {
        sender: test_data.admin.clone(),
        funds: vec![],
    };

    execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        wrap_standard(AdapterInterfaceMsg::RegisterDepositor {
            depositor_address: test_data.depositor2.to_string(),
            metadata: None,
        }),
    )
    .unwrap();

    execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        wrap_standard(AdapterInterfaceMsg::SetDepositorEnabled {
            depositor_address: test_data.depositor2.to_string(),
            enabled: false,
        }),
    )
    .unwrap();

    let res: cosmwasm_std::Binary = query(
        deps.as_ref(),
        env.clone(),
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::RegisteredDepositors {
            enabled: Some(false),
        }),
    )
    .unwrap();
    let response: RegisteredDepositorsResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(response.depositors.len(), 1);
    assert_eq!(
        response.depositors[0].depositor_address,
        test_data.depositor2.to_string()
    );

    execute(
        deps.as_mut(),
        env,
        admin_info,
        wrap_standard(AdapterInterfaceMsg::UnregisterDepositor {
            depositor_address: test_data.depositor2.to_string(),
        }),
    )
    .unwrap();
}
