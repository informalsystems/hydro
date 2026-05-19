use cosmwasm_std::{testing::mock_env, Coin, MessageInfo, Uint128};
use neutron_sdk::bindings::msg::NeutronMsg;

use crate::contract::execute;
use crate::msg::{EurekaAdapterMsg, ExecuteMsg};
use crate::state::TransferFundsInstructions;
use crate::testing_mocks::{
    setup_contract_with_chain, TEST_CHAIN_ID, TEST_DENOM, TEST_EVM_ADDR, TEST_RECOVER_ADDR,
};

fn transfer_instructions(amount: u128) -> TransferFundsInstructions {
    TransferFundsInstructions {
        chain_id: TEST_CHAIN_ID.to_string(),
        recipient: format!("0x{}", TEST_EVM_ADDR),
        recover_address: TEST_RECOVER_ADDR.to_string(),
        denom: TEST_DENOM.to_string(),
        amount: Uint128::new(amount),
    }
}

#[test]
fn test_transfer_funds_happy_path() {
    let (mut deps, test_data) = setup_contract_with_chain();
    let env = mock_env();

    // Fund the contract with the bridge amount
    deps.querier.bank.update_balance(
        env.contract.address.clone(),
        vec![Coin {
            denom: TEST_DENOM.to_string(),
            amount: Uint128::new(1_000 + 500), // 1000 to bridge + 500 fee (from executor)
        }],
    );

    let fee_amount = Uint128::new(500);
    let info = MessageInfo {
        sender: test_data.executor.clone(),
        funds: vec![Coin {
            denom: TEST_DENOM.to_string(),
            amount: fee_amount,
        }],
    };

    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CustomAction(EurekaAdapterMsg::TransferFunds {
            instructions: transfer_instructions(1_000),
        }),
    )
    .unwrap();

    // Should emit one IBC transfer message
    assert_eq!(res.messages.len(), 1);
    assert!(matches!(
        res.messages[0].msg,
        cosmwasm_std::CosmosMsg::Custom(NeutronMsg::IbcTransfer { .. })
    ));
}

#[test]
fn test_transfer_funds_fee_too_low() {
    let (mut deps, test_data) = setup_contract_with_chain();
    let env = mock_env();

    deps.querier.bank.update_balance(
        env.contract.address.clone(),
        vec![Coin {
            denom: TEST_DENOM.to_string(),
            amount: Uint128::new(2_000),
        }],
    );

    // min_eureka_fee is 100, send only 50
    let info = MessageInfo {
        sender: test_data.executor.clone(),
        funds: vec![Coin {
            denom: TEST_DENOM.to_string(),
            amount: Uint128::new(50),
        }],
    };

    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CustomAction(EurekaAdapterMsg::TransferFunds {
            instructions: transfer_instructions(1_000),
        }),
    )
    .unwrap_err();

    assert!(err.to_string().contains("too low"));
}

#[test]
fn test_transfer_funds_fee_too_high() {
    let (mut deps, test_data) = setup_contract_with_chain();
    let env = mock_env();

    deps.querier.bank.update_balance(
        env.contract.address.clone(),
        vec![Coin {
            denom: TEST_DENOM.to_string(),
            amount: Uint128::new(100_000),
        }],
    );

    // max_eureka_fee is 10_000, send 20_000
    let info = MessageInfo {
        sender: test_data.executor.clone(),
        funds: vec![Coin {
            denom: TEST_DENOM.to_string(),
            amount: Uint128::new(20_000),
        }],
    };

    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CustomAction(EurekaAdapterMsg::TransferFunds {
            instructions: transfer_instructions(1_000),
        }),
    )
    .unwrap_err();

    assert!(err.to_string().contains("too high"));
}

#[test]
fn test_transfer_funds_unregistered_denom() {
    let (mut deps, test_data) = setup_contract_with_chain();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.executor.clone(),
        funds: vec![Coin {
            denom: "ibc/UNKNOWN".to_string(),
            amount: Uint128::new(500),
        }],
    };

    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CustomAction(EurekaAdapterMsg::TransferFunds {
            instructions: TransferFundsInstructions {
                chain_id: TEST_CHAIN_ID.to_string(),
                recipient: format!("0x{}", TEST_EVM_ADDR),
                recover_address: TEST_RECOVER_ADDR.to_string(),
                denom: "ibc/UNKNOWN".to_string(),
                amount: Uint128::new(1_000),
            },
        }),
    )
    .unwrap_err();

    assert!(err.to_string().contains("Token not registered"));
}

#[test]
fn test_transfer_funds_unknown_destination() {
    let (mut deps, test_data) = setup_contract_with_chain();
    let env = mock_env();

    deps.querier.bank.update_balance(
        env.contract.address.clone(),
        vec![Coin {
            denom: TEST_DENOM.to_string(),
            amount: Uint128::new(2_000),
        }],
    );

    let info = MessageInfo {
        sender: test_data.executor.clone(),
        funds: vec![Coin {
            denom: TEST_DENOM.to_string(),
            amount: Uint128::new(500),
        }],
    };

    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CustomAction(EurekaAdapterMsg::TransferFunds {
            instructions: TransferFundsInstructions {
                chain_id: TEST_CHAIN_ID.to_string(),
                recipient: "0x1234567890123456789012345678901234567890".to_string(),
                recover_address: TEST_RECOVER_ADDR.to_string(),
                denom: TEST_DENOM.to_string(),
                amount: Uint128::new(1_000),
            },
        }),
    )
    .unwrap_err();

    assert!(err.to_string().contains("not allowed"));
}

#[test]
fn test_transfer_funds_unknown_recover_address() {
    let (mut deps, test_data) = setup_contract_with_chain();
    let env = mock_env();

    deps.querier.bank.update_balance(
        env.contract.address.clone(),
        vec![Coin {
            denom: TEST_DENOM.to_string(),
            amount: Uint128::new(2_000),
        }],
    );

    let info = MessageInfo {
        sender: test_data.executor.clone(),
        funds: vec![Coin {
            denom: TEST_DENOM.to_string(),
            amount: Uint128::new(500),
        }],
    };

    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CustomAction(EurekaAdapterMsg::TransferFunds {
            instructions: TransferFundsInstructions {
                chain_id: TEST_CHAIN_ID.to_string(),
                recipient: format!("0x{}", TEST_EVM_ADDR),
                recover_address: "cosmos1qyqa2zn5c925gfgx6qnweh2em2qk6l2c8qegzq".to_string(),
                denom: TEST_DENOM.to_string(),
                amount: Uint128::new(1_000),
            },
        }),
    )
    .unwrap_err();

    assert!(err.to_string().contains("Recover address not allowed"));
}

#[test]
fn test_transfer_funds_insufficient_balance() {
    let (mut deps, test_data) = setup_contract_with_chain();
    let env = mock_env();

    // Only put fee amount in balance — not enough for the bridge amount
    deps.querier.bank.update_balance(
        env.contract.address.clone(),
        vec![Coin {
            denom: TEST_DENOM.to_string(),
            amount: Uint128::new(500), // just the fee, no bridge amount
        }],
    );

    let info = MessageInfo {
        sender: test_data.executor.clone(),
        funds: vec![Coin {
            denom: TEST_DENOM.to_string(),
            amount: Uint128::new(500),
        }],
    };

    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CustomAction(EurekaAdapterMsg::TransferFunds {
            instructions: transfer_instructions(1_000),
        }),
    )
    .unwrap_err();

    assert!(err.to_string().contains("Insufficient balance"));
}

#[test]
fn test_non_executor_cannot_transfer() {
    let (mut deps, test_data) = setup_contract_with_chain();
    let env = mock_env();

    let info = MessageInfo {
        sender: test_data.non_admin.clone(),
        funds: vec![Coin {
            denom: TEST_DENOM.to_string(),
            amount: Uint128::new(500),
        }],
    };

    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CustomAction(EurekaAdapterMsg::TransferFunds {
            instructions: transfer_instructions(1_000),
        }),
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}
