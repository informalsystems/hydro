#[cfg(test)]
mod standard_adapter_tests {
    use cosmwasm_std::testing::mock_env;
    use cosmwasm_std::{coins, from_json, to_json_binary, Coin, MessageInfo, Uint128};

    use crate::contract::{execute, instantiate, query};
    use crate::error::ContractError;
    use crate::msg::*;
    use crate::state::DepositorCapabilities;
    use crate::testing_mocks::{
        default_test_setup, mock_dependencies, setup_contract_with_defaults,
    };

    // ============================================================================
    // INSTANTIATE TESTS
    // ============================================================================

    #[test]
    fn test_instantiate_success() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = MessageInfo {
            sender: deps.api.addr_make("creator"),
            funds: vec![],
        };

        let msg = InstantiateMsg {
            admins: vec![deps.api.addr_make("admin1").to_string()],
            denom: "ibc/usdc".to_string(),
            noble_transfer_channel_id: "channel-0".to_string(),
            ibc_default_timeout_seconds: 600,
            initial_depositors: vec![],
            initial_chains: vec![],
            initial_executors: vec![],
        };

        let res = instantiate(deps.as_mut(), env, info, msg);
        assert!(res.is_ok(), "Instantiate failed: {:?}", res);
    }

    #[test]
    fn test_instantiate_no_admins() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = MessageInfo {
            sender: deps.api.addr_make("creator"),
            funds: vec![],
        };

        let msg = InstantiateMsg {
            admins: vec![],
            denom: "ibc/usdc".to_string(),
            noble_transfer_channel_id: "channel-0".to_string(),
            ibc_default_timeout_seconds: 600,
            initial_depositors: vec![],
            initial_chains: vec![],
            initial_executors: vec![],
        };

        let err = instantiate(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::AtLeastOneAdmin {});
    }

    #[test]
    fn test_instantiate_duplicate_admins_deduped() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = MessageInfo {
            sender: deps.api.addr_make("creator"),
            funds: vec![],
        };

        let admin_addr = deps.api.addr_make("admin1").to_string();

        let msg = InstantiateMsg {
            admins: vec![admin_addr.clone(), admin_addr.clone()],
            denom: "ibc/usdc".to_string(),
            noble_transfer_channel_id: "channel-0".to_string(),
            ibc_default_timeout_seconds: 600,
            initial_depositors: vec![],
            initial_chains: vec![],
            initial_executors: vec![],
        };

        let res = instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert!(res.messages.is_empty());

        // Query admins to verify deduplication
        let query_msg = QueryMsg::CustomQuery(CctpAdapterQueryMsg::Admins {});
        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let admins_response: AdminsResponse = from_json(&res).unwrap();
        assert_eq!(admins_response.admins.len(), 1);
    }

    #[test]
    fn test_instantiate_with_depositors() {
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
            initial_depositors: vec![
                InitialDepositor {
                    address: test_data.depositor.to_string(),
                    capabilities: None,
                },
                InitialDepositor {
                    address: test_data.depositor2.to_string(),
                    capabilities: Some(DepositorCapabilities {
                        can_withdraw: false,
                    }),
                },
            ],
            initial_chains: vec![],
            initial_executors: vec![],
        };

        let res = instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert!(res.messages.is_empty());

        // Query registered depositors
        let query_msg = QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::RegisteredDepositors {
            enabled: None,
        });
        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let depositors_response: RegisteredDepositorsResponse = from_json(&res).unwrap();
        assert_eq!(depositors_response.depositors.len(), 2);
    }

    // ============================================================================
    // DEPOSIT TESTS
    // ============================================================================

    #[test]
    fn test_deposit_success() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: coins(1000, "ibc/usdc"),
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        let res = execute(deps.as_mut(), env, info, msg).unwrap();

        assert_eq!(res.attributes.len(), 4);
        assert_eq!(res.attributes[0].value, "deposit");
        assert_eq!(res.attributes[2].value, "1000");
    }

    #[test]
    fn test_deposit_unauthorized_depositor() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.non_depositor.clone(),
            funds: coins(1000, "ibc/usdc"),
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();

        assert_eq!(
            err,
            ContractError::DepositorNotRegistered {
                depositor_address: test_data.non_depositor.to_string()
            }
        );
    }

    #[test]
    fn test_deposit_wrong_denom() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: coins(1000, "wrong_denom"),
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();

        // Should fail due to wrong denom in cw_utils::must_pay
        assert!(matches!(err, ContractError::PaymentError(_)));
    }

    // ============================================================================
    // WITHDRAW TESTS
    // ============================================================================

    #[test]
    fn test_withdraw_success() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        // First deposit some funds
        let deposit_info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: coins(1000, "ibc/usdc"),
        };
        let deposit_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        execute(deps.as_mut(), env.clone(), deposit_info, deposit_msg).unwrap();

        // Update the querier to return the balance
        deps.querier
            .bank
            .update_balance(env.contract.address.clone(), coins(1000, "ibc/usdc"));

        // Now withdraw
        let withdraw_info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![],
        };
        let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
            coin: Coin {
                denom: "ibc/usdc".to_string(),
                amount: Uint128::new(500),
            },
        });

        let res = execute(deps.as_mut(), env, withdraw_info, withdraw_msg).unwrap();

        assert_eq!(res.messages.len(), 1);
        assert_eq!(res.attributes[0].value, "withdraw");
        assert_eq!(res.attributes[2].value, "500");
    }

    #[test]
    fn test_withdraw_insufficient_balance() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        // Try to withdraw without depositing
        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
            coin: Coin {
                denom: "ibc/usdc".to_string(),
                amount: Uint128::new(500),
            },
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::InsufficientBalance { .. }));
    }

    #[test]
    fn test_withdraw_zero_amount() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
            coin: Coin {
                denom: "ibc/usdc".to_string(),
                amount: Uint128::zero(),
            },
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::ZeroAmount {});
    }

    #[test]
    fn test_withdraw_wrong_denom() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
            coin: Coin {
                denom: "wrong_denom".to_string(),
                amount: Uint128::new(500),
            },
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::WrongTokenDenom { .. }));
    }

    #[test]
    fn test_withdraw_not_allowed() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let test_data = default_test_setup(&mut deps);

        // Setup contract with depositor that cannot withdraw
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
                capabilities: Some(DepositorCapabilities {
                    can_withdraw: false,
                }),
            }],
            initial_chains: vec![],
            initial_executors: vec![],
        };

        instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Try to withdraw
        let withdraw_info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![],
        };
        let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
            coin: Coin {
                denom: "ibc/usdc".to_string(),
                amount: Uint128::new(100),
            },
        });

        let err = execute(deps.as_mut(), env, withdraw_info, withdraw_msg).unwrap_err();
        assert_eq!(err, ContractError::WithdrawalNotAllowed {});
    }

    // ============================================================================
    // REGISTER/UNREGISTER DEPOSITOR TESTS
    // ============================================================================

    #[test]
    fn test_register_depositor_success() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RegisterDepositor {
            depositor_address: test_data.depositor2.to_string(),
            metadata: None,
        });

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "register_depositor");
    }

    #[test]
    fn test_register_depositor_with_capabilities() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let capabilities = DepositorCapabilities {
            can_withdraw: false,
        };

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RegisterDepositor {
            depositor_address: test_data.depositor2.to_string(),
            metadata: Some(to_json_binary(&capabilities).unwrap()),
        });

        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "register_depositor");

        // Query capabilities
        let query_msg = QueryMsg::CustomQuery(CctpAdapterQueryMsg::DepositorCapabilities {
            depositor_address: test_data.depositor2.to_string(),
        });
        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let caps: DepositorCapabilitiesResponse = from_json(&res).unwrap();
        assert!(!caps.capabilities.can_withdraw);
    }

    #[test]
    fn test_register_depositor_unauthorized() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.non_admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RegisterDepositor {
            depositor_address: test_data.depositor2.to_string(),
            metadata: None,
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    #[test]
    fn test_register_depositor_duplicate() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RegisterDepositor {
            depositor_address: test_data.depositor.to_string(), // Already registered
            metadata: None,
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(
            err,
            ContractError::DepositorAlreadyRegistered { .. }
        ));
    }

    #[test]
    fn test_unregister_depositor_success() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::UnregisterDepositor {
            depositor_address: test_data.depositor.to_string(),
        });

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "unregister_depositor");
    }

    #[test]
    fn test_unregister_depositor_unauthorized() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.non_admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::UnregisterDepositor {
            depositor_address: test_data.depositor.to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    // ============================================================================
    // SET DEPOSITOR ENABLED TESTS
    // ============================================================================

    #[test]
    fn test_set_depositor_enabled_success() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
            depositor_address: test_data.depositor.to_string(),
            enabled: false,
        });

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "toggle_depositor_enabled");
        assert_eq!(res.attributes[3].value, "false");
    }

    #[test]
    fn test_set_depositor_enabled_unauthorized() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.non_admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
            depositor_address: test_data.depositor.to_string(),
            enabled: false,
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    #[test]
    fn test_set_depositor_enabled_not_registered() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
            depositor_address: test_data.depositor2.to_string(),
            enabled: false,
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::DepositorNotRegistered { .. }));
    }

    // ============================================================================
    // QUERY TESTS
    // ============================================================================

    #[test]
    fn test_query_config() {
        let (deps, _) = setup_contract_with_defaults();
        let env = mock_env();

        let msg = QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::Config {});
        let res = query(deps.as_ref(), env, msg).unwrap();
        let config: ConfigResponse = from_json(&res).unwrap();

        assert_eq!(config.config.denom, "ibc/usdc");
        assert_eq!(config.config.noble_transfer_channel_id, "channel-0");
        assert_eq!(config.config.ibc_default_timeout_seconds, 600);
    }

    #[test]
    fn test_query_available_for_deposit() {
        let (deps, _) = setup_contract_with_defaults();
        let env = mock_env();

        let msg = QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::AvailableForDeposit {
            depositor_address: "any".to_string(),
            denom: "ibc/usdc".to_string(),
        });
        let res = query(deps.as_ref(), env, msg).unwrap();
        let available: AvailableAmountResponse = from_json(&res).unwrap();

        assert_eq!(available.amount, Uint128::MAX);
    }

    #[test]
    fn test_query_available_for_withdraw() {
        let (mut deps, _) = setup_contract_with_defaults();
        let env = mock_env();

        // Set contract balance
        deps.querier
            .bank
            .update_balance(env.contract.address.clone(), coins(1000, "ibc/usdc"));

        let msg = QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::AvailableForWithdraw {
            depositor_address: "any".to_string(),
            denom: "ibc/usdc".to_string(),
        });
        let res = query(deps.as_ref(), env, msg).unwrap();
        let available: AvailableAmountResponse = from_json(&res).unwrap();

        assert_eq!(available.amount, Uint128::new(1000));
    }

    #[test]
    fn test_query_time_to_withdraw() {
        let (deps, _) = setup_contract_with_defaults();
        let env = mock_env();

        let msg = QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::TimeToWithdraw {
            depositor_address: "any".to_string(),
            coin: Coin {
                denom: "ibc/usdc".to_string(),
                amount: Uint128::new(100),
            },
        });
        let res = query(deps.as_ref(), env, msg).unwrap();
        let time: TimeEstimateResponse = from_json(&res).unwrap();

        assert_eq!(time.blocks, 0);
        assert_eq!(time.seconds, 0);
    }

    #[test]
    fn test_query_registered_depositors() {
        let (deps, _) = setup_contract_with_defaults();
        let env = mock_env();

        let msg = QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::RegisteredDepositors {
            enabled: None,
        });
        let res = query(deps.as_ref(), env, msg).unwrap();
        let depositors: RegisteredDepositorsResponse = from_json(&res).unwrap();

        assert_eq!(depositors.depositors.len(), 1);
        assert!(depositors.depositors[0].enabled);
    }

    #[test]
    fn test_query_registered_depositors_with_filter() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        // Disable the depositor
        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
            depositor_address: test_data.depositor.to_string(),
            enabled: false,
        });
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Query only enabled depositors
        let msg = QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::RegisteredDepositors {
            enabled: Some(true),
        });
        let res = query(deps.as_ref(), env.clone(), msg).unwrap();
        let depositors: RegisteredDepositorsResponse = from_json(&res).unwrap();
        assert_eq!(depositors.depositors.len(), 0);

        // Query only disabled depositors
        let msg = QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::RegisteredDepositors {
            enabled: Some(false),
        });
        let res = query(deps.as_ref(), env, msg).unwrap();
        let depositors: RegisteredDepositorsResponse = from_json(&res).unwrap();
        assert_eq!(depositors.depositors.len(), 1);
    }

    #[test]
    fn test_query_depositor_capabilities() {
        let (deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let msg = QueryMsg::CustomQuery(CctpAdapterQueryMsg::DepositorCapabilities {
            depositor_address: test_data.depositor.to_string(),
        });
        let res = query(deps.as_ref(), env, msg).unwrap();
        let caps: DepositorCapabilitiesResponse = from_json(&res).unwrap();

        assert!(caps.capabilities.can_withdraw);
    }
}
