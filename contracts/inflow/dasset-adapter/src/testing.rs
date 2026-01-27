#[cfg(test)]
mod tests {
    use crate::contract::{execute, instantiate, query};
    use crate::error::ContractError;
    use crate::msg::{
        ConfigResponse, DAssetAdapterMsg, DAssetAdapterQueryMsg, ExecuteMsg, ExecutorsResponse,
        InstantiateMsg, QueryMsg, TokenConfigResponse, TokenRegistration, TokensResponse,
    };
    use crate::state::{EXECUTORS, TOKEN_REGISTRY, WHITELISTED_DEPOSITORS};
    use cosmwasm_std::testing::{
        mock_dependencies, mock_env, MockApi, MockQuerier, MockStorage, MOCK_CONTRACT_ADDR,
    };
    use cosmwasm_std::{from_json, BankMsg, Coin, CosmosMsg, MessageInfo, Uint128};
    use cw_utils::PaymentError;
    use interface::inflow_adapter::{
        AdapterInterfaceMsg, AdapterInterfaceQueryMsg, RegisteredDepositorsResponse,
    };

    const ADMIN: &str = "admin";
    const EXECUTOR: &str = "executor";
    const DEPOSITOR: &str = "depositor";
    const RANDOM: &str = "random";

    fn setup() -> (
        cosmwasm_std::OwnedDeps<MockStorage, MockApi, MockQuerier>,
        MockApi,
        cosmwasm_std::Env,
    ) {
        let deps = mock_dependencies();
        let env = mock_env();
        let api = deps.api;
        (deps, api, env)
    }

    fn get_message_info(api: &MockApi, sender: &str, funds: &[Coin]) -> MessageInfo {
        MessageInfo {
            sender: api.addr_make(sender),
            funds: funds.to_vec(),
        }
    }

    fn get_default_instantiate_msg(api: &MockApi) -> InstantiateMsg {
        InstantiateMsg {
            initial_admins: vec![api.addr_make(ADMIN).to_string()],
            initial_executors: vec![api.addr_make(EXECUTOR).to_string()],
            initial_depositors: vec![api.addr_make(DEPOSITOR).to_string()],
            initial_tokens: vec![TokenRegistration {
                symbol: "datom".to_string(),
                denom: "factory/drop/datom".to_string(),
                drop_staking_core: api.addr_make("staking_core").to_string(),
                drop_voucher: api.addr_make("voucher").to_string(),
                drop_withdrawal_manager: api.addr_make("withdraw_manager").to_string(),
                base_asset_denom: "uatom".to_string(),
            }],
        }
    }

    fn get_minimal_instantiate_msg(api: &MockApi) -> InstantiateMsg {
        InstantiateMsg {
            initial_admins: vec![api.addr_make(ADMIN).to_string()],
            initial_executors: vec![api.addr_make(EXECUTOR).to_string()],
            initial_depositors: vec![],
            initial_tokens: vec![],
        }
    }

    // --------------------------------------------------
    // Instantiate
    // --------------------------------------------------
    #[test]
    fn test_instantiate() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);
        let info = get_message_info(&api, ADMIN, &[]);

        let res = instantiate(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "instantiate");
        assert_eq!(res.attributes[1].value, "1"); // admin_count
        assert_eq!(res.attributes[2].value, "1"); // executor_count
        assert_eq!(res.attributes[3].value, "1"); // depositor_count
        assert_eq!(res.attributes[4].value, "1"); // token_count

        let executors = EXECUTORS.load(deps.as_ref().storage).unwrap();
        assert_eq!(executors.len(), 1);

        // Verify token was registered
        let token_config = TOKEN_REGISTRY.load(deps.as_ref().storage, "datom").unwrap();
        assert_eq!(token_config.denom, "factory/drop/datom");
        assert_eq!(token_config.base_asset_denom, "uatom");
        assert!(token_config.enabled);

        // Verify depositor was registered
        let depositor = WHITELISTED_DEPOSITORS
            .load(deps.as_ref().storage, &api.addr_make(DEPOSITOR))
            .unwrap();
        assert!(depositor.enabled);
    }

    #[test]
    fn test_instantiate_fails_with_no_admins() {
        let (mut deps, api, env) = setup();
        let mut msg = get_default_instantiate_msg(&api);
        msg.initial_admins = vec![];
        let info = get_message_info(&api, ADMIN, &[]);

        let err = instantiate(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::AtLeastOneAdmin {}));
    }

    #[test]
    fn test_instantiate_fails_with_no_executors() {
        let (mut deps, api, env) = setup();
        let mut msg = get_default_instantiate_msg(&api);
        msg.initial_executors = vec![];
        let info = get_message_info(&api, ADMIN, &[]);

        let err = instantiate(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::AtLeastOneExecutor {}));
    }

    #[test]
    fn test_instantiate_minimal() {
        let (mut deps, api, env) = setup();
        let msg = get_minimal_instantiate_msg(&api);
        let info = get_message_info(&api, ADMIN, &[]);

        let res = instantiate(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "instantiate");
    }

    // --------------------------------------------------
    // Token Registration (Admin)
    // --------------------------------------------------
    #[test]
    fn test_register_token() {
        let (mut deps, api, env) = setup();
        let msg = get_minimal_instantiate_msg(&api);
        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let res = execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::RegisterToken {
                symbol: "dntrn".to_string(),
                denom: "factory/drop/dntrn".to_string(),
                drop_staking_core: api.addr_make("ntrn_staking_core").to_string(),
                drop_voucher: api.addr_make("ntrn_voucher").to_string(),
                drop_withdrawal_manager: api.addr_make("ntrn_withdraw_manager").to_string(),
                base_asset_denom: "untrn".to_string(),
            }),
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "register_token");
        assert_eq!(res.attributes[1].value, "dntrn");
        assert_eq!(res.attributes[2].value, "factory/drop/dntrn");
        assert_eq!(res.attributes[3].value, "untrn"); // base_asset_denom

        // Verify token was registered
        let token_config = TOKEN_REGISTRY.load(deps.as_ref().storage, "dntrn").unwrap();
        assert_eq!(token_config.denom, "factory/drop/dntrn");
        assert!(token_config.enabled);
    }

    #[test]
    fn test_register_token_fails_if_already_registered() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);
        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Try to register datom again
        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::RegisterToken {
                symbol: "datom".to_string(),
                denom: "factory/drop/datom".to_string(),
                drop_staking_core: api.addr_make("staking_core").to_string(),
                drop_voucher: api.addr_make("voucher").to_string(),
                drop_withdrawal_manager: api.addr_make("withdraw_manager").to_string(),
                base_asset_denom: "uatom".to_string(),
            }),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::TokenAlreadyRegistered { symbol } if symbol == "datom"
        ));
    }

    #[test]
    fn test_register_token_fails_for_non_admin() {
        let (mut deps, api, env) = setup();
        let msg = get_minimal_instantiate_msg(&api);
        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, RANDOM, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::RegisterToken {
                symbol: "dntrn".to_string(),
                denom: "factory/drop/dntrn".to_string(),
                drop_staking_core: api.addr_make("ntrn_staking_core").to_string(),
                drop_voucher: api.addr_make("ntrn_voucher").to_string(),
                drop_withdrawal_manager: api.addr_make("ntrn_withdraw_manager").to_string(),
                base_asset_denom: "untrn".to_string(),
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::UnauthorizedAdmin {}));
    }

    #[test]
    fn test_unregister_token() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);
        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let res = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::UnregisterToken {
                symbol: "datom".to_string(),
            }),
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "unregister_token");
        assert_eq!(res.attributes[1].value, "datom");
        assert_eq!(res.attributes[2].value, "factory/drop/datom"); // denom

        // Verify token was removed
        assert!(!TOKEN_REGISTRY.has(deps.as_ref().storage, "datom"));
    }

    #[test]
    fn test_set_token_enabled() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);
        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Disable token
        let res = execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::SetTokenEnabled {
                symbol: "datom".to_string(),
                enabled: false,
            }),
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "set_token_enabled");

        let token_config = TOKEN_REGISTRY.load(deps.as_ref().storage, "datom").unwrap();
        assert!(!token_config.enabled);

        // Re-enable token
        execute(
            deps.as_mut(),
            env,
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::SetTokenEnabled {
                symbol: "datom".to_string(),
                enabled: true,
            }),
        )
        .unwrap();

        let token_config = TOKEN_REGISTRY.load(deps.as_ref().storage, "datom").unwrap();
        assert!(token_config.enabled);
    }

    // --------------------------------------------------
    // Depositor Management (Admin)
    // --------------------------------------------------
    #[test]
    fn test_register_depositor() {
        let (mut deps, api, env) = setup();
        let msg = get_minimal_instantiate_msg(&api);
        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let res = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::RegisterDepositor {
                depositor_address: api.addr_make("vault").to_string(),
                metadata: None,
            }),
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "register_depositor");

        let depositor = WHITELISTED_DEPOSITORS
            .load(deps.as_ref().storage, &api.addr_make("vault"))
            .unwrap();
        assert!(depositor.enabled);
    }

    #[test]
    fn test_register_depositor_fails_if_already_registered() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);
        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::RegisterDepositor {
                depositor_address: api.addr_make(DEPOSITOR).to_string(),
                metadata: None,
            }),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::DepositorAlreadyRegistered { .. }
        ));
    }

    #[test]
    fn test_unregister_depositor() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);
        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let res = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::UnregisterDepositor {
                depositor_address: api.addr_make(DEPOSITOR).to_string(),
            }),
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "unregister_depositor");
        assert!(!WHITELISTED_DEPOSITORS.has(deps.as_ref().storage, &api.addr_make(DEPOSITOR)));
    }

    #[test]
    fn test_set_depositor_enabled() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);
        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Disable depositor
        execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
                depositor_address: api.addr_make(DEPOSITOR).to_string(),
                enabled: false,
            }),
        )
        .unwrap();

        let depositor = WHITELISTED_DEPOSITORS
            .load(deps.as_ref().storage, &api.addr_make(DEPOSITOR))
            .unwrap();
        assert!(!depositor.enabled);
    }

    // --------------------------------------------------
    // Authorization
    // --------------------------------------------------
    #[test]
    fn test_only_executor_can_unbond() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, RANDOM, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::UnbondInDrop {
                symbol: "datom".to_string(),
                amount: None,
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::UnauthorizedExecutor {}));
    }

    #[test]
    fn test_only_depositor_can_deposit() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(
                &api,
                RANDOM,
                &[Coin {
                    denom: "factory/drop/datom".to_string(),
                    amount: Uint128::new(100),
                }],
            ),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {}),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::DepositorNotWhitelisted { .. }));
    }

    #[test]
    fn test_disabled_depositor_cannot_deposit() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Disable depositor
        execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
                depositor_address: api.addr_make(DEPOSITOR).to_string(),
                enabled: false,
            }),
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(
                &api,
                DEPOSITOR,
                &[Coin {
                    denom: "factory/drop/datom".to_string(),
                    amount: Uint128::new(100),
                }],
            ),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {}),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::DepositorDisabled { .. }));
    }

    // --------------------------------------------------
    // Deposit (StandardAction)
    // --------------------------------------------------
    #[test]
    fn test_deposit_registered_token() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let res = execute(
            deps.as_mut(),
            env,
            get_message_info(
                &api,
                DEPOSITOR,
                &[Coin {
                    denom: "factory/drop/datom".to_string(),
                    amount: Uint128::new(100),
                }],
            ),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {}),
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "deposit");
        assert_eq!(res.attributes[2].value, "datom"); // symbol
        assert_eq!(res.attributes[4].value, "100"); // amount
    }

    #[test]
    fn test_deposit_unregistered_token_fails() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(
                &api,
                DEPOSITOR,
                &[Coin {
                    denom: "unknown_token".to_string(),
                    amount: Uint128::new(100),
                }],
            ),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {}),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::TokenNotRegistered { .. }));
    }

    #[test]
    fn test_deposit_disabled_token_fails() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Disable token
        execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::SetTokenEnabled {
                symbol: "datom".to_string(),
                enabled: false,
            }),
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(
                &api,
                DEPOSITOR,
                &[Coin {
                    denom: "factory/drop/datom".to_string(),
                    amount: Uint128::new(100),
                }],
            ),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {}),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::TokenDisabled { .. }));
    }

    #[test]
    fn test_deposit_invalid_funds_fails() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // No funds
        let err = execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, DEPOSITOR, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {}),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::Payment(PaymentError::NoFunds {})
        ));

        // Multiple funds
        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(
                &api,
                DEPOSITOR,
                &[
                    Coin {
                        denom: "factory/drop/datom".to_string(),
                        amount: Uint128::new(100),
                    },
                    Coin {
                        denom: "uatom".to_string(),
                        amount: Uint128::new(50),
                    },
                ],
            ),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {}),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::Payment(PaymentError::MultipleDenoms {})
        ));
    }

    // --------------------------------------------------
    // Withdraw (StandardAction)
    // --------------------------------------------------
    #[test]
    fn test_standard_withdraw() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Set up balance
        deps.querier.bank.update_balance(
            MOCK_CONTRACT_ADDR,
            vec![Coin {
                denom: "uatom".to_string(),
                amount: Uint128::new(1000),
            }],
        );

        let res = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, DEPOSITOR, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
                coin: Coin {
                    denom: "uatom".to_string(),
                    amount: Uint128::new(500),
                },
            }),
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "withdraw");
        assert_eq!(res.messages.len(), 1);
        assert!(matches!(
            res.messages[0].msg,
            CosmosMsg::Bank(BankMsg::Send { .. })
        ));
    }

    #[test]
    fn test_standard_withdraw_insufficient_balance_fails() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Set up small balance
        deps.querier.bank.update_balance(
            MOCK_CONTRACT_ADDR,
            vec![Coin {
                denom: "uatom".to_string(),
                amount: Uint128::new(100),
            }],
        );

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, DEPOSITOR, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
                coin: Coin {
                    denom: "uatom".to_string(),
                    amount: Uint128::new(500),
                },
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InsufficientBalance {}));
    }

    // --------------------------------------------------
    // Unbond
    // --------------------------------------------------
    #[test]
    fn test_unbond_full_balance() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Simulate datom balance
        deps.querier.bank.update_balance(
            MOCK_CONTRACT_ADDR,
            vec![Coin {
                denom: "factory/drop/datom".to_string(),
                amount: Uint128::new(100),
            }],
        );

        let res = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, EXECUTOR, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::UnbondInDrop {
                symbol: "datom".to_string(),
                amount: None,
            }),
        )
        .unwrap();

        assert_eq!(res.messages.len(), 1);
        assert_eq!(res.attributes[0].value, "unbond");
        assert_eq!(res.attributes[1].value, "datom");
        assert_eq!(res.attributes[3].value, "100"); // full balance
    }

    #[test]
    fn test_unbond_partial_amount() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Simulate datom balance
        deps.querier.bank.update_balance(
            MOCK_CONTRACT_ADDR,
            vec![Coin {
                denom: "factory/drop/datom".to_string(),
                amount: Uint128::new(100),
            }],
        );

        let res = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, EXECUTOR, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::UnbondInDrop {
                symbol: "datom".to_string(),
                amount: Some(Uint128::new(50)),
            }),
        )
        .unwrap();

        assert_eq!(res.messages.len(), 1);
        assert_eq!(res.attributes[3].value, "50"); // partial amount
    }

    #[test]
    fn test_unbond_amount_exceeds_balance_fails() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Simulate small datom balance
        deps.querier.bank.update_balance(
            MOCK_CONTRACT_ADDR,
            vec![Coin {
                denom: "factory/drop/datom".to_string(),
                amount: Uint128::new(50),
            }],
        );

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, EXECUTOR, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::UnbondInDrop {
                symbol: "datom".to_string(),
                amount: Some(Uint128::new(100)),
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InsufficientBalance {}));
    }

    #[test]
    fn test_unbond_no_funds_fails() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // No balance set

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, EXECUTOR, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::UnbondInDrop {
                symbol: "datom".to_string(),
                amount: None,
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::NoFundsToUnbond {}));
    }

    #[test]
    fn test_unbond_unregistered_symbol_fails() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, EXECUTOR, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::UnbondInDrop {
                symbol: "unknown".to_string(),
                amount: None,
            }),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::TokenNotRegisteredBySymbol { .. }
        ));
    }

    #[test]
    fn test_unbond_disabled_token_fails() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Disable token
        execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::SetTokenEnabled {
                symbol: "datom".to_string(),
                enabled: false,
            }),
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, EXECUTOR, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::UnbondInDrop {
                symbol: "datom".to_string(),
                amount: None,
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::TokenDisabled { .. }));
    }

    // --------------------------------------------------
    // WithdrawFromDrop
    // --------------------------------------------------
    #[test]
    fn test_withdraw_from_drop() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let res = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, EXECUTOR, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::WithdrawFromDrop {
                symbol: "datom".to_string(),
                token_id: "123".to_string(),
            }),
        )
        .unwrap();

        assert_eq!(res.messages.len(), 1);
        assert_eq!(res.attributes[0].value, "withdraw_from_drop");
        assert_eq!(res.attributes[1].value, "datom");
        assert_eq!(res.attributes[2].value, "123");
        assert_eq!(res.attributes[3].value, "uatom"); // base_asset_denom
    }

    #[test]
    fn test_withdraw_from_drop_unregistered_symbol_fails() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, EXECUTOR, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::WithdrawFromDrop {
                symbol: "unknown".to_string(),
                token_id: "123".to_string(),
            }),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::TokenNotRegisteredBySymbol { .. }
        ));
    }

    // --------------------------------------------------
    // AddExecutor / RemoveExecutor
    // --------------------------------------------------
    #[test]
    fn test_add_executor() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let res = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::AddExecutor {
                executor_address: api.addr_make("new_executor").to_string(),
            }),
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "add_executor");
        assert_eq!(
            res.attributes[1].value,
            api.addr_make("new_executor").to_string()
        );
        assert_eq!(res.attributes[2].value, "2"); // executor_count

        let executors = EXECUTORS.load(deps.as_ref().storage).unwrap();
        assert_eq!(executors.len(), 2);
        assert!(executors.contains(&api.addr_make("new_executor")));
    }

    #[test]
    fn test_add_executor_already_exists_fails() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::AddExecutor {
                executor_address: api.addr_make(EXECUTOR).to_string(),
            }),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::ExecutorAlreadyExists { address } if address == api.addr_make(EXECUTOR).to_string()
        ));
    }

    #[test]
    fn test_remove_executor() {
        let (mut deps, api, env) = setup();
        let mut msg = get_default_instantiate_msg(&api);
        // Start with two executors
        msg.initial_executors
            .push(api.addr_make("executor2").to_string());

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let res = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::RemoveExecutor {
                executor_address: api.addr_make(EXECUTOR).to_string(),
            }),
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "remove_executor");
        assert_eq!(res.attributes[1].value, api.addr_make(EXECUTOR).to_string());
        assert_eq!(res.attributes[2].value, "1"); // executor_count

        let executors = EXECUTORS.load(deps.as_ref().storage).unwrap();
        assert_eq!(executors.len(), 1);
        assert!(!executors.contains(&api.addr_make(EXECUTOR)));
    }

    #[test]
    fn test_remove_executor_not_found_fails() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::RemoveExecutor {
                executor_address: api.addr_make("nonexistent").to_string(),
            }),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::ExecutorNotFound { address } if address == api.addr_make("nonexistent").to_string()
        ));
    }

    #[test]
    fn test_remove_last_executor_fails() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::RemoveExecutor {
                executor_address: api.addr_make(EXECUTOR).to_string(),
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::AtLeastOneExecutor {}));
    }

    // --------------------------------------------------
    // Query tests
    // --------------------------------------------------
    #[test]
    fn test_query_config() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let res = query(
            deps.as_ref(),
            env,
            QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::Config {}),
        )
        .unwrap();
        let config: ConfigResponse = from_json(&res).unwrap();

        assert_eq!(config.admins, vec![api.addr_make(ADMIN).to_string()]);
    }

    #[test]
    fn test_query_executors() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let res = query(
            deps.as_ref(),
            env,
            QueryMsg::CustomQuery(DAssetAdapterQueryMsg::Executors {}),
        )
        .unwrap();
        let executors: ExecutorsResponse = from_json(&res).unwrap();

        assert_eq!(
            executors.executors,
            vec![api.addr_make(EXECUTOR).to_string()]
        );
    }

    #[test]
    fn test_query_token_config() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let res = query(
            deps.as_ref(),
            env,
            QueryMsg::CustomQuery(DAssetAdapterQueryMsg::TokenConfig {
                symbol: "datom".to_string(),
            }),
        )
        .unwrap();
        let config: TokenConfigResponse = from_json(&res).unwrap();

        assert_eq!(config.symbol, "datom");
        assert_eq!(config.denom, "factory/drop/datom");
        assert_eq!(config.base_asset_denom, "uatom");
        assert!(config.enabled);
    }

    #[test]
    fn test_query_all_tokens() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Register another token
        execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::RegisterToken {
                symbol: "dntrn".to_string(),
                denom: "factory/drop/dntrn".to_string(),
                drop_staking_core: api.addr_make("ntrn_staking_core").to_string(),
                drop_voucher: api.addr_make("ntrn_voucher").to_string(),
                drop_withdrawal_manager: api.addr_make("ntrn_withdraw_manager").to_string(),
                base_asset_denom: "untrn".to_string(),
            }),
        )
        .unwrap();

        let res = query(
            deps.as_ref(),
            env,
            QueryMsg::CustomQuery(DAssetAdapterQueryMsg::AllTokens {}),
        )
        .unwrap();
        let tokens: TokensResponse = from_json(&res).unwrap();

        assert_eq!(tokens.tokens.len(), 2);
    }

    #[test]
    fn test_query_registered_depositors() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let res = query(
            deps.as_ref(),
            env,
            QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::RegisteredDepositors {
                enabled: None,
            }),
        )
        .unwrap();
        let depositors: RegisteredDepositorsResponse = from_json(&res).unwrap();

        assert_eq!(depositors.depositors.len(), 1);
        assert_eq!(
            depositors.depositors[0].depositor_address,
            api.addr_make(DEPOSITOR).to_string()
        );
        assert!(depositors.depositors[0].enabled);
    }

    // --------------------------------------------------
    // Phase 1: Missing Critical Authorization Tests
    // --------------------------------------------------

    #[test]
    fn test_withdraw_fails_for_non_depositor() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Set up balance
        deps.querier.bank.update_balance(
            MOCK_CONTRACT_ADDR,
            vec![Coin {
                denom: "uatom".to_string(),
                amount: Uint128::new(1000),
            }],
        );

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, RANDOM, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
                coin: Coin {
                    denom: "uatom".to_string(),
                    amount: Uint128::new(500),
                },
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::DepositorNotWhitelisted { .. }));
    }

    #[test]
    fn test_withdraw_fails_for_disabled_depositor() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Set up balance
        deps.querier.bank.update_balance(
            MOCK_CONTRACT_ADDR,
            vec![Coin {
                denom: "uatom".to_string(),
                amount: Uint128::new(1000),
            }],
        );

        // Disable depositor
        execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
                depositor_address: api.addr_make(DEPOSITOR).to_string(),
                enabled: false,
            }),
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, DEPOSITOR, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
                coin: Coin {
                    denom: "uatom".to_string(),
                    amount: Uint128::new(500),
                },
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::DepositorDisabled { .. }));
    }

    #[test]
    fn test_withdraw_from_drop_disabled_token_fails() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Disable token
        execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::SetTokenEnabled {
                symbol: "datom".to_string(),
                enabled: false,
            }),
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, EXECUTOR, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::WithdrawFromDrop {
                symbol: "datom".to_string(),
                token_id: "123".to_string(),
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::TokenDisabled { .. }));
    }

    #[test]
    fn test_only_executor_can_withdraw_from_drop() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, RANDOM, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::WithdrawFromDrop {
                symbol: "datom".to_string(),
                token_id: "123".to_string(),
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::UnauthorizedExecutor {}));
    }

    // --------------------------------------------------
    // Phase 2: Admin Authorization Tests
    // --------------------------------------------------

    #[test]
    fn test_unregister_token_fails_for_non_admin() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, RANDOM, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::UnregisterToken {
                symbol: "datom".to_string(),
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::UnauthorizedAdmin {}));
    }

    #[test]
    fn test_set_token_enabled_fails_for_non_admin() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, RANDOM, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::SetTokenEnabled {
                symbol: "datom".to_string(),
                enabled: false,
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::UnauthorizedAdmin {}));
    }

    #[test]
    fn test_add_executor_fails_for_non_admin() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, RANDOM, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::AddExecutor {
                executor_address: api.addr_make("new_executor").to_string(),
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::UnauthorizedAdmin {}));
    }

    #[test]
    fn test_remove_executor_fails_for_non_admin() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, RANDOM, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::RemoveExecutor {
                executor_address: api.addr_make(EXECUTOR).to_string(),
            }),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::UnauthorizedAdmin {}));
    }

    // --------------------------------------------------
    // Phase 3: Query Tests
    // --------------------------------------------------

    #[test]
    fn test_query_registered_depositors_enabled_filter() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Register a second depositor
        execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::RegisterDepositor {
                depositor_address: api.addr_make("depositor2").to_string(),
                metadata: None,
            }),
        )
        .unwrap();

        // Disable the second depositor
        execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
                depositor_address: api.addr_make("depositor2").to_string(),
                enabled: false,
            }),
        )
        .unwrap();

        // Query only enabled depositors
        let res = query(
            deps.as_ref(),
            env,
            QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::RegisteredDepositors {
                enabled: Some(true),
            }),
        )
        .unwrap();
        let depositors: RegisteredDepositorsResponse = from_json(&res).unwrap();

        // Should only return the original enabled depositor
        assert_eq!(depositors.depositors.len(), 1);
        assert_eq!(
            depositors.depositors[0].depositor_address,
            api.addr_make(DEPOSITOR).to_string()
        );
        assert!(depositors.depositors[0].enabled);
    }

    #[test]
    fn test_query_registered_depositors_disabled_filter() {
        let (mut deps, api, env) = setup();
        let msg = get_default_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        // Register a second depositor
        execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::RegisterDepositor {
                depositor_address: api.addr_make("depositor2").to_string(),
                metadata: None,
            }),
        )
        .unwrap();

        // Disable the second depositor
        execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            ExecuteMsg::StandardAction(AdapterInterfaceMsg::SetDepositorEnabled {
                depositor_address: api.addr_make("depositor2").to_string(),
                enabled: false,
            }),
        )
        .unwrap();

        // Query only disabled depositors
        let res = query(
            deps.as_ref(),
            env,
            QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::RegisteredDepositors {
                enabled: Some(false),
            }),
        )
        .unwrap();
        let depositors: RegisteredDepositorsResponse = from_json(&res).unwrap();

        // Should only return the disabled depositor
        assert_eq!(depositors.depositors.len(), 1);
        assert_eq!(
            depositors.depositors[0].depositor_address,
            api.addr_make("depositor2").to_string()
        );
        assert!(!depositors.depositors[0].enabled);
    }

    #[test]
    fn test_query_token_config_not_found() {
        let (mut deps, api, env) = setup();
        let msg = get_minimal_instantiate_msg(&api);

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            msg,
        )
        .unwrap();

        let err = query(
            deps.as_ref(),
            env,
            QueryMsg::CustomQuery(DAssetAdapterQueryMsg::TokenConfig {
                symbol: "nonexistent".to_string(),
            }),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::TokenNotRegisteredBySymbol { .. }
        ));
    }
}
