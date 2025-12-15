#[cfg(test)]
mod tests {
    use cosmwasm_std::testing::{mock_env, MockApi, MOCK_CONTRACT_ADDR};
    use cosmwasm_std::{coins, from_json, to_json_binary, Coin, CosmosMsg, MessageInfo, Uint128};
    use neutron_sdk::bindings::msg::NeutronMsg;

    use crate::contract::{execute, instantiate, query};
    use crate::error::ContractError;
    use crate::msg::{
        AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AvailableAmountResponse, ExecuteMsg,
        IbcAdapterMsg, InitialDepositor, InstantiateMsg, QueryMsg,
    };
    use crate::state::{
        ChainConfig, DepositorCapabilities, TokenConfig, TransferFundsInstructions,
    };
    use crate::testing_mocks::mock_dependencies;

    const ADMIN: &str = "admin";
    const DEPOSITOR: &str = "depositor";
    const DEPOSITOR_CANNOT_WITHDRAW: &str = "depositor_cannot_withdraw";
    const DENOM: &str = "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2";

    fn get_message_info(mock_api: &MockApi, sender: &str, funds: &[Coin]) -> MessageInfo {
        MessageInfo {
            sender: mock_api.addr_make(sender),
            funds: funds.to_vec(),
        }
    }

    fn get_default_instantiate_msg(mock_api: &MockApi) -> InstantiateMsg {
        let admin = mock_api.addr_make(ADMIN);
        let depositor = mock_api.addr_make(DEPOSITOR);
        let depositor_cannot_withdraw = mock_api.addr_make(DEPOSITOR_CANNOT_WITHDRAW);

        let chain_config = ChainConfig {
            chain_id: "osmosis-1".to_string(),
            channel_from_neutron: "channel-0".to_string(),
            allowed_recipients: vec![],
        };

        let capabilities_can_withdraw = DepositorCapabilities { can_withdraw: true };
        let capabilities_cannot_withdraw = DepositorCapabilities {
            can_withdraw: false,
        };

        InstantiateMsg {
            admins: vec![admin.to_string()],
            initial_executors: vec![],
            default_timeout_seconds: 600,
            initial_chains: vec![chain_config],
            initial_tokens: vec![TokenConfig {
                denom: DENOM.to_string(),
                source_chain_id: "osmosis-1".to_string(),
            }],
            initial_depositors: vec![
                InitialDepositor {
                    address: depositor.to_string(),
                    capabilities: Some(to_json_binary(&capabilities_can_withdraw).unwrap()),
                },
                InitialDepositor {
                    address: depositor_cannot_withdraw.to_string(),
                    capabilities: Some(to_json_binary(&capabilities_cannot_withdraw).unwrap()),
                },
            ],
        }
    }

    #[test]
    fn test_instantiate() {
        let mut deps = mock_dependencies();
        let msg = get_default_instantiate_msg(&deps.api);
        let info = get_message_info(&deps.api, ADMIN, &[]);
        let env = mock_env();

        let res = instantiate(deps.as_mut(), env, info, msg).unwrap();
        // Expect: action, contract_name, contract_version, admin_count, executor_count,
        // default_timeout_seconds, initial_chains_count, initial_tokens_count, depositor_registered
        assert_eq!(res.attributes.len(), 9);
        assert_eq!(res.attributes[0].value, "instantiate");
    }

    #[test]
    fn test_deposit_and_transfer_success() {
        let mut deps = mock_dependencies();
        let msg = get_default_instantiate_msg(&deps.api);
        let info = get_message_info(&deps.api, ADMIN, &[]);
        let env = mock_env();

        // Instantiate
        instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Step 1: Depositor deposits funds (no routing yet)
        let deposit_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        let info = get_message_info(&deps.api, DEPOSITOR, &coins(1000000, DENOM));

        let res = execute(deps.as_mut(), env.clone(), info, deposit_msg).unwrap();

        // Verify no messages created (funds just held)
        assert_eq!(res.messages.len(), 0);
        assert_eq!(res.attributes[0].value, "deposit");

        // Update mock balance to simulate contract received the funds
        deps.querier
            .bank
            .update_balance(MOCK_CONTRACT_ADDR, coins(1000000, DENOM));

        // Step 2: Admin routes funds via TransferFunds
        let transfer_msg = ExecuteMsg::CustomAction(IbcAdapterMsg::TransferFunds {
            coin: Coin {
                denom: DENOM.to_string(),
                amount: Uint128::new(1000000),
            },
            instructions: TransferFundsInstructions {
                destination_chain: "osmosis-1".to_string(),
                recipient: "osmo1recipient".to_string(),
                timeout_seconds: None,
            },
        });
        let info = get_message_info(&deps.api, ADMIN, &[]);

        let res = execute(deps.as_mut(), env.clone(), info, transfer_msg).unwrap();

        // Verify IBC transfer message was created
        assert_eq!(res.messages.len(), 1);

        // Check message content
        if let CosmosMsg::Custom(NeutronMsg::IbcTransfer {
            source_port,
            source_channel,
            token,
            sender,
            receiver,
            timeout_timestamp,
            memo,
            ..
        }) = &res.messages[0].msg
        {
            assert_eq!(source_port, "transfer");
            assert_eq!(source_channel, "channel-0");
            assert_eq!(token.denom, DENOM);
            assert_eq!(token.amount, Uint128::new(1000000));
            assert_eq!(sender, MOCK_CONTRACT_ADDR);
            assert_eq!(receiver, "osmo1recipient");
            assert_eq!(memo, "");
            assert!(*timeout_timestamp > 0);
        } else {
            panic!("Expected IbcTransfer message");
        }

        // Verify attributes
        assert_eq!(res.attributes[0].value, "transfer_funds");
    }

    #[test]
    fn test_deposit_unregistered_token_fails() {
        let mut deps = mock_dependencies();
        let msg = get_default_instantiate_msg(&deps.api);
        let info = get_message_info(&deps.api, ADMIN, &[]);
        let env = mock_env();

        instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

        let deposit_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Deposit {});
        let info = get_message_info(&deps.api, DEPOSITOR, &coins(1000000, "unknown_denom"));

        let err = execute(deps.as_mut(), env, info, deposit_msg).unwrap_err();
        match err {
            ContractError::TokenNotRegistered { .. } => {}
            _ => panic!("Expected TokenNotRegistered error"),
        }
    }

    #[test]
    fn test_withdraw_success() {
        let mut deps = mock_dependencies();

        // Setup mock querier to return balance
        deps.querier
            .bank
            .update_balance(MOCK_CONTRACT_ADDR, coins(2000000, DENOM));

        let msg = get_default_instantiate_msg(&deps.api);
        let info = get_message_info(&deps.api, ADMIN, &[]);
        let env = mock_env();

        instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

        let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
            coin: Coin {
                denom: DENOM.to_string(),
                amount: Uint128::new(1000000),
            },
        });
        let info = get_message_info(&deps.api, DEPOSITOR, &[]);

        let res = execute(deps.as_mut(), env, info, withdraw_msg).unwrap();

        // Verify bank send message
        assert_eq!(res.messages.len(), 1);
        assert_eq!(res.attributes[0].value, "withdraw");
    }

    #[test]
    fn test_withdraw_fails_when_cannot_withdraw() {
        let mut deps = mock_dependencies();

        // Setup mock querier to return balance
        deps.querier
            .bank
            .update_balance(MOCK_CONTRACT_ADDR, coins(2000000, DENOM));

        let msg = get_default_instantiate_msg(&deps.api);
        let info = get_message_info(&deps.api, ADMIN, &[]);
        let env = mock_env();

        instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Try to withdraw with depositor who cannot withdraw - should fail
        let withdraw_msg = ExecuteMsg::StandardAction(AdapterInterfaceMsg::Withdraw {
            coin: Coin {
                denom: DENOM.to_string(),
                amount: Uint128::new(1000000),
            },
        });
        let info = get_message_info(&deps.api, DEPOSITOR_CANNOT_WITHDRAW, &[]);

        let err = execute(deps.as_mut(), env, info, withdraw_msg).unwrap_err();

        // Verify it's the WithdrawalNotAllowed error
        match err {
            ContractError::WithdrawalNotAllowed {} => {}
            _ => panic!("Expected WithdrawalNotAllowed error, got: {:?}", err),
        }
    }

    #[test]
    fn test_query_available_for_deposit() {
        let mut deps = mock_dependencies();
        let msg = get_default_instantiate_msg(&deps.api);
        let info = get_message_info(&deps.api, ADMIN, &[]);
        let env = mock_env();

        instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

        let query_msg = QueryMsg::StandardQuery(AdapterInterfaceQueryMsg::AvailableForDeposit {
            depositor_address: DEPOSITOR.to_string(),
            denom: DENOM.to_string(),
        });

        let res = query(deps.as_ref(), env, query_msg).unwrap();
        let response: AvailableAmountResponse = from_json(&res).unwrap();

        // Should return MAX for IBC adapter
        assert_eq!(response.amount, Uint128::MAX);
    }
}
