#[cfg(test)]
mod contract_tests {
    use cosmwasm_std::testing::{mock_env, MockApi, MockQuerier, MockStorage};
    use cosmwasm_std::{Addr, Coin, Empty, MessageInfo, OwnedDeps, Uint128};

    use crate::contract::{execute, instantiate, query};
    use crate::error::ContractError;
    use crate::msg::*;
    use crate::state::{PathHop, SwapOperation, SwapVenue, UnifiedRoute};
    use crate::testing_mocks::mock_dependencies;

    // Test data structure
    pub struct TestSetupData {
        pub admin: Addr,
        pub depositor: Addr,
    }

    fn default_test_setup(
        deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, Empty>,
    ) -> TestSetupData {
        let admin = deps.api.addr_make("admin1");
        let depositor = deps.api.addr_make("depositor1");

        TestSetupData { admin, depositor }
    }

    fn setup_contract_with_depositor() -> (
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

        let mut skip_contracts = std::collections::BTreeMap::new();
        skip_contracts.insert(
            "osmosis".to_string(),
            "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u".to_string(),
        );

        let msg = InstantiateMsg {
            admins: vec![test_data.admin.to_string()],
            skip_contracts,
            default_timeout_nanos: 1800000000000,
            max_slippage_bps: 100,
            executors: vec![],
            initial_routes: vec![],
            initial_depositors: vec![test_data.depositor.to_string()],
        };

        instantiate(deps.as_mut(), env, info, msg).unwrap();
        (deps, test_data)
    }

    fn create_valid_osmosis_route() -> UnifiedRoute {
        // For Osmosis routes, denoms in operations are as they appear on Osmosis
        UnifiedRoute {
            venue: SwapVenue::Osmosis,
            denom_in: "ibc/UATOM_ON_OSMOSIS".to_string(),
            denom_out: "ibc/STATOM_ON_OSMOSIS".to_string(),
            operations: vec![SwapOperation {
                denom_in: "ibc/UATOM_ON_OSMOSIS".to_string(),
                denom_out: "ibc/STATOM_ON_OSMOSIS".to_string(),
                pool: "1234".to_string(),
                interface: None,
            }],
            swap_venue_name: "osmosis-poolmanager".to_string(),
            forward_path: vec![PathHop {
                chain_id: "osmosis-1".to_string(),
                channel: "channel-10".to_string(),
                receiver: "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u"
                    .to_string(),
            }],
            return_path: vec![PathHop {
                chain_id: "cosmoshub-4".to_string(),
                channel: "channel-0".to_string(),
                receiver: "cosmos1addr".to_string(),
            }],
            recover_address: Some("osmo1recovery".to_string()),
            enabled: true,
        }
    }

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

        let mut skip_contracts = std::collections::BTreeMap::new();
        skip_contracts.insert(
            "osmosis".to_string(),
            "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u".to_string(),
        );

        let msg = InstantiateMsg {
            admins: vec![deps.api.addr_make("admin1").to_string()],
            skip_contracts,
            default_timeout_nanos: 1800000000000,
            max_slippage_bps: 100,
            executors: vec![],
            initial_routes: vec![],
            initial_depositors: vec![],
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

        let mut skip_contracts = std::collections::BTreeMap::new();
        skip_contracts.insert(
            "osmosis".to_string(),
            "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u".to_string(),
        );

        let msg = InstantiateMsg {
            admins: vec![],
            skip_contracts,
            default_timeout_nanos: 1800000000000,
            max_slippage_bps: 100,
            executors: vec![],
            initial_routes: vec![],
            initial_depositors: vec![],
        };

        let err = instantiate(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::AtLeastOneAdmin {});
    }

    #[test]
    fn test_instantiate_invalid_slippage() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = MessageInfo {
            sender: deps.api.addr_make("creator"),
            funds: vec![],
        };

        let mut skip_contracts = std::collections::BTreeMap::new();
        skip_contracts.insert(
            "osmosis".to_string(),
            "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u".to_string(),
        );

        let msg = InstantiateMsg {
            admins: vec![deps.api.addr_make("admin1").to_string()],
            skip_contracts,
            default_timeout_nanos: 1800000000000,
            max_slippage_bps: 2000,
            executors: vec![],
            initial_routes: vec![],
            initial_depositors: vec![],
        };

        let err = instantiate(deps.as_mut(), env, info, msg).unwrap_err();
        match err {
            ContractError::InvalidSlippage { bps, max_bps } => {
                assert_eq!(bps, 2000);
                assert_eq!(max_bps, 1000);
            }
            _ => panic!("Expected InvalidSlippage error"),
        }
    }

    // ============================================================================
    // DEPOSIT TESTS
    // ============================================================================

    #[test]
    fn test_deposit_success() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();
        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![Coin::new(1000u128, "uatom")],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        let res = execute(deps.as_mut(), env, info, msg);
        assert!(res.is_ok());
    }

    #[test]
    fn test_deposit_unregistered_depositor() {
        let (mut deps, _test_data) = setup_contract_with_depositor();
        let env = mock_env();
        let info = MessageInfo {
            sender: deps.api.addr_make("random_user"),
            funds: vec![Coin::new(1000u128, "uatom")],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        match err {
            ContractError::DepositorNotRegistered { .. } => {}
            _ => panic!("Expected DepositorNotRegistered error"),
        }
    }

    #[test]
    fn test_deposit_zero_amount() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();
        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![Coin::new(0u128, "uatom")],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::ZeroAmount {});
    }

    #[test]
    fn test_deposit_multiple_coins() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();
        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![Coin::new(1000u128, "uatom"), Coin::new(500u128, "uosmo")],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        match err {
            ContractError::InvalidFunds { count } => assert_eq!(count, 2),
            _ => panic!("Expected InvalidFunds error"),
        }
    }

    // ============================================================================
    // WITHDRAW TESTS
    // ============================================================================

    #[test]
    fn test_withdraw_success() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();

        // First deposit
        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![Coin::new(1000u128, "uatom")],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Update contract balance in mock
        deps.querier.bank.update_balance(
            env.contract.address.as_str(),
            vec![Coin::new(1000u128, "uatom")],
        );

        // Then withdraw
        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
            coin: Coin::new(500u128, "uatom"),
        });
        let res = execute(deps.as_mut(), env, info, msg);
        assert!(res.is_ok(), "Withdraw failed: {:?}", res);
    }

    #[test]
    fn test_withdraw_insufficient_balance() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();
        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
            coin: Coin::new(1000u128, "uatom"),
        });
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::InsufficientBalance {});
    }

    // ============================================================================
    // ROUTE MANAGEMENT TESTS
    // ============================================================================

    #[test]
    fn test_register_route_success() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();
        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let route = create_valid_osmosis_route();
        let msg = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterRoute {
            route_id: "atom_to_statom".to_string(),
            route,
        });

        let res = execute(deps.as_mut(), env, info, msg);
        assert!(res.is_ok());
    }

    #[test]
    fn test_register_route_unauthorized() {
        let (mut deps, _test_data) = setup_contract_with_depositor();
        let env = mock_env();
        let info = MessageInfo {
            sender: deps.api.addr_make("random_user"),
            funds: vec![],
        };

        let route = create_valid_osmosis_route();
        let msg = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterRoute {
            route_id: "atom_to_statom".to_string(),
            route,
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    #[test]
    fn test_register_route_invalid_path_discontinuity() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();
        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let route = UnifiedRoute {
            venue: SwapVenue::Osmosis,
            denom_in: "tokenA".to_string(),
            denom_out: "tokenC".to_string(),
            operations: vec![
                SwapOperation {
                    denom_in: "tokenA".to_string(),
                    denom_out: "tokenB".to_string(),
                    pool: "pool1".to_string(),
                    interface: None,
                },
                SwapOperation {
                    denom_in: "tokenX".to_string(), // Discontinuous!
                    denom_out: "tokenC".to_string(),
                    pool: "pool2".to_string(),
                    interface: None,
                },
            ],
            swap_venue_name: "osmosis-poolmanager".to_string(),
            forward_path: vec![PathHop {
                chain_id: "osmosis-1".to_string(),
                channel: "channel-10".to_string(),
                receiver: "osmo1skip".to_string(),
            }],
            return_path: vec![PathHop {
                chain_id: "cosmoshub-4".to_string(),
                channel: "channel-0".to_string(),
                receiver: "cosmos1addr".to_string(),
            }],
            recover_address: None,
            enabled: true,
        };

        let msg = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterRoute {
            route_id: "invalid_route".to_string(),
            route,
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        match err {
            ContractError::InvalidRoute { reason } => {
                assert!(reason.contains("does not match"));
            }
            _ => panic!("Expected InvalidRoute error"),
        }
    }

    #[test]
    fn test_execute_swap_route_disabled() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();
        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        // Register disabled route
        let mut route = create_valid_osmosis_route();
        route.enabled = false;
        let register_msg = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterRoute {
            route_id: "route1".to_string(),
            route,
        });
        execute(deps.as_mut(), env.clone(), info.clone(), register_msg).unwrap();

        // Try to execute swap
        let msg = ExecuteMsg::CustomAction(SkipAdapterMsg::ExecuteSwap {
            params: SwapParams {
                route_id: "route1".to_string(),
                amount_in: Uint128::new(1000),
                min_amount_out: Uint128::new(900),
            },
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        match err {
            ContractError::RouteDisabled { route_id } => {
                assert_eq!(route_id, "route1");
            }
            _ => panic!("Expected RouteDisabled error"),
        }
    }

    #[test]
    fn test_query_all_routes_filtered() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();
        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        // Register two Osmosis routes
        let osmosis_route_1 = create_valid_osmosis_route();
        let mut osmosis_route_2 = create_valid_osmosis_route();
        osmosis_route_2.denom_in = "ibc/STATOM_ON_OSMOSIS".to_string();
        osmosis_route_2.denom_out = "uosmo".to_string();

        let msg1 = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterRoute {
            route_id: "osmosis_route_1".to_string(),
            route: osmosis_route_1,
        });
        execute(deps.as_mut(), env.clone(), info.clone(), msg1).unwrap();

        let msg2 = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterRoute {
            route_id: "osmosis_route_2".to_string(),
            route: osmosis_route_2,
        });
        execute(deps.as_mut(), env.clone(), info, msg2).unwrap();

        // Query all routes without filter — expect 2
        let query_msg = QueryMsg::CustomQuery(SkipAdapterQueryMsg::AllRoutes { venue: None });
        let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
        let routes: AllRoutesResponse = cosmwasm_std::from_json(&res).unwrap();
        assert_eq!(routes.routes.len(), 2);

        // Query Osmosis routes only — expect 2 (only venue on Hub)
        let query_msg = QueryMsg::CustomQuery(SkipAdapterQueryMsg::AllRoutes {
            venue: Some(SwapVenue::Osmosis),
        });
        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let routes: AllRoutesResponse = cosmwasm_std::from_json(&res).unwrap();
        assert_eq!(routes.routes.len(), 2);
        assert_eq!(routes.routes[0].1.venue, SwapVenue::Osmosis);
    }

    // ============================================================================
    // ADMIN MANAGEMENT TESTS
    // ============================================================================

    #[test]
    fn test_add_admin_success() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();

        let admin2 = deps.api.addr_make("admin2");
        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::AddAdmin {
            admin_address: admin2.to_string(),
        });
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "add_admin");
        assert_eq!(res.attributes[2].value, admin2.to_string());

        let query_msg = QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::Admins {});
        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let admins: AdminsResponse = cosmwasm_std::from_json(&res).unwrap();
        assert_eq!(admins.admins.len(), 2);
    }

    #[test]
    fn test_add_admin_unauthorized() {
        let (mut deps, _) = setup_contract_with_depositor();
        let env = mock_env();

        let non_admin = deps.api.addr_make("non_admin");
        let admin2 = deps.api.addr_make("admin2");
        let info = MessageInfo {
            sender: non_admin,
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::AddAdmin {
            admin_address: admin2.to_string(),
        });
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    #[test]
    fn test_add_admin_duplicate() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::AddAdmin {
            admin_address: test_data.admin.to_string(),
        });
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::AdminAlreadyExists { .. }));
    }

    #[test]
    fn test_remove_admin_success() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();

        let admin2 = deps.api.addr_make("admin2");
        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let add_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::AddAdmin {
            admin_address: admin2.to_string(),
        });
        execute(deps.as_mut(), env.clone(), info, add_msg).unwrap();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let remove_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RemoveAdmin {
            admin_address: test_data.admin.to_string(),
        });
        let res = execute(deps.as_mut(), env.clone(), info, remove_msg).unwrap();
        assert_eq!(res.attributes[0].value, "remove_admin");

        let query_msg = QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::Admins {});
        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let admins: AdminsResponse = cosmwasm_std::from_json(&res).unwrap();
        assert_eq!(admins.admins.len(), 1);
        assert_eq!(admins.admins[0], admin2.to_string());
    }

    #[test]
    fn test_remove_admin_unauthorized() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();

        let non_admin = deps.api.addr_make("non_admin");
        let info = MessageInfo {
            sender: non_admin,
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RemoveAdmin {
            admin_address: test_data.admin.to_string(),
        });
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    #[test]
    fn test_remove_admin_not_found() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();

        let non_admin = deps.api.addr_make("non_admin");
        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RemoveAdmin {
            admin_address: non_admin.to_string(),
        });
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::AdminNotFound { .. }));
    }

    #[test]
    fn test_remove_last_admin_fails() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RemoveAdmin {
            admin_address: test_data.admin.to_string(),
        });
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::CannotRemoveLastAdmin {});
    }

    #[test]
    fn test_admin_self_removal() {
        let (mut deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();

        let admin2 = deps.api.addr_make("admin2");
        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let add_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::AddAdmin {
            admin_address: admin2.to_string(),
        });
        execute(deps.as_mut(), env.clone(), info, add_msg).unwrap();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let remove_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::RemoveAdmin {
            admin_address: test_data.admin.to_string(),
        });
        let res = execute(deps.as_mut(), env.clone(), info, remove_msg).unwrap();
        assert_eq!(res.attributes[0].value, "remove_admin");

        let new_admin = deps.api.addr_make("new_admin");
        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let try_add_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::AddAdmin {
            admin_address: new_admin.to_string(),
        });
        let err = execute(deps.as_mut(), env, info, try_add_msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    #[test]
    fn test_query_admins() {
        let (deps, test_data) = setup_contract_with_depositor();
        let env = mock_env();

        let query_msg = QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::Admins {});
        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let admins: AdminsResponse = cosmwasm_std::from_json(&res).unwrap();

        assert_eq!(admins.admins.len(), 1);
        assert_eq!(admins.admins[0], test_data.admin.to_string());
    }
}

#[cfg(test)]
mod cross_chain_tests {
    use cosmwasm_std::testing::mock_env;
    use cosmwasm_std::{Coin, CosmosMsg, IbcMsg, Uint128};

    use crate::cross_chain::{
        build_cross_chain_swap_ibc_msg, construct_cross_chain_wasm_hook_memo,
    };
    use crate::state::{Config, PathHop, SwapOperation, SwapVenue, UnifiedRoute};

    #[test]
    fn test_statom_to_atom_swap_memo() {
        let mut skip_contracts = std::collections::BTreeMap::new();
        skip_contracts.insert(
            "osmosis".to_string(),
            "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u".to_string(),
        );

        let config = Config {
            skip_contracts,
            default_timeout_nanos: 1800000000000,
            max_slippage_bps: 100,
        };

        let route = UnifiedRoute {
            venue: SwapVenue::Osmosis,
            denom_in: "ibc/C140AFD542AE77BD7DCC83F13FDD8C5E5BB8C4929785E6EC2F4C636F98F17901"
                .to_string(),
            denom_out: "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2"
                .to_string(),
            operations: vec![SwapOperation {
                denom_in: "ibc/C140AFD542AE77BD7DCC83F13FDD8C5E5BB8C4929785E6EC2F4C636F98F17901"
                    .to_string(),
                denom_out: "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2"
                    .to_string(),
                pool: "803".to_string(),
                interface: None,
            }],
            swap_venue_name: "osmosis-poolmanager".to_string(),
            forward_path: vec![PathHop {
                chain_id: "osmosis-1".to_string(),
                channel: "channel-10".to_string(),
                receiver: "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u"
                    .to_string(),
            }],
            return_path: vec![PathHop {
                chain_id: "cosmoshub-4".to_string(),
                channel: "channel-0".to_string(),
                receiver: "cosmos16482tz43umq6c034efueggp73tpnc2q6pjhm7fjz0ghrpezwqcmq32xh7j"
                    .to_string(),
            }],
            recover_address: Some(
                "osmo1advjelut4q0slqp9rq43ffcjmj4e00gxavg7dakqelps27n7u8qssc8ssn".to_string(),
            ),
            enabled: true,
        };

        let env = mock_env();
        let timeout = env.block.time.nanos() + config.default_timeout_nanos;

        let result =
            construct_cross_chain_wasm_hook_memo(&config, &route, &Uint128::new(229521), &env);
        assert!(result.is_ok());

        let actual_memo = result.unwrap();

        // Parse actual memo for comparison
        let actual_json: serde_json::Value =
            serde_json::from_str(&actual_memo).expect("Invalid JSON in actual memo");

        // Construct expected memo using serde_json::json! macro for canonical comparison
        let expected_json = serde_json::json!({
            "wasm": {
                "contract": "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u",
                "msg": {
                    "swap_and_action": {
                        "user_swap": {
                            "swap_exact_asset_in": {
                                "swap_venue_name": "osmosis-poolmanager",
                                "operations": [{
                                    "denom_in": "ibc/C140AFD542AE77BD7DCC83F13FDD8C5E5BB8C4929785E6EC2F4C636F98F17901",
                                    "denom_out": "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2",
                                    "pool": "803"
                                    // Note: interface field is omitted when None
                                }]
                            }
                        },
                        "min_asset": {
                            "native": {
                                "denom": "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2",
                                "amount": "229521"
                            }
                        },
                        "timeout_timestamp": timeout,
                        "post_swap_action": {
                            "ibc_transfer": {
                                "ibc_info": {
                                    "source_channel": "channel-0",
                                    "receiver": "cosmos16482tz43umq6c034efueggp73tpnc2q6pjhm7fjz0ghrpezwqcmq32xh7j",
                                    "memo": "",
                                    "recover_address": "osmo1advjelut4q0slqp9rq43ffcjmj4e00gxavg7dakqelps27n7u8qssc8ssn"
                                }
                            }
                        },
                        "affiliates": []
                    }
                }
            }
        });

        // Canonical JSON comparison - order-independent
        assert_eq!(
            actual_json,
            expected_json,
            "Memo mismatch:\nActual: {}\nExpected: {}",
            serde_json::to_string_pretty(&actual_json).unwrap(),
            serde_json::to_string_pretty(&expected_json).unwrap()
        );
    }

    #[test]
    fn test_build_cross_chain_swap_ibc_msg_single_hop() {
        let coin = Coin::new(1000000u128, "uatom");
        let wasm_hook_memo = "{\"wasm\":{\"contract\":\"osmo1skip\"}}".to_string();
        let forward_path = vec![PathHop {
            chain_id: "osmosis-1".to_string(),
            channel: "channel-10".to_string(),
            receiver: "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u".to_string(),
        }];
        let timeout_nanos = 1800000000000u64;

        let result = build_cross_chain_swap_ibc_msg(
            coin.clone(),
            &forward_path,
            wasm_hook_memo.clone(),
            timeout_nanos,
        );
        assert!(
            result.is_ok(),
            "Failed to build IBC message: {:?}",
            result.err()
        );

        let msg = result.unwrap();
        match msg {
            CosmosMsg::Ibc(IbcMsg::Transfer {
                channel_id,
                to_address,
                amount,
                memo,
                ..
            }) => {
                assert_eq!(channel_id, "channel-10");
                assert_eq!(
                    to_address,
                    "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u"
                );
                assert_eq!(amount, coin);

                // For single hop, memo should be just the wasm hook (no PFM wrapper)
                assert!(memo.is_some());
                let memo_value: serde_json::Value = serde_json::from_str(&memo.unwrap()).unwrap();
                assert!(memo_value.get("wasm").is_some());
                let wasm = memo_value.get("wasm").unwrap();
                assert_eq!(wasm.get("contract").unwrap().as_str().unwrap(), "osmo1skip");
            }
            _ => panic!("Expected CosmosMsg::Ibc(IbcMsg::Transfer)"),
        }
    }

    #[test]
    fn test_build_cross_chain_swap_ibc_msg_multi_hop() {
        // Test multi-hop forward path: Cosmos Hub -> intermediate -> Osmosis
        let coin = Coin::new(1000000u128, "uatom");
        let wasm_hook_memo = "{\"wasm\":{\"contract\":\"osmo1skip\"}}".to_string();
        let forward_path = vec![
            PathHop {
                chain_id: "stride-1".to_string(),
                channel: "channel-391".to_string(), // Cosmos Hub -> Stride
                receiver: "stride1intermediate".to_string(),
            },
            PathHop {
                chain_id: "osmosis-1".to_string(),
                channel: "channel-5".to_string(), // Stride -> Osmosis
                receiver: "osmo1skip".to_string(),
            },
        ];
        let timeout_nanos = 1800000000000u64;

        let result = build_cross_chain_swap_ibc_msg(
            coin.clone(),
            &forward_path,
            wasm_hook_memo,
            timeout_nanos,
        );
        assert!(
            result.is_ok(),
            "Failed to build multi-hop IBC message: {:?}",
            result.err()
        );

        let msg = result.unwrap();
        match msg {
            CosmosMsg::Ibc(IbcMsg::Transfer {
                channel_id,
                to_address,
                amount,
                memo,
                ..
            }) => {
                // First hop goes in the IBC transfer itself
                assert_eq!(channel_id, "channel-391");
                assert_eq!(to_address, "stride1intermediate");
                assert_eq!(amount, coin);

                // PFM memo contains ONLY the second hop (Stride -> Osmosis)
                assert!(memo.is_some());
                let memo_value: serde_json::Value = serde_json::from_str(&memo.unwrap()).unwrap();

                assert!(memo_value.get("forward").is_some());
                let forward = memo_value.get("forward").unwrap();
                assert_eq!(
                    forward.get("channel").unwrap().as_str().unwrap(),
                    "channel-5"
                );
                assert_eq!(
                    forward.get("receiver").unwrap().as_str().unwrap(),
                    "osmo1skip"
                );

                // Should have nested wasm hook
                assert!(forward.get("next").is_some());
                let wasm_next = forward.get("next").unwrap();
                assert!(wasm_next.get("wasm").is_some());
                assert_eq!(
                    wasm_next
                        .get("wasm")
                        .unwrap()
                        .get("contract")
                        .unwrap()
                        .as_str()
                        .unwrap(),
                    "osmo1skip"
                );
            }
            _ => panic!("Expected CosmosMsg::Ibc(IbcMsg::Transfer)"),
        }
    }
}
