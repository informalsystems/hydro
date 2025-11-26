// Tests for Mars message JSON serialization

use cosmwasm_std::{Coin, Uint128};

use crate::mars::{Action, ActionAmount, ActionCoin, MarsExecuteMsg};

#[test]
fn test_action_json_serialization() {
    // Test Deposit action
    let deposit = Action::Deposit(Coin {
        denom: "uatom".to_string(),
        amount: Uint128::new(1000),
    });
    let json = cosmwasm_std::to_json_string(&deposit).unwrap();
    assert_eq!(json, r#"{"deposit":{"denom":"uatom","amount":"1000"}}"#);

    // Test Lend action with exact amount
    let lend = Action::Lend(ActionCoin {
        denom: "uatom".to_string(),
        amount: ActionAmount::Exact("1000".to_string()),
    });
    let json = cosmwasm_std::to_json_string(&lend).unwrap();
    assert_eq!(
        json,
        r#"{"lend":{"denom":"uatom","amount":{"exact":"1000"}}}"#
    );

    // Test Reclaim action
    let reclaim = Action::Reclaim(ActionCoin {
        denom: "uatom".to_string(),
        amount: ActionAmount::Exact("500".to_string()),
    });
    let json = cosmwasm_std::to_json_string(&reclaim).unwrap();
    assert_eq!(
        json,
        r#"{"reclaim":{"denom":"uatom","amount":{"exact":"500"}}}"#
    );

    // Test Withdraw action
    let withdraw = Action::Withdraw(ActionCoin {
        denom: "uatom".to_string(),
        amount: ActionAmount::Exact("250".to_string()),
    });
    let json = cosmwasm_std::to_json_string(&withdraw).unwrap();
    assert_eq!(
        json,
        r#"{"withdraw":{"denom":"uatom","amount":{"exact":"250"}}}"#
    );

    // Test WithdrawToWallet action
    let withdraw_to_wallet = Action::WithdrawToWallet {
        coin: ActionCoin {
            denom: "uatom".to_string(),
            amount: ActionAmount::Exact("100".to_string()),
        },
        recipient: "neutron1234".to_string(),
    };
    let json = cosmwasm_std::to_json_string(&withdraw_to_wallet).unwrap();
    assert_eq!(
        json,
        r#"{"withdraw_to_wallet":{"coin":{"denom":"uatom","amount":{"exact":"100"}},"recipient":"neutron1234"}}"#
    );

    // Test account_balance variant
    let lend_all = Action::Lend(ActionCoin {
        denom: "uatom".to_string(),
        amount: ActionAmount::AccountBalance,
    });
    let json = cosmwasm_std::to_json_string(&lend_all).unwrap();
    assert_eq!(
        json,
        r#"{"lend":{"denom":"uatom","amount":"account_balance"}}"#
    );
}

#[test]
fn test_deposit_lend_message_serialization() {
    let msg = MarsExecuteMsg::UpdateCreditAccount {
        account_id: "5696".to_string(),
        actions: vec![
            Action::Deposit(Coin {
                denom: "ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81"
                    .to_string(),
                amount: Uint128::new(12820000000),
            }),
            Action::Lend(ActionCoin {
                denom: "ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81"
                    .to_string(),
                amount: ActionAmount::Exact("12820000000".to_string()),
            }),
        ],
    };

    let json = cosmwasm_std::to_json_string(&msg).unwrap();
    let expected = r#"{"update_credit_account":{"account_id":"5696","actions":[{"deposit":{"denom":"ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81","amount":"12820000000"}},{"lend":{"denom":"ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81","amount":{"exact":"12820000000"}}}]}}"#;
    assert_eq!(json, expected);
}

#[test]
fn test_reclaim_withdraw_message_serialization() {
    let msg = MarsExecuteMsg::UpdateCreditAccount {
        account_id: "32168".to_string(),
        actions: vec![
            Action::Reclaim(ActionCoin {
                denom: "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2"
                    .to_string(),
                amount: ActionAmount::Exact("2500000000".to_string()),
            }),
            Action::Withdraw(ActionCoin {
                denom: "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2"
                    .to_string(),
                amount: ActionAmount::Exact("2500000000".to_string()),
            }),
        ],
    };

    let json = cosmwasm_std::to_json_string(&msg).unwrap();
    let expected = r#"{"update_credit_account":{"account_id":"32168","actions":[{"reclaim":{"denom":"ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2","amount":{"exact":"2500000000"}}},{"withdraw":{"denom":"ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2","amount":{"exact":"2500000000"}}}]}}"#;
    assert_eq!(json, expected);
}
