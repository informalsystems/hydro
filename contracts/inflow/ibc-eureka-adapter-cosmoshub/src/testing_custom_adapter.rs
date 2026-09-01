use cosmwasm_std::{coin, coins, testing::mock_env, MessageInfo, Uint128};

use crate::contract::execute;
use crate::error::ContractError;
use crate::msg::{ExecuteMsg, IbcEurekaAdapterMsg};
use crate::testing_mocks::{
    setup_contract_with_defaults, setup_contract_with_denom, TEST_DENOM, TEST_DESTINATION_ADDRESS,
};

fn custom(msg: IbcEurekaAdapterMsg) -> ExecuteMsg {
    ExecuteMsg::CustomAction(msg)
}

#[test]
fn test_add_and_remove_executor() {
    let (mut deps, test_data) = setup_contract_with_defaults();
    let env = mock_env();

    let admin_info = MessageInfo {
        sender: test_data.admin.clone(),
        funds: vec![],
    };

    let new_executor = deps.api.addr_make("executor2");

    execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        custom(IbcEurekaAdapterMsg::AddExecutor {
            executor_address: new_executor.to_string(),
        }),
    )
    .unwrap();

    let err = execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        custom(IbcEurekaAdapterMsg::AddExecutor {
            executor_address: new_executor.to_string(),
        }),
    )
    .unwrap_err();
    assert!(matches!(err, ContractError::ExecutorAlreadyExists { .. }));

    execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        custom(IbcEurekaAdapterMsg::RemoveExecutor {
            executor_address: new_executor.to_string(),
        }),
    )
    .unwrap();

    let err = execute(
        deps.as_mut(),
        env,
        admin_info,
        custom(IbcEurekaAdapterMsg::RemoveExecutor {
            executor_address: new_executor.to_string(),
        }),
    )
    .unwrap_err();
    assert!(matches!(err, ContractError::ExecutorNotFound { .. }));
}

#[test]
fn test_add_executor_unauthorized() {
    let (mut deps, test_data) = setup_contract_with_defaults();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.non_admin.clone(),
        funds: vec![],
    };

    let err = execute(
        deps.as_mut(),
        env,
        info,
        custom(IbcEurekaAdapterMsg::AddExecutor {
            executor_address: test_data.non_admin.to_string(),
        }),
    )
    .unwrap_err();

    assert_eq!(err, ContractError::UnauthorizedAdmin {});
}

#[test]
fn test_add_remove_allowed_denom() {
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
        custom(IbcEurekaAdapterMsg::AddAllowedDenom {
            denom: TEST_DENOM.to_string(),
        }),
    )
    .unwrap();

    let err = execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        custom(IbcEurekaAdapterMsg::AddAllowedDenom {
            denom: TEST_DENOM.to_string(),
        }),
    )
    .unwrap_err();
    assert!(matches!(err, ContractError::DenomAlreadyAllowed { .. }));

    execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        custom(IbcEurekaAdapterMsg::RemoveAllowedDenom {
            denom: TEST_DENOM.to_string(),
        }),
    )
    .unwrap();

    let err = execute(
        deps.as_mut(),
        env,
        admin_info,
        custom(IbcEurekaAdapterMsg::RemoveAllowedDenom {
            denom: TEST_DENOM.to_string(),
        }),
    )
    .unwrap_err();
    assert!(matches!(err, ContractError::DenomNotAllowed { .. }));
}

#[test]
fn test_add_remove_allowed_destination_address() {
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
        custom(IbcEurekaAdapterMsg::AddAllowedDestinationAddress {
            address: TEST_DESTINATION_ADDRESS.to_string(),
        }),
    )
    .unwrap();

    let err = execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        custom(IbcEurekaAdapterMsg::AddAllowedDestinationAddress {
            address: TEST_DESTINATION_ADDRESS.to_string(),
        }),
    )
    .unwrap_err();
    assert!(matches!(
        err,
        ContractError::DestinationAddressAlreadyExists { .. }
    ));

    execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        custom(IbcEurekaAdapterMsg::RemoveAllowedDestinationAddress {
            address: TEST_DESTINATION_ADDRESS.to_string(),
        }),
    )
    .unwrap();

    let err = execute(
        deps.as_mut(),
        env,
        admin_info,
        custom(IbcEurekaAdapterMsg::RemoveAllowedDestinationAddress {
            address: TEST_DESTINATION_ADDRESS.to_string(),
        }),
    )
    .unwrap_err();
    assert!(matches!(
        err,
        ContractError::DestinationAddressDoesNotExist { .. }
    ));
}

fn transfer_funds_msg(amount: u128) -> ExecuteMsg {
    custom(IbcEurekaAdapterMsg::TransferFunds {
        denom: TEST_DENOM.to_string(),
        amount: Uint128::new(amount),
        recipient: TEST_DESTINATION_ADDRESS.to_string(),
    })
}

#[test]
fn test_transfer_funds_success() {
    let (mut deps, test_data) = setup_contract_with_denom();
    let env = mock_env();

    deps.querier.bank.update_balance(
        env.contract.address.clone(),
        vec![coin(10_000_213, TEST_DENOM)],
    );

    let info = MessageInfo {
        sender: test_data.executor.clone(),
        funds: coins(213, TEST_DENOM),
    };

    let res = execute(deps.as_mut(), env, info, transfer_funds_msg(9_999_787)).unwrap();

    assert_eq!(res.messages.len(), 1);
    assert!(res
        .attributes
        .iter()
        .any(|a| a.key == "action" && a.value == "transfer_funds"));
}

#[test]
fn test_transfer_funds_unauthorized() {
    let (mut deps, test_data) = setup_contract_with_denom();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.non_admin.clone(),
        funds: coins(10, TEST_DENOM),
    };

    let err = execute(deps.as_mut(), env, info, transfer_funds_msg(1_000)).unwrap_err();
    assert_eq!(err, ContractError::UnauthorizedExecutor {});
}

#[test]
fn test_transfer_funds_zero_amount() {
    let (mut deps, test_data) = setup_contract_with_denom();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.executor.clone(),
        funds: coins(10, TEST_DENOM),
    };

    let err = execute(deps.as_mut(), env, info, transfer_funds_msg(0)).unwrap_err();
    assert_eq!(err, ContractError::ZeroAmount {});
}

#[test]
fn test_transfer_funds_missing_fee() {
    let (mut deps, test_data) = setup_contract_with_denom();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.executor.clone(),
        funds: vec![],
    };

    let err = execute(deps.as_mut(), env, info, transfer_funds_msg(1_000)).unwrap_err();
    assert!(matches!(err, ContractError::PaymentError(_)));
}

#[test]
fn test_transfer_funds_denom_not_allowed() {
    let (mut deps, test_data) = setup_contract_with_defaults();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.executor.clone(),
        funds: coins(10, TEST_DENOM),
    };

    let err = execute(deps.as_mut(), env, info, transfer_funds_msg(1_000)).unwrap_err();
    assert!(matches!(err, ContractError::DenomNotAllowed { .. }));
}

#[test]
fn test_transfer_funds_insufficient_balance() {
    let (mut deps, test_data) = setup_contract_with_denom();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.executor.clone(),
        funds: coins(10, TEST_DENOM),
    };

    let err = execute(deps.as_mut(), env, info, transfer_funds_msg(1_000)).unwrap_err();
    assert!(matches!(err, ContractError::InsufficientBalance { .. }));
}

#[test]
fn test_transfer_funds_recipient_not_allowed() {
    let (mut deps, test_data) = setup_contract_with_denom();
    let env = mock_env();

    deps.querier.bank.update_balance(
        env.contract.address.clone(),
        vec![coin(10_000_010, TEST_DENOM)],
    );

    let info = MessageInfo {
        sender: test_data.executor.clone(),
        funds: coins(10, TEST_DENOM),
    };

    let msg = custom(IbcEurekaAdapterMsg::TransferFunds {
        denom: TEST_DENOM.to_string(),
        amount: Uint128::new(1_000),
        recipient: "0x000000000000000000000000000000000000dead".to_string(),
    });

    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert!(matches!(
        err,
        ContractError::DestinationAddressNotAllowed { .. }
    ));
}
