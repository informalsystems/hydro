#[cfg(test)]
mod custom_adapter_tests {
    use cosmwasm_std::testing::mock_env;
    use cosmwasm_std::{coins, from_json, MessageInfo, Uint128};

    use crate::contract::{execute, query};
    use crate::error::ContractError;
    use crate::msg::*;
    use crate::testing_mocks::{
        create_test_chain_config, setup_contract_with_chain, setup_contract_with_defaults,
    };

    // ============================================================================
    // EXECUTOR MANAGEMENT TESTS
    // ============================================================================

    #[test]
    fn test_add_executor_success() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let new_executor = deps.api.addr_make("new_executor");
        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddExecutor {
            executor_address: new_executor.to_string(),
        });

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "add_executor");
        assert_eq!(res.attributes[2].value, new_executor.to_string());
    }

    #[test]
    fn test_add_executor_unauthorized() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.non_admin.clone(),
            funds: vec![],
        };

        let new_executor = deps.api.addr_make("new_executor");
        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddExecutor {
            executor_address: new_executor.to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    #[test]
    fn test_add_executor_duplicate() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddExecutor {
            executor_address: test_data.executor.to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::ExecutorAlreadyExists { .. }));
    }

    #[test]
    fn test_remove_executor_success() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::RemoveExecutor {
            executor_address: test_data.executor.to_string(),
        });

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "remove_executor");
    }

    #[test]
    fn test_remove_executor_unauthorized() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.non_admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::RemoveExecutor {
            executor_address: test_data.executor.to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    #[test]
    fn test_remove_executor_not_found() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let non_existent = deps.api.addr_make("non_existent");
        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::RemoveExecutor {
            executor_address: non_existent.to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::ExecutorNotFound { .. }));
    }

    #[test]
    fn test_query_executors() {
        let (deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let msg = QueryMsg::CustomQuery(CctpAdapterQueryMsg::Executors {});
        let res = query(deps.as_ref(), env, msg).unwrap();
        let executors: ExecutorsResponse = from_json(&res).unwrap();

        assert_eq!(executors.executors.len(), 1);
        assert_eq!(
            executors.executors[0].executor_address,
            test_data.executor.to_string()
        );
    }

    // ============================================================================
    // ADMIN MANAGEMENT TESTS
    // ============================================================================

    #[test]
    fn test_add_admin_success() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddAdmin {
            admin_address: test_data.admin2.to_string(),
        });

        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "add_admin");
        assert_eq!(res.attributes[2].value, test_data.admin2.to_string());

        // Verify admin was added
        let query_msg = QueryMsg::CustomQuery(CctpAdapterQueryMsg::Admins {});
        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let admins: AdminsResponse = from_json(&res).unwrap();
        assert_eq!(admins.admins.len(), 2);
    }

    #[test]
    fn test_add_admin_unauthorized() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.non_admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddAdmin {
            admin_address: test_data.admin2.to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    #[test]
    fn test_add_admin_duplicate() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddAdmin {
            admin_address: test_data.admin.to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::AdminAlreadyExists { .. }));
    }

    #[test]
    fn test_remove_admin_success() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        // First add a second admin
        let add_info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let add_msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddAdmin {
            admin_address: test_data.admin2.to_string(),
        });
        execute(deps.as_mut(), env.clone(), add_info, add_msg).unwrap();

        // Now remove the first admin
        let remove_info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let remove_msg = ExecuteMsg::CustomAction(CctpAdapterMsg::RemoveAdmin {
            admin_address: test_data.admin.to_string(),
        });

        let res = execute(deps.as_mut(), env.clone(), remove_info, remove_msg).unwrap();
        assert_eq!(res.attributes[0].value, "remove_admin");

        // Verify admin was removed
        let query_msg = QueryMsg::CustomQuery(CctpAdapterQueryMsg::Admins {});
        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let admins: AdminsResponse = from_json(&res).unwrap();
        assert_eq!(admins.admins.len(), 1);
        assert_eq!(admins.admins[0], test_data.admin2.to_string());
    }

    #[test]
    fn test_remove_admin_unauthorized() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.non_admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::RemoveAdmin {
            admin_address: test_data.admin.to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    #[test]
    fn test_remove_admin_not_found() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::RemoveAdmin {
            admin_address: test_data.non_admin.to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::AdminNotFound { .. }));
    }

    #[test]
    fn test_remove_last_admin_fails() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::RemoveAdmin {
            admin_address: test_data.admin.to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::CannotRemoveLastAdmin {});
    }

    #[test]
    fn test_admin_self_removal() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        // Add second admin
        let add_info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let add_msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddAdmin {
            admin_address: test_data.admin2.to_string(),
        });
        execute(deps.as_mut(), env.clone(), add_info, add_msg).unwrap();

        // Admin removes themselves
        let remove_info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let remove_msg = ExecuteMsg::CustomAction(CctpAdapterMsg::RemoveAdmin {
            admin_address: test_data.admin.to_string(),
        });

        let res = execute(deps.as_mut(), env.clone(), remove_info, remove_msg).unwrap();
        assert_eq!(res.attributes[0].value, "remove_admin");

        // Verify they can no longer perform admin actions
        let try_add_info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };
        let try_add_msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddAdmin {
            admin_address: deps.api.addr_make("new_admin").to_string(),
        });

        let err = execute(deps.as_mut(), env, try_add_info, try_add_msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    #[test]
    fn test_query_admins() {
        let (deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let msg = QueryMsg::CustomQuery(CctpAdapterQueryMsg::Admins {});
        let res = query(deps.as_ref(), env, msg).unwrap();
        let admins: AdminsResponse = from_json(&res).unwrap();

        assert_eq!(admins.admins.len(), 1);
        assert_eq!(admins.admins[0], test_data.admin.to_string());
    }

    // ============================================================================
    // CHAIN MANAGEMENT TESTS
    // ============================================================================

    #[test]
    fn test_register_chain_success() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let chain_config = create_test_chain_config("ethereum");

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::RegisterChain { chain_config });

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "register_chain");
        assert_eq!(res.attributes[2].value, "ethereum");
    }

    #[test]
    fn test_register_chain_unauthorized() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.non_admin.clone(),
            funds: vec![],
        };

        let chain_config = create_test_chain_config("ethereum");

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::RegisterChain { chain_config });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedAdmin {});
    }

    #[test]
    fn test_register_chain_duplicate() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let chain_config = create_test_chain_config("ethereum");

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::RegisterChain { chain_config });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::ChainAlreadyRegistered { .. }));
    }

    #[test]
    fn test_update_registered_chain_success() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let mut chain_config = create_test_chain_config("ethereum");
        chain_config.bridging_config.destination_domain = 999;

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::UpdateRegisteredChain {
            chain_config: chain_config.clone(),
        });

        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "update_registered_chain");

        // Verify the update
        let query_msg = QueryMsg::CustomQuery(CctpAdapterQueryMsg::ChainConfig {
            chain_id: "ethereum".to_string(),
        });
        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let config: ChainConfigResponse = from_json(&res).unwrap();
        assert_eq!(config.chain_config.bridging_config.destination_domain, 999);
    }

    #[test]
    fn test_update_registered_chain_not_found() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        // Use a chain config that passes validation
        let chain_config = create_test_chain_config("non_existent_chain");

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::UpdateRegisteredChain { chain_config });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::ChainNotRegistered { .. }));
    }

    #[test]
    fn test_unregister_chain_success() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::UnregisterChain {
            chain_id: "ethereum".to_string(),
        });

        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "unregister_chain");

        // Verify chain was removed
        let query_msg = QueryMsg::CustomQuery(CctpAdapterQueryMsg::AllChains {});
        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let chains: AllChainsResponse = from_json(&res).unwrap();
        assert_eq!(chains.chains.len(), 0);
    }

    #[test]
    fn test_unregister_chain_not_found() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::UnregisterChain {
            chain_id: "non_existent".to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::ChainNotRegistered { .. }));
    }

    #[test]
    fn test_query_all_chains() {
        let (deps, _) = setup_contract_with_chain();
        let env = mock_env();

        let msg = QueryMsg::CustomQuery(CctpAdapterQueryMsg::AllChains {});
        let res = query(deps.as_ref(), env, msg).unwrap();
        let chains: AllChainsResponse = from_json(&res).unwrap();

        assert_eq!(chains.chains.len(), 1);
        assert_eq!(chains.chains[0].chain_id, "ethereum");
    }

    #[test]
    fn test_query_chain_config() {
        let (deps, _) = setup_contract_with_chain();
        let env = mock_env();

        let msg = QueryMsg::CustomQuery(CctpAdapterQueryMsg::ChainConfig {
            chain_id: "ethereum".to_string(),
        });
        let res = query(deps.as_ref(), env, msg).unwrap();
        let config: ChainConfigResponse = from_json(&res).unwrap();

        assert_eq!(config.chain_config.chain_id, "ethereum");
        assert_eq!(
            config.chain_config.bridging_config.noble_receiver,
            "noble15xt7kx5mles58vkkfxvf0lq78sw04jajvfgd4d"
        );
    }

    // ============================================================================
    // DESTINATION ADDRESS MANAGEMENT TESTS
    // ============================================================================

    #[test]
    fn test_add_allowed_destination_address_success() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddAllowedDestinationAddress {
            chain_id: "ethereum".to_string(),
            address: "0x1234567890123456789012345678901234567890".to_string(),
            protocol: "aave-v3".to_string(),
        });

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(res.attributes[0].value, "add_allowed_destination_address");
    }

    #[test]
    fn test_add_allowed_destination_address_normalized_to_lowercase() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        // Add address with mixed case
        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddAllowedDestinationAddress {
            chain_id: "ethereum".to_string(),
            address: "0xDACDBEEA12345678901234567890123456789012".to_string(), // Mixed case
            protocol: "compound-v3".to_string(),
        });

        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
        assert_eq!(res.attributes[0].value, "add_allowed_destination_address");
        // Verify the address attribute is lowercase
        assert_eq!(
            res.attributes[3].value,
            "dacdbeea12345678901234567890123456789012"
        );

        // Query to verify it's stored as lowercase
        let query_msg = QueryMsg::CustomQuery(CctpAdapterQueryMsg::AllowedDestinationAddresses {
            chain_id: "ethereum".to_string(),
            start_after: None,
            limit: None,
        });
        let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
        let addresses: AllowedDestinationAddressesResponse = from_json(&res).unwrap();

        // Find our newly added address
        let found = addresses
            .addresses
            .iter()
            .find(|a| a.address == "dacdbeea12345678901234567890123456789012");
        assert!(found.is_some());
        assert_eq!(found.unwrap().protocol, "compound-v3");

        // Now try to add the same address as lowercase - should fail with duplicate error
        let msg_lowercase =
            ExecuteMsg::CustomAction(CctpAdapterMsg::AddAllowedDestinationAddress {
                chain_id: "ethereum".to_string(),
                address: "0xdacdbeea12345678901234567890123456789012".to_string(), // All lowercase
                protocol: "aave-v3".to_string(),
            });

        let err = execute(deps.as_mut(), env, info, msg_lowercase).unwrap_err();
        assert!(
            matches!(err, ContractError::DestinationAddressAlreadyExists { .. }),
            "Expected MintRecipientAlreadyExists error, got: {:?}",
            err
        );
    }

    #[test]
    fn test_add_allowed_destination_address_chain_not_registered() {
        let (mut deps, test_data) = setup_contract_with_defaults();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddAllowedDestinationAddress {
            chain_id: "ethereum".to_string(),
            address: "0x1234567890123456789012345678901234567890".to_string(),
            protocol: "aave-v3".to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::ChainNotRegistered { .. }));
    }

    #[test]
    fn test_add_allowed_destination_address_duplicate() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddAllowedDestinationAddress {
            chain_id: "ethereum".to_string(),
            address: "0xabcd1234abcd1234abcd1234abcd1234abcd1234".to_string(),
            protocol: "uniswap-v3".to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(
            err,
            ContractError::DestinationAddressAlreadyExists { .. }
        ));
    }

    #[test]
    fn test_add_allowed_destination_address_case_insensitive_duplicate() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        // Try to add the same address with mixed case (should be normalized to lowercase and detected as duplicate)
        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::AddAllowedDestinationAddress {
            chain_id: "ethereum".to_string(),
            address: "0xAbCd1234AbCd1234AbCd1234AbCd1234AbCd1234".to_string(), // Mixed case
            protocol: "aave-v3".to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(
            err,
            ContractError::DestinationAddressAlreadyExists { .. }
        ));
    }

    #[test]
    fn test_remove_allowed_destination_address_success() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::RemoveAllowedDestinationAddress {
            chain_id: "ethereum".to_string(),
            address: "0xabcd1234abcd1234abcd1234abcd1234abcd1234".to_string(),
        });

        let res = execute(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(
            res.attributes[0].value,
            "remove_allowed_destination_address"
        );
    }

    #[test]
    fn test_remove_allowed_destination_address_not_found() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.admin.clone(),
            funds: vec![],
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::RemoveAllowedDestinationAddress {
            chain_id: "ethereum".to_string(),
            address: "0x1111111111111111111111111111111111111111".to_string(),
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(
            err,
            ContractError::DestinationAddressDoesNotExist { .. }
        ));
    }

    #[test]
    fn test_query_allowed_destination_addresses() {
        let (deps, _) = setup_contract_with_chain();
        let env = mock_env();

        let msg = QueryMsg::CustomQuery(CctpAdapterQueryMsg::AllowedDestinationAddresses {
            chain_id: "ethereum".to_string(),
            start_after: None,
            limit: None,
        });
        let res = query(deps.as_ref(), env, msg).unwrap();
        let addresses: AllowedDestinationAddressesResponse = from_json(&res).unwrap();

        assert_eq!(addresses.addresses.len(), 1);
        assert_eq!(
            addresses.addresses[0].address,
            "abcd1234abcd1234abcd1234abcd1234abcd1234"
        );
        assert_eq!(addresses.addresses[0].protocol, "uniswap-v3");
    }

    // ============================================================================
    // TRANSFER FUNDS TESTS
    // ============================================================================

    #[test]
    fn test_transfer_funds_success() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        // First deposit funds as depositor
        let deposit_info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: coins(2000, "ibc/usdc"),
        };
        let deposit_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        execute(deps.as_mut(), env.clone(), deposit_info, deposit_msg).unwrap();

        // Update querier to reflect balance
        deps.querier
            .bank
            .update_balance(env.contract.address.clone(), coins(2000, "ibc/usdc"));

        // Execute transfer as executor with bridging fee
        let transfer_info = MessageInfo {
            sender: test_data.executor.clone(),
            funds: coins(100, "ibc/usdc"), // Bridging fee
        };

        let transfer_msg = ExecuteMsg::CustomAction(CctpAdapterMsg::TransferFunds {
            amount: Uint128::new(1000),
            instructions: crate::state::TransferFundsInstructions {
                chain_id: "ethereum".to_string(),
                recipient: "0xabcd1234abcd1234abcd1234abcd1234abcd1234".to_string(),
            },
        });

        let res = execute(deps.as_mut(), env, transfer_info, transfer_msg).unwrap();
        assert_eq!(res.attributes[0].value, "transfer_funds");
        assert_eq!(res.messages.len(), 1); // IBC transfer message
    }

    #[test]
    fn test_transfer_funds_unauthorized() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.non_admin.clone(),
            funds: coins(100, "ibc/usdc"),
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::TransferFunds {
            amount: Uint128::new(1000),
            instructions: crate::state::TransferFundsInstructions {
                chain_id: "ethereum".to_string(),
                recipient: "0xabcd1234abcd1234abcd1234abcd1234abcd1234".to_string(),
            },
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::UnauthorizedExecutor {});
    }

    #[test]
    fn test_transfer_funds_zero_amount() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.executor.clone(),
            funds: coins(100, "ibc/usdc"),
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::TransferFunds {
            amount: Uint128::zero(),
            instructions: crate::state::TransferFundsInstructions {
                chain_id: "ethereum".to_string(),
                recipient: "0xabcd1234abcd1234abcd1234abcd1234abcd1234".to_string(),
            },
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::ZeroAmount {});
    }

    #[test]
    fn test_transfer_funds_insufficient_balance() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        let info = MessageInfo {
            sender: test_data.executor.clone(),
            funds: coins(100, "ibc/usdc"),
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::TransferFunds {
            amount: Uint128::new(10000), // More than available
            instructions: crate::state::TransferFundsInstructions {
                chain_id: "ethereum".to_string(),
                recipient: "0xabcd1234abcd1234abcd1234abcd1234abcd1234".to_string(),
            },
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::InsufficientBalance { .. }));
    }

    #[test]
    fn test_transfer_funds_chain_not_registered() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        // Deposit funds
        let deposit_info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: coins(2000, "ibc/usdc"),
        };
        let deposit_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        execute(deps.as_mut(), env.clone(), deposit_info, deposit_msg).unwrap();

        deps.querier
            .bank
            .update_balance(env.contract.address.clone(), coins(2000, "ibc/usdc"));

        let info = MessageInfo {
            sender: test_data.executor.clone(),
            funds: coins(100, "ibc/usdc"),
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::TransferFunds {
            amount: Uint128::new(1000),
            instructions: crate::state::TransferFundsInstructions {
                chain_id: "arbitrum".to_string(), // Not registered
                recipient: "0xabcd1234abcd1234abcd1234abcd1234abcd1234".to_string(),
            },
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::ChainNotRegistered { .. }));
    }

    #[test]
    fn test_transfer_funds_recipient_not_allowed() {
        let (mut deps, test_data) = setup_contract_with_chain();
        let env = mock_env();

        // Deposit funds
        let deposit_info = MessageInfo {
            sender: test_data.depositor.clone(),
            funds: coins(2000, "ibc/usdc"),
        };
        let deposit_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        execute(deps.as_mut(), env.clone(), deposit_info, deposit_msg).unwrap();

        deps.querier
            .bank
            .update_balance(env.contract.address.clone(), coins(2000, "ibc/usdc"));

        let info = MessageInfo {
            sender: test_data.executor.clone(),
            funds: coins(100, "ibc/usdc"),
        };

        let msg = ExecuteMsg::CustomAction(CctpAdapterMsg::TransferFunds {
            amount: Uint128::new(1000),
            instructions: crate::state::TransferFundsInstructions {
                chain_id: "ethereum".to_string(),
                recipient: "0x9999999999999999999999999999999999999999".to_string(), // Not in allowlist
            },
        });

        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(
            err,
            ContractError::DestinationAddressNotAllowed { .. }
        ));
    }
}
