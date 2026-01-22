#[cfg(test)]
mod tests {
    use crate::contract::{execute, instantiate, query, reply};
    use crate::error::ContractError;
    use crate::msg::{ConfigResponse, DAssetAdapterMsg, ExecuteMsg, InstantiateMsg, QueryMsg};
    use crate::state::{CONFIG, EXECUTORS};
    use cosmwasm_std::testing::{
        mock_dependencies, mock_env, MockApi, MockQuerier, MockStorage, MOCK_CONTRACT_ADDR,
    };
    use cosmwasm_std::{from_json, BankMsg, Coin, CosmosMsg, MessageInfo, Uint128};

    const ADMIN: &str = "admin";
    const EXECUTOR: &str = "executor";
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
            admins: vec![api.addr_make(ADMIN).to_string()],
            executors: vec![api.addr_make(EXECUTOR).to_string()],
            drop_staking_core: api.addr_make("staking_core").to_string(),
            drop_voucher: api.addr_make("voucher").to_string(),
            drop_withdrawal_manager: api.addr_make("withdraw_manager").to_string(),
            vault_contract: api.addr_make("vault").to_string(),
            liquid_asset_denom: "datom".to_string(),
            base_asset_denom: "uatom".to_string(),
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

        let executors = EXECUTORS.load(deps.as_ref().storage).unwrap();
        assert_eq!(executors.len(), 1);

        let config = CONFIG.load(deps.as_ref().storage).unwrap();
        assert_eq!(config.base_asset_denom, "uatom");
    }

    // --------------------------------------------------
    // Authorization
    // --------------------------------------------------
    #[test]
    fn test_only_executor_can_call() {
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
            ExecuteMsg::CustomAction(DAssetAdapterMsg::Unbond {}),
        )
        .unwrap_err();

        match err {
            ContractError::UnauthorizedExecutor {} => {}
            _ => panic!("expected UnauthorizedExecutor"),
        }
    }

    // --------------------------------------------------
    // Unbond
    // --------------------------------------------------
    #[test]
    fn test_unbond_creates_message() {
        let (mut deps, api, env) = setup();

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            get_default_instantiate_msg(&api),
        )
        .unwrap();

        // simulate datom balance
        deps.querier.bank.update_balance(
            MOCK_CONTRACT_ADDR,
            vec![Coin {
                denom: "datom".to_string(),
                amount: Uint128::new(100),
            }],
        );

        let res = execute(
            deps.as_mut(),
            env,
            get_message_info(&api, EXECUTOR, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::Unbond {}),
        )
        .unwrap();

        assert_eq!(res.messages.len(), 1);
        assert_eq!(res.attributes[0].value, "unbond");
    }

    // --------------------------------------------------
    // Withdraw
    // --------------------------------------------------
    #[test]
    fn test_withdraw_sends_atom_to_vault() {
        let (mut deps, api, env) = setup();

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            get_default_instantiate_msg(&api),
        )
        .unwrap();

        // simulate atom balance after withdraw reply
        deps.querier.bank.update_balance(
            MOCK_CONTRACT_ADDR,
            vec![Coin {
                denom: "uatom".to_string(),
                amount: Uint128::new(500),
            }],
        );

        let res = execute(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, EXECUTOR, &[]),
            ExecuteMsg::CustomAction(DAssetAdapterMsg::Withdraw {
                token_id: "1".to_string(),
            }),
        )
        .unwrap();

        // Withdraw only schedules a submessage; BankMsg is sent in reply
        assert_eq!(res.messages.len(), 1);
    }

    // --------------------------------------------------
    // Reply tests
    // --------------------------------------------------
    #[test]
    fn test_reply_withdraw_executes_bank_send() {
        let (mut deps, api, env) = setup();

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            get_default_instantiate_msg(&api),
        )
        .unwrap();

        // simulate that contract has uatom balance to send
        deps.querier.bank.update_balance(
            MOCK_CONTRACT_ADDR,
            vec![Coin {
                denom: "uatom".to_string(),
                amount: Uint128::new(1000),
            }],
        );

        let reply_id = 1;

        // Call reply
        let res = reply(
            deps.as_mut(),
            env,
            cosmwasm_std::Reply {
                id: reply_id,
                #[allow(deprecated)]
                result: cosmwasm_std::SubMsgResult::Ok(cosmwasm_std::SubMsgResponse {
                    events: vec![],
                    data: None,
                    msg_responses: vec![],
                }),
                gas_used: 0,
                payload: cosmwasm_std::Binary::default(),
            },
        )
        .unwrap();

        // Expect a bank send msg in reply
        assert!(res
            .messages
            .iter()
            .any(|m| matches!(m.msg, CosmosMsg::Bank(BankMsg::Send { .. }))));
    }

    #[test]
    fn test_reply_withdraw_fails_on_error() {
        let (mut deps, api, env) = setup();

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            get_default_instantiate_msg(&api),
        )
        .unwrap();

        // Call reply with error result
        let err = reply(
            deps.as_mut(),
            env,
            cosmwasm_std::Reply {
                id: 1,
                #[allow(deprecated)]
                result: cosmwasm_std::SubMsgResult::Err("withdrawal failed".to_string()),
                gas_used: 0,
                payload: cosmwasm_std::Binary::default(),
            },
        )
        .unwrap_err();

        match err {
            ContractError::WithdrawalFailed { .. } => {}
            _ => panic!("expected WithdrawalFailed error"),
        }
    }

    #[test]
    fn test_reply_withdraw_fails_on_zero_balance() {
        let (mut deps, api, env) = setup();

        instantiate(
            deps.as_mut(),
            env.clone(),
            get_message_info(&api, ADMIN, &[]),
            get_default_instantiate_msg(&api),
        )
        .unwrap();

        // No balance set - contract has zero uatom

        // Call reply with success but zero balance
        let err = reply(
            deps.as_mut(),
            env,
            cosmwasm_std::Reply {
                id: 1,
                #[allow(deprecated)]
                result: cosmwasm_std::SubMsgResult::Ok(cosmwasm_std::SubMsgResponse {
                    events: vec![],
                    data: None,
                    msg_responses: vec![],
                }),
                gas_used: 0,
                payload: cosmwasm_std::Binary::default(),
            },
        )
        .unwrap_err();

        match err {
            ContractError::NoFundsReceived {} => {}
            _ => panic!("expected NoFundsReceived error"),
        }
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
            msg.clone(),
        )
        .unwrap();

        let res = query(deps.as_ref(), env, QueryMsg::Config {}).unwrap();
        let config: ConfigResponse = from_json(&res).unwrap();

        assert_eq!(config.admins, vec![api.addr_make(ADMIN).to_string()]);
        assert_eq!(config.executors, vec![api.addr_make(EXECUTOR).to_string()]);
        assert_eq!(
            config.drop_staking_core,
            api.addr_make("staking_core").to_string()
        );
        assert_eq!(config.drop_voucher, api.addr_make("voucher").to_string());
        assert_eq!(
            config.drop_withdrawal_manager,
            api.addr_make("withdraw_manager").to_string()
        );
        assert_eq!(config.vault_contract, api.addr_make("vault").to_string());
        assert_eq!(config.liquid_asset_denom, "datom");
        assert_eq!(config.base_asset_denom, "uatom");
    }
}
