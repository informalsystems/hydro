use std::{collections::HashMap, marker::PhantomData};

use cosmwasm_std::{
    from_json,
    testing::{mock_env, MockApi, MockQuerier, MockStorage},
    to_json_binary, Addr, Coin, ContractResult, Decimal, Env, MessageInfo, OwnedDeps,
    QuerierResult, Response, SystemError, SystemResult, Uint128, WasmQuery,
};
use interface::inflow_control_center::{
    ExecuteMsg, FeeAccrualInfoResponse, FeeConfigInit, FeeConfigResponse, QueryMsg,
};
use interface::inflow_vault::{
    PoolInfoResponse as VaultPoolInfoResponse, QueryMsg as VaultQueryMsg,
};
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

use crate::{
    contract::{execute, instantiate, query},
    msg::InstantiateMsg,
    state::{FEE_CONFIG, HIGH_WATER_MARK_PRICE},
};

const WHITELIST: &str = "whitelist1";
const USER1: &str = "user1";
const SUBVAULT1: &str = "subvault1";
const SUBVAULT2: &str = "subvault2";
const TREASURY: &str = "treasury";
const DEFAULT_DEPOSIT_CAP: Uint128 = Uint128::new(10000000);

type WasmQueryFunc = Box<dyn Fn(&WasmQuery) -> QuerierResult>;

pub fn mock_dependencies() -> OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery> {
    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
        querier: MockQuerier::new(&[]),
        custom_query_type: PhantomData,
    }
}

pub fn get_message_info(api: &MockApi, sender: &str, funds: &[Coin]) -> MessageInfo {
    MessageInfo {
        sender: api.addr_make(sender),
        funds: funds.to_vec(),
    }
}

fn get_instantiate_msg(
    deposit_cap: Uint128,
    whitelist_addr: Addr,
    subvaults: Vec<Addr>,
    fee_config: Option<FeeConfigInit>,
) -> InstantiateMsg {
    InstantiateMsg {
        deposit_cap,
        whitelist: vec![whitelist_addr.to_string()],
        subvaults: subvaults
            .iter()
            .map(|subvault_addr| subvault_addr.to_string())
            .collect(),
        fee_config,
    }
}

/// Sets up a mock querier that handles subvault PoolInfo queries
fn setup_mock_querier_with_subvaults(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
    subvault_shares: Vec<(String, Uint128)>,
) {
    let handlers: HashMap<String, WasmQueryFunc> = subvault_shares
        .into_iter()
        .map(|(addr, shares)| {
            let handler: WasmQueryFunc = Box::new(move |query: &WasmQuery| match query {
                WasmQuery::Smart { msg, .. } => {
                    let _query_msg: VaultQueryMsg = from_json(msg).unwrap();
                    // All queries return the same pool info for simplicity
                    let response = to_json_binary(&VaultPoolInfoResponse {
                        shares_issued: shares,
                        balance_base_tokens: shares, // 1:1 for simplicity
                        adapter_deposits_base_tokens: Uint128::zero(),
                        withdrawal_queue_base_tokens: Uint128::zero(),
                    })
                    .unwrap();
                    SystemResult::Ok(ContractResult::Ok(response))
                }
                _ => SystemResult::Err(SystemError::UnsupportedRequest {
                    kind: "only smart queries supported".to_string(),
                }),
            });
            (addr, handler)
        })
        .collect();

    deps.querier.update_wasm(move |query| {
        let contract_addr = match query {
            WasmQuery::Smart { contract_addr, .. } => contract_addr.clone(),
            _ => {
                return SystemResult::Err(SystemError::UnsupportedRequest {
                    kind: "unsupported query type".to_string(),
                })
            }
        };

        if let Some(handler) = handlers.get(&contract_addr) {
            (handler)(query)
        } else {
            SystemResult::Err(SystemError::NoSuchContract {
                addr: contract_addr,
            })
        }
    });
}

// ============================================================================
// Initialization Tests
// ============================================================================

#[test]
fn test_instantiate_with_fee_config() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    let res = instantiate(deps.as_mut(), env.clone(), info, instantiate_msg);
    assert!(res.is_ok());

    // Verify fee config was stored
    let fee_config = FEE_CONFIG.load(&deps.storage).unwrap();
    assert_eq!(fee_config.fee_rate, Decimal::percent(20));
    assert_eq!(fee_config.fee_recipient, treasury_addr);

    // Verify high-water mark was initialized
    let high_water_mark_price = HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap();
    assert_eq!(high_water_mark_price, Decimal::one());
}

#[test]
fn test_instantiate_without_fee_config() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);

    let instantiate_msg = get_instantiate_msg(DEFAULT_DEPOSIT_CAP, whitelist_addr, vec![], None);

    let info = get_message_info(&deps.api, "creator", &[]);
    let res = instantiate(deps.as_mut(), env.clone(), info, instantiate_msg);
    assert!(res.is_ok());

    // Verify fee config was stored with defaults (fee_rate = 0 means disabled)
    let fee_config = FEE_CONFIG.load(&deps.storage).unwrap();
    assert_eq!(fee_config.fee_rate, Decimal::zero());

    // Verify high-water mark was initialized
    let high_water_mark_price = HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap();
    assert_eq!(high_water_mark_price, Decimal::one());
}

#[test]
fn test_instantiate_invalid_fee_rate() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);

    // Try to instantiate with fee_rate > 100%
    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(150), // Invalid: > 100%
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    let res = instantiate(deps.as_mut(), env.clone(), info, instantiate_msg);
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("Invalid fee rate"));
}

// ============================================================================
// Fee Config Update Tests
// ============================================================================

#[test]
fn test_update_fee_config_partial() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);

    // Instantiate with initial fee config
    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr.clone(),
        vec![],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Update only the fee rate
    let info = get_message_info(&deps.api, WHITELIST, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdateFeeConfig {
            fee_rate: Some(Decimal::percent(15)),
            fee_recipient: None,
        },
    );
    assert!(res.is_ok());

    // Verify only fee_rate was updated
    let fee_config = FEE_CONFIG.load(&deps.storage).unwrap();
    assert_eq!(fee_config.fee_rate, Decimal::percent(15));
    assert_eq!(fee_config.fee_recipient, treasury_addr); // Unchanged
}

#[test]
fn test_update_fee_config_unauthorized() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Try to update from non-whitelisted address
    let info = get_message_info(&deps.api, USER1, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdateFeeConfig {
            fee_rate: Some(Decimal::percent(10)),
            fee_recipient: None,
        },
    );
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("Unauthorized"));
}

#[test]
fn test_update_fee_config_invalid_rate() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr.clone(),
        vec![],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Try to update with invalid fee rate
    let info = get_message_info(&deps.api, WHITELIST, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdateFeeConfig {
            fee_rate: Some(Decimal::percent(150)), // Invalid
            fee_recipient: None,
        },
    );
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("Invalid fee rate"));
}

#[test]
fn test_update_fee_config_nonzero_rate_without_recipient() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);

    // Instantiate without fee config (recipient is empty, fee_rate is 0)
    let instantiate_msg =
        get_instantiate_msg(DEFAULT_DEPOSIT_CAP, whitelist_addr.clone(), vec![], None);

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Try to set a non-zero fee rate without setting a recipient first
    let info = get_message_info(&deps.api, WHITELIST, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdateFeeConfig {
            fee_rate: Some(Decimal::percent(20)),
            fee_recipient: None,
        },
    );
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("Fee recipient must be set"));
}

#[test]
fn test_update_fee_config_change_recipient() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let old_treasury = deps.api.addr_make("old_treasury");
    let new_treasury = deps.api.addr_make("new_treasury");

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr.clone(),
        vec![],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: old_treasury.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Update the recipient
    let info = get_message_info(&deps.api, WHITELIST, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdateFeeConfig {
            fee_rate: None,
            fee_recipient: Some(new_treasury.to_string()),
        },
    );
    assert!(res.is_ok());

    // Verify recipient was updated
    let fee_config = FEE_CONFIG.load(&deps.storage).unwrap();
    assert_eq!(fee_config.fee_recipient, new_treasury);
}

#[test]
fn test_update_fee_config_disable_by_zero_rate() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr.clone(),
        vec![],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Disable fees by setting fee_rate to 0
    let info = get_message_info(&deps.api, WHITELIST, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdateFeeConfig {
            fee_rate: Some(Decimal::zero()),
            fee_recipient: None,
        },
    );
    assert!(res.is_ok());

    // Verify fees are disabled (fee_rate = 0)
    let fee_config = FEE_CONFIG.load(&deps.storage).unwrap();
    assert_eq!(fee_config.fee_rate, Decimal::zero());
}

// ============================================================================
// Query Tests
// ============================================================================

#[test]
fn test_query_fee_config() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Query fee config
    let query_res = query(deps.as_ref(), env.clone(), QueryMsg::FeeConfig {}).unwrap();
    let fee_config: FeeConfigResponse = from_json(query_res).unwrap();

    assert_eq!(fee_config.fee_rate, Decimal::percent(20));
    assert_eq!(fee_config.fee_recipient, treasury_addr);
}

#[test]
fn test_query_fee_accrual_info_no_subvaults() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Query fee accrual info (with no subvaults, current share price defaults to 1)
    let query_res = query(deps.as_ref(), env.clone(), QueryMsg::FeeAccrualInfo {}).unwrap();
    let info: FeeAccrualInfoResponse = from_json(query_res).unwrap();

    assert_eq!(info.high_water_mark_price, Decimal::one());
    assert_eq!(info.current_share_price, Decimal::one()); // No shares, defaults to 1
    assert_eq!(info.pending_yield, Uint128::zero());
    assert_eq!(info.pending_fee, Uint128::zero());
}

// ============================================================================
// AccrueFees Tests
// ============================================================================

#[test]
fn test_accrue_fees_disabled() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);

    // Instantiate with fees disabled
    let instantiate_msg = get_instantiate_msg(DEFAULT_DEPOSIT_CAP, whitelist_addr, vec![], None);

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Try to accrue fees
    let info = get_message_info(&deps.api, USER1, &[]); // Anyone can call AccrueFees
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {});

    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("Fee accrual is disabled"));
}

#[test]
fn test_accrue_fees_no_shares() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![], // No subvaults
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Try to accrue fees with no shares
    let info = get_message_info(&deps.api, USER1, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {});

    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("No shares have been issued"));
}

#[test]
fn test_accrue_fees_below_high_water_mark() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);
    let subvault1_addr = deps.api.addr_make(SUBVAULT1);

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![subvault1_addr.clone()],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Setup mock querier with subvault having shares but pool value = shares (share price = 1.0)
    // Since high-water mark is initialized to 1.0, current price <= high-water mark
    setup_mock_querier_with_subvaults(
        &mut deps,
        vec![(subvault1_addr.to_string(), Uint128::new(1000))],
    );

    // Try to accrue fees - should return "below_high_water_mark"
    let info = get_message_info(&deps.api, USER1, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {});

    assert!(res.is_ok());
    let response = res.unwrap();
    assert!(response
        .attributes
        .iter()
        .any(|a| a.key == "result" && a.value == "below_high_water_mark"));
}

#[test]
fn test_accrue_fees_basic_yield() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);
    let subvault1_addr = deps.api.addr_make(SUBVAULT1);

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![subvault1_addr.clone()],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Setup mock querier: subvault has 1000 shares, balance is 1100 (10% yield)
    // This means share_price = 1100/1000 = 1.1
    let vault_shares = Uint128::new(1000);
    let vault_balance = Uint128::new(1100);

    deps.querier.update_wasm({
        let subvault_addr = subvault1_addr.to_string();
        move |query| match query {
            WasmQuery::Smart { contract_addr, .. } if contract_addr == &subvault_addr => {
                let response = to_json_binary(&VaultPoolInfoResponse {
                    shares_issued: vault_shares,
                    balance_base_tokens: vault_balance,
                    adapter_deposits_base_tokens: Uint128::zero(),
                    withdrawal_queue_base_tokens: Uint128::zero(),
                })
                .unwrap();
                SystemResult::Ok(ContractResult::Ok(response))
            }
            _ => SystemResult::Err(SystemError::NoSuchContract {
                addr: "unknown".to_string(),
            }),
        }
    });

    // Accrue fees
    let info = get_message_info(&deps.api, USER1, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {});

    assert!(res.is_ok());
    let response = res.unwrap();

    // Verify the response indicates fees were accrued
    assert!(response
        .attributes
        .iter()
        .any(|a| a.key == "result" && a.value == "fees_accrued"));

    // Verify MintFeeShares message was generated
    assert!(!response.messages.is_empty());

    // Verify high-water mark was updated
    let high_water_mark_price = HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap();
    assert_eq!(
        high_water_mark_price,
        Decimal::from_ratio(1100u128, 1000u128)
    );
}

#[test]
fn test_accrue_fees_permissionless() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);
    let subvault1_addr = deps.api.addr_make(SUBVAULT1);

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![subvault1_addr.clone()],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Setup mock with yield
    let vault_shares = Uint128::new(1000);
    let vault_balance = Uint128::new(1100);

    deps.querier.update_wasm({
        let subvault_addr = subvault1_addr.to_string();
        move |query| match query {
            WasmQuery::Smart { contract_addr, .. } if contract_addr == &subvault_addr => {
                let response = to_json_binary(&VaultPoolInfoResponse {
                    shares_issued: vault_shares,
                    balance_base_tokens: vault_balance,
                    adapter_deposits_base_tokens: Uint128::zero(),
                    withdrawal_queue_base_tokens: Uint128::zero(),
                })
                .unwrap();
                SystemResult::Ok(ContractResult::Ok(response))
            }
            _ => SystemResult::Err(SystemError::NoSuchContract {
                addr: "unknown".to_string(),
            }),
        }
    });

    // Any address (not whitelisted) can call AccrueFees
    let info = get_message_info(&deps.api, "random_user", &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {});

    // Should succeed - AccrueFees is permissionless
    assert!(res.is_ok());
}

#[test]
fn test_accrue_fees_zero_fee_rate_is_disabled() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);
    let subvault1_addr = deps.api.addr_make(SUBVAULT1);

    // Instantiate with fee_rate=0, which means fees are disabled
    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![subvault1_addr.clone()],
        Some(FeeConfigInit {
            fee_rate: Decimal::zero(), // 0% fee rate means disabled
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Setup mock with yield
    let vault_shares = Uint128::new(1000);
    let vault_balance = Uint128::new(1100);

    deps.querier.update_wasm({
        let subvault_addr = subvault1_addr.to_string();
        move |query| match query {
            WasmQuery::Smart { contract_addr, .. } if contract_addr == &subvault_addr => {
                let response = to_json_binary(&VaultPoolInfoResponse {
                    shares_issued: vault_shares,
                    balance_base_tokens: vault_balance,
                    adapter_deposits_base_tokens: Uint128::zero(),
                    withdrawal_queue_base_tokens: Uint128::zero(),
                })
                .unwrap();
                SystemResult::Ok(ContractResult::Ok(response))
            }
            _ => SystemResult::Err(SystemError::NoSuchContract {
                addr: "unknown".to_string(),
            }),
        }
    });

    // Try to accrue fees - should fail because fee_rate=0 means disabled
    let info = get_message_info(&deps.api, USER1, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {});

    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("Fee accrual is disabled"));
}

#[test]
fn test_accrue_fees_proportional_two_vaults() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);
    let subvault1_addr = deps.api.addr_make(SUBVAULT1);
    let subvault2_addr = deps.api.addr_make(SUBVAULT2);

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![subvault1_addr.clone(), subvault2_addr.clone()],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Setup mock with two vaults:
    // Vault 1: 250 shares (25%), balance 275
    // Vault 2: 750 shares (75%), balance 825
    // Total: 1000 shares, 1100 balance (10% yield)
    let vault1_shares = Uint128::new(250);
    let vault1_balance = Uint128::new(275);
    let vault2_shares = Uint128::new(750);
    let vault2_balance = Uint128::new(825);

    deps.querier.update_wasm({
        let subvault1 = subvault1_addr.to_string();
        let subvault2 = subvault2_addr.to_string();
        move |query| match query {
            WasmQuery::Smart { contract_addr, .. } => {
                let response = if contract_addr == &subvault1 {
                    to_json_binary(&VaultPoolInfoResponse {
                        shares_issued: vault1_shares,
                        balance_base_tokens: vault1_balance,
                        adapter_deposits_base_tokens: Uint128::zero(),
                        withdrawal_queue_base_tokens: Uint128::zero(),
                    })
                } else if contract_addr == &subvault2 {
                    to_json_binary(&VaultPoolInfoResponse {
                        shares_issued: vault2_shares,
                        balance_base_tokens: vault2_balance,
                        adapter_deposits_base_tokens: Uint128::zero(),
                        withdrawal_queue_base_tokens: Uint128::zero(),
                    })
                } else {
                    return SystemResult::Err(SystemError::NoSuchContract {
                        addr: contract_addr.clone(),
                    });
                };
                SystemResult::Ok(ContractResult::Ok(response.unwrap()))
            }
            _ => SystemResult::Err(SystemError::UnsupportedRequest {
                kind: "only smart queries supported".to_string(),
            }),
        }
    });

    // Accrue fees
    let info = get_message_info(&deps.api, USER1, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {});

    assert!(res.is_ok());
    let response = res.unwrap();

    // Should have 2 mint messages (one per vault)
    assert_eq!(response.messages.len(), 2);

    // Verify fees were accrued
    assert!(response
        .attributes
        .iter()
        .any(|a| a.key == "result" && a.value == "fees_accrued"));
}

#[test]
fn test_high_water_mark_consecutive_accruals() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);
    let subvault1_addr = deps.api.addr_make(SUBVAULT1);

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![subvault1_addr.clone()],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Helper to update mock and accrue
    let run_accrual = |deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
                       env: &Env,
                       shares: u128,
                       balance: u128|
     -> Response<NeutronMsg> {
        let subvault_addr = deps.api.addr_make(SUBVAULT1).to_string();
        deps.querier.update_wasm({
            let addr = subvault_addr.clone();
            move |query| match query {
                WasmQuery::Smart { contract_addr, .. } if contract_addr == &addr => {
                    let response = to_json_binary(&VaultPoolInfoResponse {
                        shares_issued: Uint128::new(shares),
                        balance_base_tokens: Uint128::new(balance),
                        adapter_deposits_base_tokens: Uint128::zero(),
                        withdrawal_queue_base_tokens: Uint128::zero(),
                    })
                    .unwrap();
                    SystemResult::Ok(ContractResult::Ok(response))
                }
                _ => SystemResult::Err(SystemError::NoSuchContract {
                    addr: "unknown".to_string(),
                }),
            }
        });

        let info = MessageInfo {
            sender: deps.api.addr_make(USER1),
            funds: vec![],
        };
        execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {}).unwrap()
    };

    // First accrual: 5% yield (price 1.0 -> 1.05)
    let res1 = run_accrual(&mut deps, &env, 1000, 1050);
    assert!(res1
        .attributes
        .iter()
        .any(|a| a.key == "result" && a.value == "fees_accrued"));
    assert_eq!(
        HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap(),
        Decimal::from_ratio(1050u128, 1000u128)
    );

    // Second accrual: another 5% yield (price 1.05 -> ~1.10)
    let res2 = run_accrual(&mut deps, &env, 1000, 1100);
    assert!(res2
        .attributes
        .iter()
        .any(|a| a.key == "result" && a.value == "fees_accrued"));
    assert_eq!(
        HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap(),
        Decimal::from_ratio(1100u128, 1000u128)
    );

    // Third accrual: another ~5% yield
    let res3 = run_accrual(&mut deps, &env, 1000, 1150);
    assert!(res3
        .attributes
        .iter()
        .any(|a| a.key == "result" && a.value == "fees_accrued"));
    assert_eq!(
        HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap(),
        Decimal::from_ratio(1150u128, 1000u128)
    );
}

#[test]
fn test_high_water_mark_recovery_from_loss() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);
    let subvault1_addr = deps.api.addr_make(SUBVAULT1);

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![subvault1_addr.clone()],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Helper function
    let run_accrual = |deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
                       env: &Env,
                       shares: u128,
                       balance: u128|
     -> Response<NeutronMsg> {
        let subvault_addr = deps.api.addr_make(SUBVAULT1).to_string();
        deps.querier.update_wasm({
            let addr = subvault_addr.clone();
            move |query| match query {
                WasmQuery::Smart { contract_addr, .. } if contract_addr == &addr => {
                    let response = to_json_binary(&VaultPoolInfoResponse {
                        shares_issued: Uint128::new(shares),
                        balance_base_tokens: Uint128::new(balance),
                        adapter_deposits_base_tokens: Uint128::zero(),
                        withdrawal_queue_base_tokens: Uint128::zero(),
                    })
                    .unwrap();
                    SystemResult::Ok(ContractResult::Ok(response))
                }
                _ => SystemResult::Err(SystemError::NoSuchContract {
                    addr: "unknown".to_string(),
                }),
            }
        });

        let info = MessageInfo {
            sender: deps.api.addr_make(USER1),
            funds: vec![],
        };
        execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {}).unwrap()
    };

    // Step 1: Yield to 1.2 -> fees charged, hwm = 1.2
    let res1 = run_accrual(&mut deps, &env, 1000, 1200);
    assert!(res1
        .attributes
        .iter()
        .any(|a| a.key == "result" && a.value == "fees_accrued"));
    assert_eq!(
        HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap(),
        Decimal::from_ratio(1200u128, 1000u128)
    );

    // Step 2: Loss to 0.9 -> no fees
    let res2 = run_accrual(&mut deps, &env, 1000, 900);
    assert!(res2
        .attributes
        .iter()
        .any(|a| a.key == "result" && a.value == "below_high_water_mark"));
    // hwm should remain at 1.2
    assert_eq!(
        HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap(),
        Decimal::from_ratio(1200u128, 1000u128)
    );

    // Step 3: Recovery to 1.1 -> no fees (1.1 < hwm 1.2)
    let res3 = run_accrual(&mut deps, &env, 1000, 1100);
    assert!(res3
        .attributes
        .iter()
        .any(|a| a.key == "result" && a.value == "below_high_water_mark"));
    // hwm should remain at 1.2
    assert_eq!(
        HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap(),
        Decimal::from_ratio(1200u128, 1000u128)
    );

    // Step 4: New high at 1.3 -> fees on (1.3 - 1.2) = 0.1
    let res4 = run_accrual(&mut deps, &env, 1000, 1300);
    assert!(res4
        .attributes
        .iter()
        .any(|a| a.key == "result" && a.value == "fees_accrued"));
    assert_eq!(
        HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap(),
        Decimal::from_ratio(1300u128, 1000u128)
    );
}
