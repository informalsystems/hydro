// Tests for Mars adapter with proper Mars query mocking

use cosmwasm_std::testing::{mock_dependencies, mock_env, MockApi, MockQuerier, MockStorage};
use cosmwasm_std::{
    from_json, to_json_binary, Addr, Coin, ContractResult, Event, MessageInfo, OwnedDeps, Reply,
    SubMsgResponse, SubMsgResult, SystemError, SystemResult, Uint128, WasmQuery,
};

use crate::contract::{execute, instantiate, query, reply};
use crate::error::ContractError;
use crate::mars::{
    MarsCreditManagerQueryMsg, MarsParamsQueryMsg, PositionsResponse, TotalDepositResponse,
};
use crate::msg::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AllPositionsResponse, AvailableAmountResponse,
    DepositorPositionResponse, DepositorPositionsResponse, ExecuteMsg, InstantiateMsg,
    MarsAdapterMsg, MarsConfigResponse, QueryMsg, RegisteredDepositorsResponse,
    TimeEstimateResponse,
};
use crate::state::{Depositor, ADMINS, CONFIG, WHITELISTED_DEPOSITORS};

const USDC_DENOM: &str = "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";
const REPLY_CREATE_ACCOUNT: u64 = 1;

/// Test data structure containing all relevant addresses and messages
pub struct TestSetupData {
    pub admin: Addr,
    pub mars_credit_manager: Addr,
    pub mars_params: Addr,
    pub mars_red_bank: Addr,
    pub depositor_address: Addr,
    pub msg: InstantiateMsg,
}

/// Custom querier that mocks Mars positions queries
fn mock_dependencies_with_mars_positions(
    positions: Vec<(String, PositionsResponse)>,
) -> OwnedDeps<MockStorage, MockApi, MockQuerier> {
    let mut deps = mock_dependencies();

    // Store the positions for querying
    deps.querier.update_wasm(move |query| match query {
        WasmQuery::Smart {
            contract_addr: _,
            msg,
        } => {
            // Try to parse as Mars Positions query
            let parsed: Result<MarsCreditManagerQueryMsg, _> = from_json(msg);
            if let Ok(MarsCreditManagerQueryMsg::Positions { account_id, .. }) = parsed {
                // Find the positions for this account
                for (acc_id, pos) in &positions {
                    if acc_id == &account_id {
                        let binary = to_json_binary(pos).unwrap();
                        return SystemResult::Ok(ContractResult::Ok(binary));
                    }
                }
                // Return empty positions if not found
                let empty_positions = PositionsResponse {
                    account_id: account_id.clone(),
                    deposits: vec![],
                    debts: vec![],
                    lends: vec![],
                };
                let binary = to_json_binary(&empty_positions).unwrap();
                return SystemResult::Ok(ContractResult::Ok(binary));
            }
            SystemResult::Err(SystemError::InvalidRequest {
                error: "Unsupported query".to_string(),
                request: msg.clone(),
            })
        }
        _ => SystemResult::Err(SystemError::UnsupportedRequest {
            kind: "Not implemented".to_string(),
        }),
    });

    deps
}

/// Custom querier that mocks Mars Params TotalDeposit queries
fn mock_dependencies_with_mars_params(
    total_deposits: Vec<(String, TotalDepositResponse)>,
) -> OwnedDeps<MockStorage, MockApi, MockQuerier> {
    let mut deps = mock_dependencies();

    // Store the total deposit responses for querying
    deps.querier.update_wasm(move |query| match query {
        WasmQuery::Smart {
            contract_addr: _,
            msg,
        } => {
            // Try to parse as Mars Params TotalDeposit query
            let parsed: Result<MarsParamsQueryMsg, _> = from_json(msg);
            if let Ok(MarsParamsQueryMsg::TotalDeposit { denom }) = parsed {
                // Find the total deposit response for this denom
                for (d, response) in &total_deposits {
                    if d == &denom {
                        let binary = to_json_binary(response).unwrap();
                        return SystemResult::Ok(ContractResult::Ok(binary));
                    }
                }
                // Return default if not found (cap 0, amount 0)
                let default_response = TotalDepositResponse {
                    denom: denom.clone(),
                    cap: Uint128::zero(),
                    amount: Uint128::zero(),
                };
                let binary = to_json_binary(&default_response).unwrap();
                return SystemResult::Ok(ContractResult::Ok(binary));
            }
            SystemResult::Err(SystemError::InvalidRequest {
                error: "Unsupported query".to_string(),
                request: msg.clone(),
            })
        }
        _ => SystemResult::Err(SystemError::UnsupportedRequest {
            kind: "Not implemented".to_string(),
        }),
    });

    deps
}

fn default_instantiate_msg(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier>,
) -> TestSetupData {
    let admin = deps.api.addr_make("admin");
    let mars_credit_manager = deps.api.addr_make("mars_credit_manager");
    let mars_params = deps.api.addr_make("mars_params");
    let mars_red_bank = deps.api.addr_make("mars_red_bank");
    let depositor_address = deps.api.addr_make("depositor");

    let msg = InstantiateMsg {
        admins: vec![admin.to_string()],
        mars_credit_manager: mars_credit_manager.to_string(),
        mars_red_bank: mars_red_bank.to_string(),
        mars_params: mars_params.to_string(),
        supported_denoms: vec![USDC_DENOM.to_string()],
        initial_depositors: vec![depositor_address.to_string()],
    };

    TestSetupData {
        admin,
        mars_credit_manager,
        mars_params,
        mars_red_bank,
        depositor_address,
        msg,
    }
}

// Helper function to instantiate contract with valid addresses and simulate reply
fn setup_contract(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier>,
    mars_account_id: &str,
) -> TestSetupData {
    let env = mock_env();
    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };
    let test_data = default_instantiate_msg(&mut *deps);

    instantiate(deps.as_mut(), env.clone(), info, test_data.msg.clone()).unwrap();

    // Simulate the reply from Mars CreateCreditAccount
    let mock_reply = create_mock_mars_account_reply(mars_account_id);
    reply(deps.as_mut(), env, mock_reply).unwrap();

    test_data
}

// Helper to create a mock reply for Mars account creation
fn create_mock_mars_account_reply(account_id: &str) -> Reply {
    Reply {
        id: REPLY_CREATE_ACCOUNT,
        #[allow(deprecated)]
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![Event::new("wasm")
                .add_attribute("action", "mint")
                .add_attribute("token_id", account_id)],
            msg_responses: vec![],
            data: None,
        }),
        payload: cosmwasm_std::Binary::default(),
        gas_used: 0,
    }
}

// Helper to register a depositor with a Mars account ID for testing
fn register_depositor_with_account(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier>,
    depositor_address: Addr,
    mars_account_id: String,
) {
    let depositor = Depositor {
        mars_account_id,
        enabled: true,
    };
    WHITELISTED_DEPOSITORS
        .save(deps.as_mut().storage, depositor_address, &depositor)
        .unwrap();
}

// ============================================================================
// Instantiate Tests
// ============================================================================

#[test]
fn proper_instantiation() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };

    let test_data = default_instantiate_msg(&mut deps);

    let res = instantiate(deps.as_mut(), env, info, test_data.msg.clone()).unwrap();
    assert_eq!(res.attributes.len(), 4);
    assert_eq!(res.attributes[0].key, "action");
    assert_eq!(res.attributes[0].value, "instantiate");

    // Verify config was saved correctly
    let config = CONFIG.load(&deps.storage).unwrap();
    assert_eq!(config.mars_credit_manager, test_data.mars_credit_manager);
    assert_eq!(config.mars_params, test_data.mars_params);
    assert_eq!(config.mars_red_bank, test_data.mars_red_bank);
    assert_eq!(config.supported_denoms, vec![USDC_DENOM.to_string()]);

    // Verify admins are setup correctly
    let admins = ADMINS.load(&deps.storage).unwrap();
    assert_eq!(admins.len(), 1);
    assert_eq!(admins[0], test_data.admin);

    // Verify depositor is initialized correctly
    let depositor = WHITELISTED_DEPOSITORS
        .load(&deps.storage, test_data.depositor_address)
        .unwrap();
    assert!(depositor.enabled);
}

#[test]
fn instantiate_with_no_denoms_fails() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let admin = deps.api.addr_make("admin");
    let mars_credit_manager = deps.api.addr_make("mars_credit_manager");
    let mars_params = deps.api.addr_make("mars_params");
    let mars_red_bank = deps.api.addr_make("mars_red_bank");
    let depositor_address = deps.api.addr_make("depositor");

    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };

    let msg = InstantiateMsg {
        admins: vec![admin.to_string()],
        mars_credit_manager: mars_credit_manager.to_string(),
        mars_params: mars_params.to_string(),
        mars_red_bank: mars_red_bank.to_string(),
        supported_denoms: vec![],
        initial_depositors: vec![depositor_address.to_string()],
    };

    let err = instantiate(deps.as_mut(), env, info, msg).unwrap_err();
    assert!(matches!(err, ContractError::AtLeastOneDenom {}));
}

// ============================================================================
// Deposit Tests
// ============================================================================

#[test]
fn deposit_success() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Deposit funds
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![Coin {
            denom: USDC_DENOM.to_string(),
            amount: Uint128::new(1000),
        }],
    };

    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});

    let res = execute(deps.as_mut(), env, info, msg).unwrap();

    // Should have Mars message
    assert_eq!(res.messages.len(), 1);

    // Check attributes
    assert_eq!(res.attributes[0].value, "deposit");
    assert_eq!(res.attributes[1].value, "1000");
    assert_eq!(res.attributes[2].value, USDC_DENOM);
}

#[test]
fn deposit_unauthorized() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    setup_contract(&mut deps, "123");

    // Try deposit from unauthorized address
    let info = MessageInfo {
        sender: deps.api.addr_make("unauthorized"),
        funds: vec![Coin {
            denom: USDC_DENOM.to_string(),
            amount: Uint128::new(1000),
        }],
    };

    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});

    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(err, ContractError::Unauthorized {});
}

#[test]
fn deposit_unsupported_denom() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Try to deposit unsupported denom
    let unsupported_denom = "uatom".to_string();
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![Coin {
            denom: unsupported_denom.clone(),
            amount: Uint128::new(1000),
        }],
    };

    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});

    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::UnsupportedDenom {
            denom: unsupported_denom
        }
    );
}

#[test]
fn deposit_invalid_funds() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Try to deposit with no coins
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![],
    };

    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});

    let err = execute(deps.as_mut(), env.clone(), info, msg.clone()).unwrap_err();
    assert_eq!(err, ContractError::InvalidFunds { count: 0 });

    // Try to deposit with multiple coins
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![
            Coin {
                denom: USDC_DENOM.to_string(),
                amount: Uint128::new(1000),
            },
            Coin {
                denom: "uatom".to_string(),
                amount: Uint128::new(500),
            },
        ],
    };

    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(err, ContractError::InvalidFunds { count: 2 });
}

#[test]
fn deposit_zero_amount() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Try to deposit zero amount
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![Coin {
            denom: USDC_DENOM.to_string(),
            amount: Uint128::zero(),
        }],
    };

    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});

    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(err, ContractError::ZeroAmount {});
}

#[test]
fn deposit_multiple_times_creates_messages() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // First deposit: 1000 USDC
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![Coin {
            denom: USDC_DENOM.to_string(),
            amount: Uint128::new(1000),
        }],
    };

    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});

    let res1 = execute(deps.as_mut(), env.clone(), info, msg.clone()).unwrap();
    assert_eq!(res1.messages.len(), 1);

    // Second deposit: 500 USDC
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![Coin {
            denom: USDC_DENOM.to_string(),
            amount: Uint128::new(500),
        }],
    };

    let res2 = execute(deps.as_mut(), env.clone(), info, msg.clone()).unwrap();
    assert_eq!(res2.messages.len(), 1);

    // Third deposit: 300 USDC
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![Coin {
            denom: USDC_DENOM.to_string(),
            amount: Uint128::new(300),
        }],
    };

    let res3 = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(res3.messages.len(), 1);
}

// ============================================================================
// Withdraw Tests (with Mars query mocking)
// ============================================================================

#[test]
fn withdraw_success() {
    // Mock Mars positions: depositor has 1000 USDC lent
    let mars_account_id = "123".to_string();
    let positions = vec![(
        mars_account_id.clone(),
        PositionsResponse {
            account_id: mars_account_id.clone(),
            deposits: vec![],
            debts: vec![],
            lends: vec![Coin {
                denom: USDC_DENOM.to_string(),
                amount: Uint128::new(1000),
            }],
        },
    )];
    let mut deps = mock_dependencies_with_mars_positions(positions);
    let env = mock_env();
    let test_data = setup_contract(&mut deps, &mars_account_id);

    // Withdraw 400
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![],
    };
    let coin = Coin {
        denom: USDC_DENOM.to_string(),
        amount: Uint128::new(400),
    };
    let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw { coin });
    let res = execute(deps.as_mut(), env.clone(), info, withdraw_msg).unwrap();

    // Should have Mars message
    assert_eq!(res.messages.len(), 1);

    // Check attributes
    assert_eq!(res.attributes[0].value, "withdraw");
    assert_eq!(
        res.attributes[1].value,
        test_data.depositor_address.to_string()
    );
    assert_eq!(res.attributes[2].value, "400");
    assert_eq!(res.attributes[3].value, USDC_DENOM);
}

#[test]
fn withdraw_unauthorized() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    setup_contract(&mut deps, "123");

    // Try withdraw from unauthorized address
    let info = MessageInfo {
        sender: deps.api.addr_make("hacker"),
        funds: vec![],
    };
    let coin = Coin {
        denom: USDC_DENOM.to_string(),
        amount: Uint128::new(100),
    };

    let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw { coin });
    let err = execute(deps.as_mut(), env.clone(), info, withdraw_msg).unwrap_err();
    assert_eq!(err, ContractError::Unauthorized {});
}

#[test]
fn withdraw_unregistered_depositor() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    setup_contract(&mut deps, "123");

    // Try withdraw from completely unregistered depositor address
    let unregistered = deps.api.addr_make("unregistered_depositor");
    let info = MessageInfo {
        sender: unregistered,
        funds: vec![],
    };
    let coin = Coin {
        denom: USDC_DENOM.to_string(),
        amount: Uint128::new(100),
    };

    let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw { coin });
    let err = execute(deps.as_mut(), env.clone(), info, withdraw_msg).unwrap_err();
    assert_eq!(err, ContractError::Unauthorized {});
}

#[test]
fn withdraw_unsupported_denom() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Try to withdraw unsupported denom
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![],
    };
    let unsupported_denom = "uatom".to_string();
    let coin = Coin {
        denom: unsupported_denom.clone(),
        amount: Uint128::new(100),
    };

    let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw { coin });
    let err = execute(deps.as_mut(), env, info, withdraw_msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::UnsupportedDenom {
            denom: unsupported_denom
        }
    );
}

#[test]
fn withdraw_insufficient_balance() {
    // Mock Mars positions: depositor has 500 USDC lent
    let mars_account_id = "666";
    let positions = vec![(
        mars_account_id.to_string(),
        PositionsResponse {
            account_id: mars_account_id.to_string(),
            deposits: vec![],
            debts: vec![],
            lends: vec![Coin {
                denom: USDC_DENOM.to_string(),
                amount: Uint128::new(500),
            }],
        },
    )];
    let mut deps = mock_dependencies_with_mars_positions(positions);
    let env = mock_env();
    let test_data = setup_contract(&mut deps, mars_account_id);

    // Try to withdraw 1000 (more than lent)
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![],
    };
    let coin = Coin {
        denom: USDC_DENOM.to_string(),
        amount: Uint128::new(1000),
    };

    let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw { coin });
    let err = execute(deps.as_mut(), env, info, withdraw_msg).unwrap_err();
    assert_eq!(err, ContractError::InsufficientBalance {});
}

#[test]
fn withdraw_zero_amount() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Try to withdraw zero amount
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![],
    };
    let coin = Coin {
        denom: USDC_DENOM.to_string(),
        amount: Uint128::zero(),
    };

    let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw { coin });
    let err = execute(deps.as_mut(), env, info, withdraw_msg).unwrap_err();
    assert_eq!(err, ContractError::ZeroAmount {});
}

#[test]
fn withdraw_full_amount() {
    // Mock Mars positions: depositor has 1000 USDC lent
    let mars_account_id = "555";
    let positions = vec![(
        mars_account_id.to_string(),
        PositionsResponse {
            account_id: mars_account_id.to_string(),
            deposits: vec![],
            debts: vec![],
            lends: vec![Coin {
                denom: USDC_DENOM.to_string(),
                amount: Uint128::new(1000),
            }],
        },
    )];
    let mut deps = mock_dependencies_with_mars_positions(positions);
    let env = mock_env();
    let test_data = setup_contract(&mut deps, mars_account_id);

    // Withdraw full amount
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![],
    };
    let coin = Coin {
        denom: USDC_DENOM.to_string(),
        amount: Uint128::new(1000),
    };
    let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw { coin });
    let res = execute(deps.as_mut(), env, info, withdraw_msg).unwrap();

    // Should have Mars message
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.attributes[0].value, "withdraw");
}

// Security Tests - Depositor Isolation
#[test]
fn depositor_cannot_withdraw_another_depositors_funds() {
    // Mock Mars positions: depositor1 has 1000, depositor2 has 500
    let positions = vec![
        (
            "100".to_string(),
            PositionsResponse {
                account_id: "100".to_string(),
                deposits: vec![],
                debts: vec![],
                lends: vec![Coin {
                    denom: USDC_DENOM.to_string(),
                    amount: Uint128::new(1000),
                }],
            },
        ),
        (
            "200".to_string(),
            PositionsResponse {
                account_id: "200".to_string(),
                deposits: vec![],
                debts: vec![],
                lends: vec![Coin {
                    denom: USDC_DENOM.to_string(),
                    amount: Uint128::new(500),
                }],
            },
        ),
    ];
    let mut deps = mock_dependencies_with_mars_positions(positions);
    let env = mock_env();

    // Setup first depositor with account ID "100"
    let test_data = setup_contract(&mut deps, "100");

    // Register second depositor manually with account ID "200"
    let depositor2_addr = deps.api.addr_make("depositor2");
    register_depositor_with_account(&mut deps, depositor2_addr.clone(), "200".to_string());

    // Try to withdraw from depositor2's perspective (which only has 500 USDC)
    // This tests that depositor2 can only access its own funds
    let info = MessageInfo {
        sender: depositor2_addr.clone(),
        funds: vec![],
    };
    let coin = Coin {
        denom: USDC_DENOM.to_string(),
        amount: Uint128::new(600), // More than depositor2 has
    };

    // This should fail with InsufficientBalance
    let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw { coin });

    let err = execute(deps.as_mut(), env.clone(), info, withdraw_msg.clone()).unwrap_err();
    assert_eq!(err, ContractError::InsufficientBalance {});

    // Verify depositor1 cannot access depositor2's Mars account
    // Depositor1 tries to withdraw, but it uses its own account (100), not account 200
    let info = MessageInfo {
        sender: test_data.depositor_address,
        funds: vec![],
    };
    let coin = Coin {
        denom: USDC_DENOM.to_string(),
        amount: Uint128::new(900), // Within depositor1's balance
    };

    // This should succeed because depositor1 has 1000 USDC in its own account
    let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw { coin });

    let res = execute(deps.as_mut(), env, info, withdraw_msg).unwrap();
    assert_eq!(res.messages.len(), 1);
}

#[test]
fn deposit_and_withdraw_isolation_between_depositors() {
    // Mock Mars positions: depositor1 has 1000, depositor2 has 500
    let positions = vec![
        (
            "300".to_string(),
            PositionsResponse {
                account_id: "300".to_string(),
                deposits: vec![],
                debts: vec![],
                lends: vec![Coin {
                    denom: USDC_DENOM.to_string(),
                    amount: Uint128::new(1000),
                }],
            },
        ),
        (
            "400".to_string(),
            PositionsResponse {
                account_id: "400".to_string(),
                deposits: vec![],
                debts: vec![],
                lends: vec![Coin {
                    denom: USDC_DENOM.to_string(),
                    amount: Uint128::new(500),
                }],
            },
        ),
    ];
    let mut deps = mock_dependencies_with_mars_positions(positions);
    let env = mock_env();

    // Setup first depositor with account ID "300"
    let test_data = setup_contract(&mut deps, "300");

    // Register second depositor manually with account ID "400"
    let depositor2_addr = deps.api.addr_make("depositor2_vault");
    register_depositor_with_account(&mut deps, depositor2_addr.clone(), "400".to_string());

    // Depositor1 withdraws 300 from its own account (has 1000)
    let info = MessageInfo {
        sender: test_data.depositor_address.clone(),
        funds: vec![],
    };
    let coin = Coin {
        denom: USDC_DENOM.to_string(),
        amount: Uint128::new(300),
    };
    let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw { coin });
    let res = execute(deps.as_mut(), env.clone(), info, withdraw_msg.clone()).unwrap();
    assert_eq!(res.messages.len(), 1);

    // Depositor2 withdraws 400 from its own account (has 500)
    let info = MessageInfo {
        sender: depositor2_addr,
        funds: vec![],
    };
    let coin = Coin {
        denom: USDC_DENOM.to_string(),
        amount: Uint128::new(400),
    };
    let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw { coin });
    let res = execute(deps.as_mut(), env.clone(), info, withdraw_msg).unwrap();
    assert_eq!(res.messages.len(), 1);

    // Both withdrawals succeed because they're isolated to their own Mars accounts
}

// ============================================================================
// Query Tests
// ============================================================================

#[test]
fn query_config_works() {
    let mut deps = mock_dependencies();
    let test_data = setup_contract(&mut deps, "123");

    // Query config
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::Config {}),
    )
    .unwrap();
    let config: MarsConfigResponse = from_json(&res).unwrap();

    assert_eq!(
        config.mars_credit_manager,
        test_data.mars_credit_manager.to_string()
    );
    assert_eq!(config.mars_params, test_data.mars_params.to_string());
    assert_eq!(config.mars_red_bank, test_data.mars_red_bank.to_string());
    assert_eq!(config.supported_denoms, vec![USDC_DENOM.to_string()]);
}

#[test]
fn query_registered_depositors_returns_all() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Register a second depositor
    let depositor2_addr = deps.api.addr_make("depositor2");
    register_depositor_with_account(&mut deps, depositor2_addr.clone(), "456".to_string());

    // Query all registered depositors
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::RegisteredDepositors { enabled: None }),
    )
    .unwrap();
    let response: RegisteredDepositorsResponse = from_json(&res).unwrap();

    assert_eq!(response.depositors.len(), 2);
    assert!(response
        .depositors
        .iter()
        .any(|i| i.depositor_address == test_data.depositor_address.to_string() && i.enabled));
    assert!(response
        .depositors
        .iter()
        .any(|i| i.depositor_address == depositor2_addr.to_string() && i.enabled));
}

#[test]
fn query_registered_depositors_filter_enabled() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Register a second depositor
    let depositor2_addr = deps.api.addr_make("depositor2");
    register_depositor_with_account(&mut deps, depositor2_addr.clone(), "456".to_string());

    // Disable the first depositor
    let depositor = Depositor {
        mars_account_id: "123".to_string(),
        enabled: false,
    };
    WHITELISTED_DEPOSITORS
        .save(
            &mut deps.storage,
            test_data.depositor_address.clone(),
            &depositor,
        )
        .unwrap();

    // Query only enabled depositors
    let res = query(
        deps.as_ref(),
        env.clone(),
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::RegisteredDepositors {
            enabled: Some(true),
        }),
    )
    .unwrap();
    let response: RegisteredDepositorsResponse = from_json(&res).unwrap();

    assert_eq!(response.depositors.len(), 1);
    assert_eq!(
        response.depositors[0].depositor_address,
        depositor2_addr.to_string()
    );
    assert!(response.depositors[0].enabled);

    // Query only disabled depositors
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::RegisteredDepositors {
            enabled: Some(false),
        }),
    )
    .unwrap();
    let response: RegisteredDepositorsResponse = from_json(&res).unwrap();

    assert_eq!(response.depositors.len(), 1);
    assert_eq!(
        response.depositors[0].depositor_address,
        test_data.depositor_address.to_string()
    );
    assert!(!response.depositors[0].enabled);
}

#[test]
fn query_time_to_withdraw_is_instant() {
    // Mock Mars positions: depositor has 500 USDC lent
    let mars_account_id = "123";
    let positions = vec![(
        mars_account_id.to_string(),
        PositionsResponse {
            account_id: mars_account_id.to_string(),
            deposits: vec![],
            debts: vec![],
            lends: vec![Coin {
                denom: USDC_DENOM.to_string(),
                amount: Uint128::new(500),
            }],
        },
    )];
    let mut deps = mock_dependencies_with_mars_positions(positions);

    let env = mock_env();
    let test_data = setup_contract(&mut deps, mars_account_id);

    // Mock Red Bank balance - has plenty of liquidity (more than depositor's 500)
    deps.querier.bank.update_balance(
        test_data.mars_red_bank.as_str(),
        vec![Coin {
            denom: USDC_DENOM.to_string(),
            amount: Uint128::new(10_000),
        }],
    );

    // Try to withdraw 100 USDC (less than the 500 available)
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::TimeToWithdraw {
            depositor_address: test_data.depositor_address.to_string(),
            coin: Coin {
                denom: USDC_DENOM.to_string(),
                amount: Uint128::new(100),
            },
        }),
    )
    .unwrap();
    let response: TimeEstimateResponse = from_json(&res).unwrap();

    // Should be instant (0 seconds/blocks)
    assert_eq!(response.blocks, 0);
    assert_eq!(response.seconds, 0);
}

#[test]
fn query_time_to_withdraw_returns_one_week_when_liquidity_constrained() {
    // Mock Mars positions: depositor has 1000 USDC lent
    let mars_account_id = "999";
    let positions = vec![(
        mars_account_id.to_string(),
        PositionsResponse {
            account_id: mars_account_id.to_string(),
            deposits: vec![],
            debts: vec![],
            lends: vec![Coin {
                denom: USDC_DENOM.to_string(),
                amount: Uint128::new(1000),
            }],
        },
    )];
    let mut deps = mock_dependencies_with_mars_positions(positions);

    let env = mock_env();
    let test_data = setup_contract(&mut deps, mars_account_id);

    // Mock Red Bank balance - only has 300 USDC available (less than depositor's 1000)
    deps.querier.bank.update_balance(
        test_data.mars_red_bank.as_str(),
        vec![Coin {
            denom: USDC_DENOM.to_string(),
            amount: Uint128::new(300),
        }],
    );

    // Try to withdraw 500 USDC (more than the 300 available in Red Bank)
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::TimeToWithdraw {
            depositor_address: test_data.depositor_address.to_string(),
            coin: Coin {
                denom: USDC_DENOM.to_string(),
                amount: Uint128::new(500),
            },
        }),
    )
    .unwrap();
    let response: TimeEstimateResponse = from_json(&res).unwrap();

    // Should return 1 week estimate (604,800 seconds/blocks)
    const ONE_WEEK: u64 = 604_800;
    assert_eq!(response.blocks, ONE_WEEK);
    assert_eq!(response.seconds, ONE_WEEK);
}

#[test]
fn query_all_positions_aggregates_from_all_depositors() {
    const ATOM_DENOM: &str = "ibc/atom";

    // Mock Mars positions for three different depositors
    let positions = vec![
        // Depositor 1 (account_id: "123") has 1000 USDC
        (
            "123".to_string(),
            PositionsResponse {
                account_id: "123".to_string(),
                deposits: vec![],
                debts: vec![],
                lends: vec![Coin {
                    denom: USDC_DENOM.to_string(),
                    amount: Uint128::new(1000),
                }],
            },
        ),
        // Depositor 2 (account_id: "456") has 500 USDC and 250 ATOM
        (
            "456".to_string(),
            PositionsResponse {
                account_id: "456".to_string(),
                deposits: vec![],
                debts: vec![],
                lends: vec![
                    Coin {
                        denom: USDC_DENOM.to_string(),
                        amount: Uint128::new(500),
                    },
                    Coin {
                        denom: ATOM_DENOM.to_string(),
                        amount: Uint128::new(250),
                    },
                ],
            },
        ),
        // Depositor 3 (account_id: "789") has 300 ATOM
        (
            "789".to_string(),
            PositionsResponse {
                account_id: "789".to_string(),
                deposits: vec![],
                debts: vec![],
                lends: vec![Coin {
                    denom: ATOM_DENOM.to_string(),
                    amount: Uint128::new(300),
                }],
            },
        ),
    ];

    let mut deps = mock_dependencies_with_mars_positions(positions);
    let env = mock_env();
    setup_contract(&mut deps, "123");

    // Register two additional depositors
    let depositor2 = deps.api.addr_make("depositor2");
    register_depositor_with_account(&mut deps, depositor2.clone(), "456".to_string());

    let depositor3 = deps.api.addr_make("depositor3");
    register_depositor_with_account(&mut deps, depositor3.clone(), "789".to_string());

    // Query all positions - should aggregate across all depositors
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::AllPositions {}),
    )
    .unwrap();
    let response: AllPositionsResponse = from_json(&res).unwrap();

    // Should have positions for both USDC and ATOM
    assert_eq!(response.positions.len(), 2);

    // Find USDC position (1000 + 500 = 1500)
    let usdc_position = response
        .positions
        .iter()
        .find(|c| c.denom == USDC_DENOM)
        .unwrap();
    assert_eq!(usdc_position.amount, Uint128::new(1500));

    // Find ATOM position (250 + 300 = 550)
    let atom_position = response
        .positions
        .iter()
        .find(|c| c.denom == ATOM_DENOM)
        .unwrap();
    assert_eq!(atom_position.amount, Uint128::new(550));
}

#[test]
fn query_available_for_withdraw_returns_mars_position() {
    // Mock Mars positions: depositor has 750 USDC lent
    let positions = vec![(
        "789".to_string(),
        PositionsResponse {
            account_id: "789".to_string(),
            deposits: vec![],
            debts: vec![],
            lends: vec![Coin {
                denom: USDC_DENOM.to_string(),
                amount: Uint128::new(750),
            }],
        },
    )];
    let mut deps = mock_dependencies_with_mars_positions(positions);

    let env = mock_env();
    let test_data = setup_contract(&mut deps, "789");

    // Mock Red Bank balance - has plenty of liquidity (more than depositor position)
    deps.querier.bank.update_balance(
        test_data.mars_red_bank.as_str(),
        vec![Coin {
            denom: USDC_DENOM.to_string(),
            amount: Uint128::new(100_000_000),
        }],
    );

    // Query available for withdraw - should return depositor position (750) since Red Bank has sufficient liquidity
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::AvailableForWithdraw {
            depositor_address: test_data.depositor_address.to_string(),
            denom: USDC_DENOM.to_string(),
        }),
    )
    .unwrap();
    let response: AvailableAmountResponse = from_json(&res).unwrap();

    assert_eq!(response.amount, Uint128::new(750));
}

#[test]
fn query_available_for_withdraw_limited_by_red_bank_liquidity() {
    // Mock Mars positions: depositor has 1000 USDC lent
    let positions = vec![(
        "888".to_string(),
        PositionsResponse {
            account_id: "888".to_string(),
            deposits: vec![],
            debts: vec![],
            lends: vec![Coin {
                denom: USDC_DENOM.to_string(),
                amount: Uint128::new(1000),
            }],
        },
    )];
    let mut deps = mock_dependencies_with_mars_positions(positions);

    let env = mock_env();
    let test_data = setup_contract(&mut deps, "888");

    // Mock Red Bank balance - only has 300 USDC available (less than depositor's 1000)
    deps.querier.bank.update_balance(
        test_data.mars_red_bank.as_str(),
        vec![Coin {
            denom: USDC_DENOM.to_string(),
            amount: Uint128::new(300),
        }],
    );

    // Query available for withdraw - should return Red Bank liquidity (300) since it's less than position (1000)
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::AvailableForWithdraw {
            depositor_address: test_data.depositor_address.to_string(),
            denom: USDC_DENOM.to_string(),
        }),
    )
    .unwrap();
    let response: AvailableAmountResponse = from_json(&res).unwrap();

    // Should return min(1000, 300) = 300
    assert_eq!(response.amount, Uint128::new(300));
}

#[test]
fn query_depositor_positions_multiple_denoms() {
    // Mock Mars positions: depositor has multiple denoms lent
    let atom_denom = "uatom";
    let positions = vec![(
        "999".to_string(),
        PositionsResponse {
            account_id: "999".to_string(),
            deposits: vec![],
            debts: vec![],
            lends: vec![
                Coin {
                    denom: USDC_DENOM.to_string(),
                    amount: Uint128::new(500),
                },
                Coin {
                    denom: atom_denom.to_string(),
                    amount: Uint128::new(300),
                },
            ],
        },
    )];
    let mut deps = mock_dependencies_with_mars_positions(positions);

    // Instantiate contract with multiple denoms
    let env = mock_env();
    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };

    let admin = deps.api.addr_make("admin");
    let mars_credit_manager = deps.api.addr_make("mars_credit_manager");
    let mars_params = deps.api.addr_make("mars_params");
    let mars_red_bank = deps.api.addr_make("mars_red_bank");
    let depositor_address = deps.api.addr_make("depositor");

    let msg = InstantiateMsg {
        admins: vec![admin.to_string()],
        mars_credit_manager: mars_credit_manager.to_string(),
        mars_params: mars_params.to_string(),
        mars_red_bank: mars_red_bank.to_string(),
        supported_denoms: vec![USDC_DENOM.to_string(), atom_denom.to_string()],
        initial_depositors: vec![depositor_address.to_string()],
    };
    instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Simulate the reply from Mars to set up the account
    let mock_reply = create_mock_mars_account_reply("999");
    reply(deps.as_mut(), env.clone(), mock_reply).unwrap();

    // Query depositor positions
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::DepositorPositions {
            depositor_address: depositor_address.to_string(),
        }),
    )
    .unwrap();
    let response: DepositorPositionsResponse = from_json(&res).unwrap();

    assert_eq!(response.positions.len(), 2);
    assert!(response
        .positions
        .iter()
        .any(|c| c.denom == USDC_DENOM && c.amount == Uint128::new(500)));
    assert!(response
        .positions
        .iter()
        .any(|c| c.denom == atom_denom && c.amount == Uint128::new(300)));
}

// ============================================================================
// Admin Operations Tests
// ============================================================================

#[test]
fn admin_can_register_new_depositor() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "111");

    // Admin registers a second depositor
    let depositor2 = deps.api.addr_make("depositor2");
    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RegisterDepositor {
        depositor_address: depositor2.to_string(),
        metadata: None,
    });

    let info = MessageInfo {
        sender: test_data.admin.clone(),
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Should have submessage for Mars account creation
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.attributes[0].value, "register_depositor");
    assert_eq!(res.attributes[1].value, depositor2.to_string());

    // Simulate reply to complete registration
    let mock_reply2 = create_mock_mars_account_reply("222");
    reply(deps.as_mut(), env.clone(), mock_reply2).unwrap();

    // Verify second depositor is registered using query
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::RegisteredDepositors { enabled: None }),
    )
    .unwrap();
    let response: RegisteredDepositorsResponse = from_json(&res).unwrap();

    assert_eq!(response.depositors.len(), 2);
    assert!(response
        .depositors
        .iter()
        .any(|i| i.depositor_address == depositor2.to_string() && i.enabled));
}

#[test]
fn non_admin_cannot_register_depositor() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    setup_contract(&mut deps, "111");

    // Non-admin tries to register depositor
    let depositor2 = deps.api.addr_make("depositor2");
    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RegisterDepositor {
        depositor_address: depositor2.to_string(),
        metadata: None,
    });

    let info = MessageInfo {
        sender: deps.api.addr_make("hacker"),
        funds: vec![],
    };
    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();

    assert_eq!(err, ContractError::UnauthorizedAdmin {});
}

#[test]
fn register_depositor_already_registered_fails() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Try to register the same depositor again
    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RegisterDepositor {
        depositor_address: test_data.depositor_address.to_string(),
        metadata: None,
    });

    let info = MessageInfo {
        sender: test_data.admin,
        funds: vec![],
    };
    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();

    assert_eq!(
        err,
        ContractError::DepositorAlreadyRegistered {
            depositor_address: test_data.depositor_address.to_string()
        }
    );
}

#[test]
fn admin_can_unregister_depositor() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Verify depositor is registered
    assert!(WHITELISTED_DEPOSITORS
        .may_load(&deps.storage, test_data.depositor_address.clone())
        .unwrap()
        .is_some());

    // Unregister the depositor
    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::UnregisterDepositor {
        depositor_address: test_data.depositor_address.to_string(),
    });

    let info = MessageInfo {
        sender: test_data.admin,
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env, info, msg).unwrap();

    assert_eq!(res.attributes[0].value, "unregister_depositor");
    assert_eq!(
        res.attributes[1].value,
        test_data.depositor_address.to_string()
    );
    assert_eq!(res.attributes[2].value, "123");

    // Verify depositor is no longer registered
    assert!(WHITELISTED_DEPOSITORS
        .may_load(&deps.storage, test_data.depositor_address)
        .unwrap()
        .is_none());
}

#[test]
fn non_admin_cannot_unregister_depositor() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Non-admin tries to unregister
    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::UnregisterDepositor {
        depositor_address: test_data.depositor_address.to_string(),
    });

    let info = MessageInfo {
        sender: deps.api.addr_make("hacker"),
        funds: vec![],
    };
    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();

    assert_eq!(err, ContractError::UnauthorizedAdmin {});
}

#[test]
fn unregister_nonexistent_depositor_fails() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Try to unregister non-existent depositor
    let nonexistent = deps.api.addr_make("nonexistent");
    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::UnregisterDepositor {
        depositor_address: nonexistent.to_string(),
    });

    let info = MessageInfo {
        sender: test_data.admin,
        funds: vec![],
    };
    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();

    assert_eq!(
        err,
        ContractError::DepositorNotRegistered {
            depositor_address: nonexistent.to_string()
        }
    );
}

#[test]
fn admin_can_toggle_depositor_enabled() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Verify depositor is enabled by default
    let depositor = WHITELISTED_DEPOSITORS
        .load(&deps.storage, test_data.depositor_address.clone())
        .unwrap();
    assert!(depositor.enabled);

    // Disable the depositor
    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
        depositor_address: test_data.depositor_address.to_string(),
        enabled: false,
    });

    let info = MessageInfo {
        sender: test_data.admin.clone(),
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    assert_eq!(res.attributes[0].value, "toggle_depositor_enabled");
    assert_eq!(
        res.attributes[1].value,
        test_data.depositor_address.to_string()
    );
    assert_eq!(res.attributes[2].value, "false");

    // Verify depositor is disabled
    let depositor = WHITELISTED_DEPOSITORS
        .load(&deps.storage, test_data.depositor_address.clone())
        .unwrap();
    assert!(!depositor.enabled);

    // Re-enable the depositor
    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
        depositor_address: test_data.depositor_address.to_string(),
        enabled: true,
    });

    let info = MessageInfo {
        sender: test_data.admin,
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env, info, msg).unwrap();

    assert_eq!(res.attributes[2].value, "true");

    // Verify depositor is enabled again
    let depositor = WHITELISTED_DEPOSITORS
        .load(&deps.storage, test_data.depositor_address)
        .unwrap();
    assert!(depositor.enabled);
}

#[test]
fn non_admin_cannot_toggle_depositor_enabled() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Non-admin tries to toggle
    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
        depositor_address: test_data.depositor_address.to_string(),
        enabled: false,
    });

    let info = MessageInfo {
        sender: deps.api.addr_make("hacker"),
        funds: vec![],
    };
    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();

    assert_eq!(err, ContractError::UnauthorizedAdmin {});
}

#[test]
fn toggle_nonexistent_depositor_fails() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Try to toggle non-existent depositor
    let nonexistent = deps.api.addr_make("nonexistent");
    let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
        depositor_address: nonexistent.to_string(),
        enabled: false,
    });

    let info = MessageInfo {
        sender: test_data.admin,
        funds: vec![],
    };
    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();

    assert_eq!(
        err,
        ContractError::DepositorNotRegistered {
            depositor_address: nonexistent.to_string()
        }
    );
}

// ============================================================================
// UpdateConfig Tests
// ============================================================================

#[test]
fn update_config_protocol_address() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Update protocol addresses (admin only)
    let new_mars_credit_manager = deps.api.addr_make("new_mars_credit_manager");
    let new_mars_params = deps.api.addr_make("new_mars_params");
    let new_mars_red_bank = deps.api.addr_make("new_mars_red_bank");
    let msg = ExecuteMsg::CustomAction(MarsAdapterMsg::UpdateConfig {
        mars_credit_manager: Some(new_mars_credit_manager.to_string()),
        mars_params: Some(new_mars_params.to_string()),
        mars_red_bank: Some(new_mars_red_bank.to_string()),
        supported_denoms: None,
    });

    let info = MessageInfo {
        sender: test_data.admin,
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env, info, msg).unwrap();

    assert_eq!(res.attributes[0].value, "update_config");
    assert_eq!(res.attributes[1].value, new_mars_credit_manager.to_string());
    assert_eq!(res.attributes[2].value, new_mars_params.to_string());
    assert_eq!(res.attributes[3].value, new_mars_red_bank.to_string());

    // Verify config was updated
    let config = CONFIG.load(&deps.storage).unwrap();
    assert_eq!(config.mars_credit_manager, new_mars_credit_manager);
    assert_eq!(config.mars_params, new_mars_params);
    assert_eq!(config.mars_red_bank, new_mars_red_bank);
}

#[test]
fn update_config_supported_denoms() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Update supported denoms (admin only) - only update this parameter
    let new_denoms = vec!["uatom".to_string(), "uosmo".to_string()];
    let msg = ExecuteMsg::CustomAction(MarsAdapterMsg::UpdateConfig {
        mars_credit_manager: None,
        mars_params: None,
        mars_red_bank: None,
        supported_denoms: Some(new_denoms.clone()),
    });

    let info = MessageInfo {
        sender: test_data.admin,
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env, info, msg).unwrap();

    assert_eq!(res.attributes[0].value, "update_config");

    // Verify config was updated
    let config = CONFIG.load(&deps.storage).unwrap();
    assert_eq!(config.supported_denoms, new_denoms);
}

#[test]
fn update_config_both_parameters() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Update all parameters (admin only) - can be done in a single call
    let new_mars_credit_manager = deps.api.addr_make("new_mars_credit_manager");
    let new_mars_params = deps.api.addr_make("new_mars_params");
    let new_mars_red_bank = deps.api.addr_make("new_mars_red_bank");
    let new_denoms = vec!["uatom".to_string()];

    // Update all in one call
    let msg = ExecuteMsg::CustomAction(MarsAdapterMsg::UpdateConfig {
        mars_credit_manager: Some(new_mars_credit_manager.to_string()),
        mars_params: Some(new_mars_params.to_string()),
        mars_red_bank: Some(new_mars_red_bank.to_string()),
        supported_denoms: Some(new_denoms.clone()),
    });
    let info = MessageInfo {
        sender: test_data.admin,
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(res.attributes[0].value, "update_config");
    // action + 3 Mars addresses
    assert_eq!(res.attributes.len(), 4);

    // Verify all were updated
    let config = CONFIG.load(&deps.storage).unwrap();
    assert_eq!(config.mars_credit_manager, new_mars_credit_manager);
    assert_eq!(config.mars_params, new_mars_params);
    assert_eq!(config.mars_red_bank, new_mars_red_bank);
    assert_eq!(config.supported_denoms, new_denoms);
}

#[test]
fn update_config_unauthorized() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    setup_contract(&mut deps, "123");

    // Unauthorized tries to update config
    let msg = ExecuteMsg::CustomAction(MarsAdapterMsg::UpdateConfig {
        mars_credit_manager: Some("new_mars".to_string()),
        mars_params: None,
        mars_red_bank: None,
        supported_denoms: None,
    });

    let info = MessageInfo {
        sender: deps.api.addr_make("hacker"),
        funds: vec![],
    };
    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();

    assert_eq!(err, ContractError::UnauthorizedAdmin {});
}

// ============================================================================
// Additional Query Tests
// ============================================================================

#[test]
fn query_available_for_deposit_with_capacity_available() {
    // Mock Mars Params: cap is 4,000,000, current amount is 2,500,000
    // Available should be 1,500,000
    let total_deposits = vec![(
        USDC_DENOM.to_string(),
        TotalDepositResponse {
            denom: USDC_DENOM.to_string(),
            cap: Uint128::new(4_000_000),
            amount: Uint128::new(2_500_000),
        },
    )];
    let mut deps = mock_dependencies_with_mars_params(total_deposits);
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Query available for deposit
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::AvailableForDeposit {
            depositor_address: test_data.depositor_address.to_string(),
            denom: USDC_DENOM.to_string(),
        }),
    )
    .unwrap();
    let response: AvailableAmountResponse = from_json(&res).unwrap();

    // Should return cap - amount = 4,000,000 - 2,500,000 = 1,500,000
    assert_eq!(response.amount, Uint128::new(1_500_000));
}

#[test]
fn query_available_for_deposit_when_at_cap() {
    // Mock Mars Params: cap is 1,000,000, current amount is also 1,000,000
    // Available should be 0
    let total_deposits = vec![(
        USDC_DENOM.to_string(),
        TotalDepositResponse {
            denom: USDC_DENOM.to_string(),
            cap: Uint128::new(1_000_000),
            amount: Uint128::new(1_000_000),
        },
    )];
    let mut deps = mock_dependencies_with_mars_params(total_deposits);
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Query available for deposit
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::AvailableForDeposit {
            depositor_address: test_data.depositor_address.to_string(),
            denom: USDC_DENOM.to_string(),
        }),
    )
    .unwrap();
    let response: AvailableAmountResponse = from_json(&res).unwrap();

    // Should return 0 (cap reached)
    assert_eq!(response.amount, Uint128::zero());
}

#[test]
fn query_available_for_deposit_saturating_sub_protection() {
    // Mock Mars Params: amount somehow exceeds cap (shouldn't happen in practice)
    // This tests the saturating_sub protection
    let total_deposits = vec![(
        USDC_DENOM.to_string(),
        TotalDepositResponse {
            denom: USDC_DENOM.to_string(),
            cap: Uint128::new(1_000_000),
            amount: Uint128::new(1_500_000), // Exceeds cap!
        },
    )];
    let mut deps = mock_dependencies_with_mars_params(total_deposits);
    let env = mock_env();
    let test_data = setup_contract(&mut deps, "123");

    // Query available for deposit
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::AvailableForDeposit {
            depositor_address: test_data.depositor_address.to_string(),
            denom: USDC_DENOM.to_string(),
        }),
    )
    .unwrap();
    let response: AvailableAmountResponse = from_json(&res).unwrap();

    // Should return 0 (saturating_sub prevents underflow)
    assert_eq!(response.amount, Uint128::zero());
}

// ============================================================================
// Instantiation Error Tests
// ============================================================================

#[test]
fn instantiate_with_empty_admins_fails() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let mars_credit_manager = deps.api.addr_make("mars_credit_manager");
    let mars_params = deps.api.addr_make("mars_params");
    let mars_red_bank = deps.api.addr_make("mars_red_bank");
    let depositor_address = deps.api.addr_make("depositor");

    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };

    let msg = InstantiateMsg {
        admins: vec![],
        mars_credit_manager: mars_credit_manager.to_string(),
        mars_params: mars_params.to_string(),
        mars_red_bank: mars_red_bank.to_string(),
        supported_denoms: vec![USDC_DENOM.to_string()],
        initial_depositors: vec![depositor_address.to_string()],
    };

    let err = instantiate(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(err, ContractError::AtLeastOneAdmin {});
}

#[test]
fn instantiate_with_duplicate_admins_deduplicates() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let admin = deps.api.addr_make("admin");
    let mars_credit_manager = deps.api.addr_make("mars_credit_manager");
    let mars_params = deps.api.addr_make("mars_params");
    let mars_red_bank = deps.api.addr_make("mars_red_bank");
    let depositor_address = deps.api.addr_make("depositor");

    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };

    // Provide same admin twice
    let msg = InstantiateMsg {
        admins: vec![admin.to_string(), admin.to_string()],
        mars_credit_manager: mars_credit_manager.to_string(),
        mars_params: mars_params.to_string(),
        mars_red_bank: mars_red_bank.to_string(),
        supported_denoms: vec![USDC_DENOM.to_string()],
        initial_depositors: vec![depositor_address.to_string()],
    };

    instantiate(deps.as_mut(), env, info, msg).unwrap();

    // Verify only one admin is stored
    let admins = ADMINS.load(&deps.storage).unwrap();
    assert_eq!(admins.len(), 1);
    assert_eq!(admins[0], admin);
}

// ============================================================================
// Reply Handler Error Tests
// ============================================================================

#[test]
fn reply_handler_with_error_result_fails() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = default_instantiate_msg(&mut deps);

    // Instantiate
    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };
    instantiate(deps.as_mut(), env.clone(), info, test_data.msg).unwrap();

    // Create a reply with error result
    let error_reply = Reply {
        id: REPLY_CREATE_ACCOUNT,
        result: SubMsgResult::Err("Mars account creation failed".to_string()),
        payload: cosmwasm_std::Binary::default(),
        gas_used: 0,
    };

    let err = reply(deps.as_mut(), env, error_reply).unwrap_err();
    assert!(matches!(err, ContractError::MarsProtocolError { .. }));
}

#[test]
fn reply_handler_without_token_id_fails() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = default_instantiate_msg(&mut deps);

    // Instantiate
    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };
    instantiate(deps.as_mut(), env.clone(), info, test_data.msg).unwrap();

    // Create a reply without token_id attribute (or without mint action)
    #[allow(deprecated)]
    let invalid_reply = Reply {
        id: REPLY_CREATE_ACCOUNT,
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![Event::new("wasm").add_attribute("wrong_key", "123")],
            msg_responses: vec![],
            data: None,
        }),
        payload: cosmwasm_std::Binary::default(),
        gas_used: 0,
    };

    let err = reply(deps.as_mut(), env, invalid_reply).unwrap_err();
    assert!(matches!(err, ContractError::MarsProtocolError { .. }));
}

#[test]
fn reply_handler_unknown_reply_id_fails() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let test_data = default_instantiate_msg(&mut deps);

    // Instantiate
    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };
    instantiate(deps.as_mut(), env.clone(), info, test_data.msg).unwrap();

    // Create a reply with unknown ID
    #[allow(deprecated)]
    let unknown_reply = Reply {
        id: 999,
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![],
            msg_responses: vec![],
            data: None,
        }),
        payload: cosmwasm_std::Binary::default(),
        gas_used: 0,
    };

    let err = reply(deps.as_mut(), env, unknown_reply).unwrap_err();
    assert!(matches!(err, ContractError::Std(_)));
}

// ============================================================================
// DepositorPosition Query Tests (singular - specific denom)
// ============================================================================

#[test]
fn query_depositor_position_single_denom() {
    // Mock Mars positions: depositor has 1500 USDC lent
    let mars_account_id = "555";
    let positions = vec![(
        mars_account_id.to_string(),
        PositionsResponse {
            account_id: mars_account_id.to_string(),
            deposits: vec![],
            debts: vec![],
            lends: vec![Coin {
                denom: USDC_DENOM.to_string(),
                amount: Uint128::new(1500),
            }],
        },
    )];
    let mut deps = mock_dependencies_with_mars_positions(positions);
    let env = mock_env();
    let test_data = setup_contract(&mut deps, mars_account_id);

    // Query depositor position for USDC
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address: test_data.depositor_address.to_string(),
            denom: USDC_DENOM.to_string(),
        }),
    )
    .unwrap();
    let response: DepositorPositionResponse = from_json(&res).unwrap();

    assert_eq!(response.amount, Uint128::new(1500));
}

#[test]
fn query_depositor_position_zero_when_no_position() {
    // Mock Mars positions: depositor has no lends
    let mars_account_id = "777";
    let positions = vec![(
        mars_account_id.to_string(),
        PositionsResponse {
            account_id: mars_account_id.to_string(),
            deposits: vec![],
            debts: vec![],
            lends: vec![],
        },
    )];
    let mut deps = mock_dependencies_with_mars_positions(positions);
    let env = mock_env();
    let test_data = setup_contract(&mut deps, mars_account_id);

    // Query depositor position for USDC when there are no lends
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address: test_data.depositor_address.to_string(),
            denom: USDC_DENOM.to_string(),
        }),
    )
    .unwrap();
    let response: DepositorPositionResponse = from_json(&res).unwrap();

    assert_eq!(response.amount, Uint128::zero());
}

#[test]
fn query_depositor_position_different_denom_returns_zero() {
    // Mock Mars positions: depositor has 1000 USDC lent but we query for ATOM
    let mars_account_id = "888";
    let positions = vec![(
        mars_account_id.to_string(),
        PositionsResponse {
            account_id: mars_account_id.to_string(),
            deposits: vec![],
            debts: vec![],
            lends: vec![Coin {
                denom: USDC_DENOM.to_string(),
                amount: Uint128::new(1000),
            }],
        },
    )];
    let mut deps = mock_dependencies_with_mars_positions(positions);
    let env = mock_env();
    let test_data = setup_contract(&mut deps, mars_account_id);

    // Query for ATOM when only USDC is lent
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address: test_data.depositor_address.to_string(),
            denom: "uatom".to_string(),
        }),
    )
    .unwrap();
    let response: DepositorPositionResponse = from_json(&res).unwrap();

    assert_eq!(response.amount, Uint128::zero());
}

#[test]
fn query_depositor_position_multiple_denoms_returns_correct_one() {
    // Mock Mars positions: depositor has both USDC and ATOM lent
    let mars_account_id = "999";
    let atom_denom = "uatom";
    let positions = vec![(
        mars_account_id.to_string(),
        PositionsResponse {
            account_id: mars_account_id.to_string(),
            deposits: vec![],
            debts: vec![],
            lends: vec![
                Coin {
                    denom: USDC_DENOM.to_string(),
                    amount: Uint128::new(2000),
                },
                Coin {
                    denom: atom_denom.to_string(),
                    amount: Uint128::new(500),
                },
            ],
        },
    )];
    let mut deps = mock_dependencies_with_mars_positions(positions);

    // Instantiate contract with both denoms
    let env = mock_env();
    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };

    let admin = deps.api.addr_make("admin");
    let mars_credit_manager = deps.api.addr_make("mars_credit_manager");
    let mars_params = deps.api.addr_make("mars_params");
    let mars_red_bank = deps.api.addr_make("mars_red_bank");
    let depositor_address = deps.api.addr_make("depositor");

    let msg = InstantiateMsg {
        admins: vec![admin.to_string()],
        mars_credit_manager: mars_credit_manager.to_string(),
        mars_params: mars_params.to_string(),
        mars_red_bank: mars_red_bank.to_string(),
        supported_denoms: vec![USDC_DENOM.to_string(), atom_denom.to_string()],
        initial_depositors: vec![depositor_address.to_string()],
    };
    instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Simulate the reply from Mars to set up the account
    let mock_reply = create_mock_mars_account_reply(mars_account_id);
    reply(deps.as_mut(), env.clone(), mock_reply).unwrap();

    // Query USDC deposit
    let res_usdc = query(
        deps.as_ref(),
        env.clone(),
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address: depositor_address.to_string(),
            denom: USDC_DENOM.to_string(),
        }),
    )
    .unwrap();
    let response_usdc: DepositorPositionResponse = from_json(&res_usdc).unwrap();
    assert_eq!(response_usdc.amount, Uint128::new(2000));

    // Query ATOM deposit
    let res_atom = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address: depositor_address.to_string(),
            denom: atom_denom.to_string(),
        }),
    )
    .unwrap();
    let response_atom: DepositorPositionResponse = from_json(&res_atom).unwrap();
    assert_eq!(response_atom.amount, Uint128::new(500));
}

#[test]
fn query_depositor_position_unregistered_depositor_returns_error() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    setup_contract(&mut deps, "123");

    // Query for unregistered depositor
    let unregistered = deps.api.addr_make("unregistered_depositor");
    let err = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address: unregistered.to_string(),
            denom: USDC_DENOM.to_string(),
        }),
    )
    .unwrap_err();
    assert!(err
        .to_string()
        .contains(&format!("Depositor not registered: {}", unregistered)));
}

#[test]
fn query_depositor_position_isolation_between_depositors() {
    // Mock Mars positions: depositor1 has 1000 USDC, depositor2 has 300 USDC
    let positions = vec![
        (
            "account1".to_string(),
            PositionsResponse {
                account_id: "account1".to_string(),
                deposits: vec![],
                debts: vec![],
                lends: vec![Coin {
                    denom: USDC_DENOM.to_string(),
                    amount: Uint128::new(1000),
                }],
            },
        ),
        (
            "account2".to_string(),
            PositionsResponse {
                account_id: "account2".to_string(),
                deposits: vec![],
                debts: vec![],
                lends: vec![Coin {
                    denom: USDC_DENOM.to_string(),
                    amount: Uint128::new(300),
                }],
            },
        ),
    ];
    let mut deps = mock_dependencies_with_mars_positions(positions);
    let env = mock_env();

    // Setup first depositor with account "account1"
    let test_data = setup_contract(&mut deps, "account1");

    // Register second depositor manually with account "account2"
    let depositor2_addr = deps.api.addr_make("depositor2");
    register_depositor_with_account(&mut deps, depositor2_addr.clone(), "account2".to_string());

    // Query depositor1 position
    let res1 = query(
        deps.as_ref(),
        env.clone(),
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address: test_data.depositor_address.to_string(),
            denom: USDC_DENOM.to_string(),
        }),
    )
    .unwrap();
    let response1: DepositorPositionResponse = from_json(&res1).unwrap();
    assert_eq!(response1.amount, Uint128::new(1000));

    // Query depositor2 position
    let res2 = query(
        deps.as_ref(),
        env,
        QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address: depositor2_addr.to_string(),
            denom: USDC_DENOM.to_string(),
        }),
    )
    .unwrap();
    let response2: DepositorPositionResponse = from_json(&res2).unwrap();
    assert_eq!(response2.amount, Uint128::new(300));
}
