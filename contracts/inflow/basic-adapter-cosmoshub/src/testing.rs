#[cfg(test)]
mod tests {
    use cosmwasm_std::testing::{mock_env, MOCK_CONTRACT_ADDR};
    use cosmwasm_std::{coins, from_json, BankMsg, CosmosMsg, MessageInfo, Uint128};

    use crate::contract::{execute, instantiate, query};
    use crate::error::ContractError;
    use crate::msg::{
        AdminsResponse, AvailableAmountResponse, ExecuteMsg, InstantiateMsg, QueryMsg,
        RegisteredDepositorsResponse,
    };
    use crate::testing_mocks::{mock_dependencies, setup_contract_with_defaults};
    use interface::inflow_adapter::{AdapterInterfaceMsg, AdapterInterfaceQueryMsg};

    #[test]
    fn test_instantiate_success() {
        let (deps, _test_data) = setup_contract_with_defaults();

        // Config query succeeds and returns an empty response
        query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::Config {}),
        )
        .unwrap();
    }

    #[test]
    fn test_instantiate_duplicate_admin_fails() {
        let mut deps = mock_dependencies();
        let admin_addr = deps.api.addr_make("admin1").to_string();
        let info = MessageInfo {
            sender: deps.api.addr_make("creator"),
            funds: vec![],
        };
        let msg = InstantiateMsg {
            admins: vec![admin_addr.clone(), admin_addr],
            initial_depositors: vec![],
        };
        let err = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        assert!(matches!(err, ContractError::AdminAlreadyExists { .. }));
    }

    #[test]
    fn test_instantiate_no_admins_fails() {
        let mut deps = mock_dependencies();
        let info = MessageInfo {
            sender: deps.api.addr_make("creator"),
            funds: vec![],
        };
        let msg = InstantiateMsg {
            admins: vec![],
            initial_depositors: vec![],
        };
        let err = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        assert_eq!(err, ContractError::AtLeastOneAdmin {});
    }

    #[test]
    fn test_deposit_success() {
        let (mut deps, test_data) = setup_contract_with_defaults();

        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: coins(1_000_000, "uatom"),
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        execute(deps.as_mut(), mock_env(), info, msg).unwrap();
    }

    #[test]
    fn test_deposit_unregistered_fails() {
        let (mut deps, test_data) = setup_contract_with_defaults();

        let info = MessageInfo {
            sender: test_data.non_depositor.clone(),
            funds: coins(1_000_000, "uatom"),
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        assert!(matches!(err, ContractError::DepositorNotRegistered { .. }));
    }

    #[test]
    fn test_deposit_zero_amount_fails() {
        let (mut deps, test_data) = setup_contract_with_defaults();

        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: coins(0, "uatom"),
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        assert!(matches!(err, ContractError::InvalidFunds { .. }));
    }

    #[test]
    fn test_withdraw_success() {
        let (mut deps, test_data) = setup_contract_with_defaults();

        deps.querier
            .bank
            .update_balance(MOCK_CONTRACT_ADDR, coins(2_000_000, "uatom"));

        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
            coin: cosmwasm_std::Coin {
                denom: "uatom".to_string(),
                amount: Uint128::new(1_000_000),
            },
        });
        let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        assert_eq!(res.messages.len(), 1);
        match &res.messages[0].msg {
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(to_address, &test_data.depositor.to_string());
                assert_eq!(amount, &coins(1_000_000, "uatom"));
            }
            _ => panic!("expected BankMsg::Send"),
        }
    }

    #[test]
    fn test_withdraw_insufficient_balance_fails() {
        let (mut deps, test_data) = setup_contract_with_defaults();

        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
            coin: cosmwasm_std::Coin {
                denom: "uatom".to_string(),
                amount: Uint128::new(1_000_000),
            },
        });
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        assert_eq!(err, ContractError::InsufficientBalance {});
    }

    #[test]
    fn test_register_depositor() {
        let (mut deps, test_data) = setup_contract_with_defaults();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let new_depositor = deps.api.addr_make("new_depositor");
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RegisterDepositor {
            depositor_address: new_depositor.to_string(),
            metadata: None,
        });
        execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        let res: RegisteredDepositorsResponse = from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::RegisteredDepositors {
                    enabled: None,
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(res.depositors.len(), 2); // depositor1 from setup + new_depositor
        let found = res
            .depositors
            .iter()
            .find(|d| d.depositor_address == new_depositor.to_string());
        assert!(found.is_some());
        assert!(found.unwrap().enabled);
    }

    #[test]
    fn test_register_depositor_unauthorized() {
        let (mut deps, test_data) = setup_contract_with_defaults();

        let info = MessageInfo {
            sender: test_data.non_admin.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RegisterDepositor {
            depositor_address: test_data.non_depositor.to_string(),
            metadata: None,
        });
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    #[test]
    fn test_set_depositor_enabled() {
        let (mut deps, test_data) = setup_contract_with_defaults();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
            depositor_address: test_data.depositor.to_string(),
            enabled: false,
        });
        execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        // Deposit should now fail
        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: coins(1_000_000, "uatom"),
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        assert_eq!(err, ContractError::Unauthorized {});
    }

    #[test]
    fn test_add_and_remove_admin() {
        let (mut deps, test_data) = setup_contract_with_defaults();

        // Add admin2
        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        execute(
            deps.as_mut(),
            mock_env(),
            info,
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::AddAdmin {
                admin_address: test_data.admin2.to_string(),
            }),
        )
        .unwrap();

        let res: AdminsResponse = from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::Admins {}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(res.admins.len(), 2);

        // Remove admin1
        let info = MessageInfo {
            sender: test_data.admin2.clone(),
            funds: vec![],
        };
        execute(
            deps.as_mut(),
            mock_env(),
            info,
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::RemoveAdmin {
                admin_address: test_data.admin.to_string(),
            }),
        )
        .unwrap();

        let res: AdminsResponse = from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::Admins {}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(res.admins, vec![test_data.admin2.to_string()]);
    }

    #[test]
    fn test_cannot_remove_last_admin() {
        let (mut deps, test_data) = setup_contract_with_defaults();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let err = execute(
            deps.as_mut(),
            mock_env(),
            info,
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::RemoveAdmin {
                admin_address: test_data.admin.to_string(),
            }),
        )
        .unwrap_err();
        assert_eq!(err, ContractError::CannotRemoveLastAdmin {});
    }

    #[test]
    fn test_available_for_withdraw_query() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        deps.querier
            .bank
            .update_balance(MOCK_CONTRACT_ADDR, coins(5_000_000, "uatom"));

        let res: AvailableAmountResponse = from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::AvailableForWithdraw {
                    depositor_address: test_data.depositor.to_string(),
                    denom: "uatom".to_string(),
                }),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(res.amount, Uint128::new(5_000_000));
    }

    #[test]
    fn test_withdraw_any_denom() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        deps.querier
            .bank
            .update_balance(MOCK_CONTRACT_ADDR, coins(1_000_000, "ibc/SOMEHASH"));

        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
            coin: cosmwasm_std::Coin {
                denom: "ibc/SOMEHASH".to_string(),
                amount: Uint128::new(500_000),
            },
        });
        let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();
        match &res.messages[0].msg {
            CosmosMsg::Bank(BankMsg::Send { amount, .. }) => {
                assert_eq!(amount[0].denom, "ibc/SOMEHASH");
            }
            _ => panic!("expected BankMsg::Send"),
        }
    }
}
