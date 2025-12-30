#[cfg(test)]
mod contract_tests {
    use cosmwasm_std::testing::{mock_env, MockApi, MockQuerier, MockStorage};
    use cosmwasm_std::{Addr, Coin, MessageInfo, OwnedDeps, Uint128};
    use neutron_sdk::bindings::query::NeutronQuery;

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
        deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier<NeutronQuery>, NeutronQuery>,
    ) -> TestSetupData {
        let admin = deps.api.addr_make("admin1");
        let depositor = deps.api.addr_make("depositor1");

        TestSetupData { admin, depositor }
    }

    fn setup_contract_with_depositor() -> (
        OwnedDeps<MockStorage, MockApi, MockQuerier<NeutronQuery>, NeutronQuery>,
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
            neutron_skip_contract: deps
                .api
                .addr_make("neutron1zvesudsdfxusz06jztpph4d3h5x6veglqsspxns2v2jqml9nhywskcc923")
                .to_string(),
            osmosis_skip_contract:
                "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u".to_string(),
            ibc_adapter: deps.api.addr_make("ibc_adapter").to_string(),
            default_timeout_nanos: 1800000000000,
            max_slippage_bps: 100,
            executors: vec![],
            initial_routes: vec![],
            initial_depositors: vec![test_data.depositor.to_string()],
        };

        instantiate(deps.as_mut(), env, info, msg).unwrap();
        (deps, test_data)
    }

    fn create_valid_neutron_route() -> UnifiedRoute {
        UnifiedRoute {
            venue: SwapVenue::NeutronAstroport,
            denom_in: "factory/neutron1/astro".to_string(),
            denom_out: "untrn".to_string(),
            operations: vec![SwapOperation {
                denom_in: "factory/neutron1/astro".to_string(),
                denom_out: "untrn".to_string(),
                pool: "pool1".to_string(),
                interface: None,
            }],
            swap_venue_name: "neutron-astroport".to_string(),
            forward_path: vec![],
            return_path: vec![],
            recover_address: None,
            enabled: true,
        }
    }

    fn create_valid_osmosis_route() -> UnifiedRoute {
        // For Osmosis routes, denoms in operations are as they appear on Osmosis
        // The route's denom_in/out should match the first/last operation respectively
        UnifiedRoute {
            venue: SwapVenue::Osmosis,
            denom_in: "ibc/NTRN_ON_OSMOSIS".to_string(),
            denom_out: "ibc/ATOM_ON_OSMOSIS".to_string(),
            operations: vec![SwapOperation {
                denom_in: "ibc/NTRN_ON_OSMOSIS".to_string(),
                denom_out: "ibc/ATOM_ON_OSMOSIS".to_string(),
                pool: "1234".to_string(),
                interface: None,
            }],
            swap_venue_name: "osmosis-poolmanager".to_string(),
            forward_path: vec![PathHop {
                channel: "channel-10".to_string(),
                receiver: "osmo1skip".to_string(),
            }],
            return_path: vec![
                PathHop {
                    channel: "channel-0".to_string(),
                    receiver: "cosmos1addr".to_string(),
                },
                PathHop {
                    channel: "channel-569".to_string(),
                    receiver: "neutron1addr".to_string(),
                },
            ],
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

        let msg = InstantiateMsg {
            admins: vec![deps.api.addr_make("admin1").to_string()],
            neutron_skip_contract: deps
                .api
                .addr_make("neutron1zvesudsdfxusz06jztpph4d3h5x6veglqsspxns2v2jqml9nhywskcc923")
                .to_string(),
            osmosis_skip_contract:
                "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u".to_string(),
            ibc_adapter: deps.api.addr_make("ibc_adapter").to_string(),
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

        let msg = InstantiateMsg {
            admins: vec![],
            neutron_skip_contract: "neutron1skip".to_string(),
            osmosis_skip_contract:
                "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u".to_string(),
            ibc_adapter: "neutron1ibc".to_string(),
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

        let msg = InstantiateMsg {
            admins: vec![deps.api.addr_make("admin1").to_string()],
            neutron_skip_contract: deps
                .api
                .addr_make("neutron1zvesudsdfxusz06jztpph4d3h5x6veglqsspxns2v2jqml9nhywskcc923")
                .to_string(),
            osmosis_skip_contract:
                "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u".to_string(),
            ibc_adapter: deps.api.addr_make("ibc_adapter").to_string(),
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
            funds: vec![Coin::new(1000u128, "untrn")],
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
            funds: vec![Coin::new(1000u128, "untrn")],
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
            funds: vec![Coin::new(0u128, "untrn")],
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
            funds: vec![Coin::new(1000u128, "untrn"), Coin::new(500u128, "uatom")],
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
            funds: vec![Coin::new(1000u128, "untrn")],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Update contract balance in mock
        deps.querier.bank.update_balance(
            env.contract.address.as_str(),
            vec![Coin::new(1000u128, "untrn")],
        );

        // Then withdraw
        let info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
            coin: Coin::new(500u128, "untrn"),
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
            coin: Coin::new(1000u128, "untrn"),
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

        let route = create_valid_neutron_route();
        let msg = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterRoute {
            route_id: "astro_to_ntrn".to_string(),
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

        let route = create_valid_neutron_route();
        let msg = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterRoute {
            route_id: "astro_to_ntrn".to_string(),
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
            venue: SwapVenue::NeutronAstroport,
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
            swap_venue_name: "neutron-astroport".to_string(),
            forward_path: vec![],
            return_path: vec![],
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
        let mut route = create_valid_neutron_route();
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

        // Register routes
        let neutron_route = create_valid_neutron_route();
        let osmosis_route = create_valid_osmosis_route();

        let msg1 = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterRoute {
            route_id: "neutron_route".to_string(),
            route: neutron_route,
        });
        execute(deps.as_mut(), env.clone(), info.clone(), msg1).unwrap();

        let msg2 = ExecuteMsg::CustomAction(SkipAdapterMsg::RegisterRoute {
            route_id: "osmosis_route".to_string(),
            route: osmosis_route,
        });
        execute(deps.as_mut(), env.clone(), info, msg2).unwrap();

        // Query Neutron routes only
        let query_msg = QueryMsg::CustomQuery(SkipAdapterQueryMsg::AllRoutes {
            venue: Some(SwapVenue::NeutronAstroport),
        });
        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let routes: AllRoutesResponse = cosmwasm_std::from_json(&res).unwrap();
        assert_eq!(routes.routes.len(), 1);
        assert_eq!(routes.routes[0].1.venue, SwapVenue::NeutronAstroport);
    }
}

#[cfg(test)]
mod cross_chain_tests {
    use cosmwasm_std::testing::mock_env;
    use cosmwasm_std::{from_json, Addr, Coin, Uint128, WasmMsg};

    use crate::cross_chain::{
        build_osmosis_swap_ibc_adapter_msg, construct_osmosis_wasm_hook_memo, IbcAdapterExecuteMsg,
        IbcAdapterMsg,
    };
    use crate::state::{Config, PathHop, SwapOperation, SwapVenue, UnifiedRoute};

    #[test]
    fn test_statom_to_atom_swap_memo() {
        let config = Config {
            neutron_skip_contract: Addr::unchecked(
                "neutron1zvesudsdfxusz06jztpph4d3h5x6veglqsspxns2v2jqml9nhywskcc923",
            ),
            osmosis_skip_contract:
                "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u".to_string(),
            ibc_adapter: Addr::unchecked("neutron1ibc"),
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
                channel: "channel-10".to_string(),
                receiver: "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u"
                    .to_string(),
            }],
            return_path: vec![
                PathHop {
                    channel: "channel-0".to_string(),
                    receiver: "cosmos16482tz43umq6c034efueggp73tpnc2q6pjhm7fjz0ghrpezwqcmq32xh7j"
                        .to_string(),
                },
                PathHop {
                    channel: "channel-569".to_string(),
                    receiver: "neutron1g4ydedvm96rqt9e8smcvwsqu8twp52gkrcg3aqg22uzj75d29res7avj8l"
                        .to_string(),
                },
            ],
            recover_address: Some(
                "osmo1advjelut4q0slqp9rq43ffcjmj4e00gxavg7dakqelps27n7u8qssc8ssn".to_string(),
            ),
            enabled: true,
        };

        let env = mock_env();
        let timeout = env.block.time.nanos() + config.default_timeout_nanos;

        let result = construct_osmosis_wasm_hook_memo(&config, &route, &Uint128::new(229521), &env);
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
                                    "memo": serde_json::json!({
                                        "forward": {
                                            "channel": "channel-569",
                                            "port": "transfer",
                                            "receiver": "neutron1g4ydedvm96rqt9e8smcvwsqu8twp52gkrcg3aqg22uzj75d29res7avj8l",
                                            "retries": 2,
                                            "timeout": timeout
                                        }
                                    }).to_string(),
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
    fn test_build_osmosis_swap_ibc_adapter_msg_single_hop() {
        let coin = Coin::new(1000000u128, "untrn");
        let wasm_hook_memo = "{\"wasm\":{\"contract\":\"osmo1skip\"}}".to_string();
        let forward_path = vec![PathHop {
            channel: "channel-10".to_string(),
            receiver: "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u".to_string(),
        }];
        let timeout_nanos = 1800000000000u64;

        let result = build_osmosis_swap_ibc_adapter_msg(
            "neutron1ibc".to_string(),
            coin.clone(),
            &forward_path,
            wasm_hook_memo.clone(),
            timeout_nanos,
        );
        assert!(
            result.is_ok(),
            "Failed to build IBC adapter message: {:?}",
            result.err()
        );

        let msg = result.unwrap();
        match msg {
            WasmMsg::Execute {
                contract_addr,
                msg: binary,
                ..
            } => {
                assert_eq!(contract_addr, "neutron1ibc");

                // Deserialize and verify the message content
                let ibc_msg: IbcAdapterExecuteMsg = from_json(&binary).unwrap();
                match ibc_msg {
                    IbcAdapterExecuteMsg::CustomAction(IbcAdapterMsg::TransferFunds {
                        coin: transfer_coin,
                        instructions,
                    }) => {
                        // Verify coin in message body
                        assert_eq!(transfer_coin, coin);

                        // Verify transfer instructions
                        assert_eq!(instructions.destination_chain, "osmosis-1");
                        assert_eq!(instructions.recipient, forward_path[0].receiver);
                        assert_eq!(instructions.timeout_seconds, None);

                        // Verify memo contains PFM forward structure with wasm hook
                        assert!(instructions.memo.is_some());
                        let memo_value: serde_json::Value =
                            serde_json::from_str(&instructions.memo.unwrap()).unwrap();

                        // Should have forward structure
                        assert!(memo_value.get("forward").is_some());
                        let forward = memo_value.get("forward").unwrap();
                        assert_eq!(
                            forward.get("channel").unwrap().as_str().unwrap(),
                            "channel-10"
                        );
                        assert_eq!(
                            forward.get("receiver").unwrap().as_str().unwrap(),
                            "osmo10a3k4hvk37cc4hnxctw4p95fhscd2z6h2rmx0aukc6rm8u9qqx9smfsh7u"
                        );

                        // Should have nested wasm hook
                        assert!(forward.get("next").is_some());
                        let wasm_next = forward.get("next").unwrap();
                        assert!(wasm_next.get("wasm").is_some());
                    }
                    _ => panic!("Expected CustomAction(TransferFunds)"),
                }
            }
            _ => panic!("Expected WasmMsg::Execute"),
        }
    }

    #[test]
    fn test_build_osmosis_swap_ibc_adapter_msg_multi_hop() {
        // Test multi-hop forward path: Neutron -> Cosmos Hub -> Osmosis
        let coin = Coin::new(1000000u128, "ibc/ATOM");
        let wasm_hook_memo = "{\"wasm\":{\"contract\":\"osmo1skip\"}}".to_string();
        let forward_path = vec![
            PathHop {
                channel: "channel-1".to_string(), // Neutron -> Cosmos Hub
                receiver: "cosmos1intermediate".to_string(),
            },
            PathHop {
                channel: "channel-0".to_string(), // Cosmos Hub -> Osmosis
                receiver: "osmo1skip".to_string(),
            },
        ];
        let timeout_nanos = 1800000000000u64;

        let result = build_osmosis_swap_ibc_adapter_msg(
            "neutron1ibc".to_string(),
            coin.clone(),
            &forward_path,
            wasm_hook_memo,
            timeout_nanos,
        );
        assert!(
            result.is_ok(),
            "Failed to build multi-hop IBC adapter message: {:?}",
            result.err()
        );

        let msg = result.unwrap();
        match msg {
            WasmMsg::Execute {
                contract_addr,
                msg: binary,
                ..
            } => {
                assert_eq!(contract_addr, "neutron1ibc");

                // Deserialize and verify the message content
                let ibc_msg: IbcAdapterExecuteMsg = from_json(&binary).unwrap();
                match ibc_msg {
                    IbcAdapterExecuteMsg::CustomAction(IbcAdapterMsg::TransferFunds {
                        coin: transfer_coin,
                        instructions,
                    }) => {
                        // Verify coin
                        assert_eq!(transfer_coin, coin);

                        // For multi-hop, first destination should be Cosmos Hub
                        assert_eq!(instructions.destination_chain, "cosmoshub-4");
                        assert_eq!(instructions.recipient, "cosmos1intermediate");

                        // Verify nested PFM structure
                        assert!(instructions.memo.is_some());
                        let memo_value: serde_json::Value =
                            serde_json::from_str(&instructions.memo.unwrap()).unwrap();

                        // First forward hop
                        assert!(memo_value.get("forward").is_some());
                        let first_forward = memo_value.get("forward").unwrap();
                        assert_eq!(
                            first_forward.get("channel").unwrap().as_str().unwrap(),
                            "channel-1"
                        );

                        // Should have nested second hop
                        assert!(first_forward.get("next").is_some());
                        let second_forward =
                            first_forward.get("next").unwrap().get("forward").unwrap();
                        assert_eq!(
                            second_forward.get("channel").unwrap().as_str().unwrap(),
                            "channel-0"
                        );
                        assert_eq!(
                            second_forward.get("receiver").unwrap().as_str().unwrap(),
                            "osmo1skip"
                        );

                        // Final hop should have wasm hook
                        assert!(second_forward.get("next").is_some());
                        let wasm_next = second_forward.get("next").unwrap();
                        assert!(wasm_next.get("wasm").is_some());
                    }
                    _ => panic!("Expected CustomAction(TransferFunds)"),
                }
            }
            _ => panic!("Expected WasmMsg::Execute"),
        }
    }
}
