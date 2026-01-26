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

/// Tests that dust yield does NOT update the high-water mark.
/// When yield is too small to mint any shares, the high-water mark should remain unchanged
/// so that dust yield accumulates over multiple accrual calls until it's large enough.
///
/// Bug scenario (before fix):
/// 1. Small yield occurs -> shares_to_mint = 0.5 (dust)
/// 2. High-water mark updated to current price (BUG!)
/// 3. Next small yield: calculated from new HWM, dust again
/// 4. Dust yields are lost forever, never accumulated
///
/// Correct behavior (after fix):
/// 1. Small yield occurs -> shares_to_mint = 0.5 (dust)
/// 2. High-water mark stays at old price (correct!)
/// 3. Next small yield: calculated from old HWM, accumulates
/// 4. Eventually combined yield is enough to mint shares
#[test]
fn test_dust_yield_does_not_update_high_water_mark() {
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

    // Verify initial high-water mark is 1.0
    let initial_hwm = HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap();
    assert_eq!(initial_hwm, Decimal::one());

    // Helper to set up mock querier
    let setup_vault_state =
        |deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
         shares: u128,
         balance: u128| {
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
        };

    // Step 1: Set up a scenario with tiny yield that results in dust
    // With 1000 shares at price 1.0, if balance = 1001:
    //   yield_per_share = 1001/1000 - 1.0 = 0.001
    //   total_yield = 0.001 * 1000 = 1
    //   fee_amount = 1 * 0.2 = 0.2
    //   shares_to_mint = 0.2 / 1.001 ≈ 0.1998 (dust!)
    setup_vault_state(&mut deps, 1000, 1001);

    let info = get_message_info(&deps.api, USER1, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {});
    assert!(res.is_ok());

    let response = res.unwrap();
    // Verify it was recognized as dust yield
    assert!(
        response
            .attributes
            .iter()
            .any(|a| a.key == "result" && a.value == "dust_yield"),
        "Expected dust_yield result, got: {:?}",
        response.attributes
    );

    // CRITICAL CHECK: High-water mark should NOT have been updated
    // The bug causes it to update to 1.001, losing the dust yield
    let hwm_after_dust = HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap();
    assert_eq!(
        hwm_after_dust, initial_hwm,
        "High-water mark should NOT be updated on dust yield! \
        Expected: {}, Got: {}. The dust yield was lost.",
        initial_hwm, hwm_after_dust
    );

    // Step 2: Add more small yield (now balance = 1005)
    // If HWM stayed at 1.0:
    //   yield_per_share = 1005/1000 - 1.0 = 0.005
    //   total_yield = 0.005 * 1000 = 5
    //   fee_amount = 5 * 0.2 = 1
    //   shares_to_mint = 1 / 1.005 ≈ 0.995 (still dust but closer)
    setup_vault_state(&mut deps, 1000, 1005);

    let info = get_message_info(&deps.api, USER1, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {});
    assert!(res.is_ok());

    // Still dust (0.995 shares), so HWM should still be unchanged
    let hwm_after_second_dust = HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap();
    assert_eq!(
        hwm_after_second_dust, initial_hwm,
        "High-water mark should still NOT be updated after second dust yield"
    );

    // Step 3: Add enough yield to finally mint shares (balance = 1020)
    // If HWM stayed at 1.0:
    //   yield_per_share = 1020/1000 - 1.0 = 0.02
    //   total_yield = 0.02 * 1000 = 20
    //   fee_amount = 20 * 0.2 = 4
    //   shares_to_mint = 4 / 1.02 ≈ 3.92 -> 3 shares (enough!)
    setup_vault_state(&mut deps, 1000, 1020);

    let info = get_message_info(&deps.api, USER1, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {});
    assert!(res.is_ok());

    let response = res.unwrap();
    // Now it should accrue fees
    assert!(
        response
            .attributes
            .iter()
            .any(|a| a.key == "result" && a.value == "fees_accrued"),
        "Expected fees_accrued when yield is large enough"
    );

    // NOW the high-water mark should be updated to 1.02
    let hwm_after_accrual = HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap();
    assert_eq!(
        hwm_after_accrual,
        Decimal::from_ratio(1020u128, 1000u128),
        "High-water mark should be updated after successful fee accrual"
    );
}

/// Tests that when fees are re-enabled after being disabled, the high-water mark
/// is reset to the current share price, so fees are NOT charged on yield that
/// occurred while fees were disabled.
#[test]
fn test_reenable_fees_resets_high_water_mark() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);
    let subvault1_addr = deps.api.addr_make(SUBVAULT1);

    // Step 1: Instantiate with fees enabled at 20%
    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr.clone(),
        vec![subvault1_addr.clone()],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Verify initial high-water mark is 1.0
    assert_eq!(
        HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap(),
        Decimal::one()
    );

    // Helper to set up mock querier with specific shares and balance
    let setup_vault_state =
        |deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
         shares: u128,
         balance: u128| {
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
        };

    // Step 2: Accrue fees with 10% yield (price 1.0 -> 1.1)
    // This sets high-water mark to 1.1
    setup_vault_state(&mut deps, 1000, 1100);
    let info = get_message_info(&deps.api, USER1, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {});
    assert!(res.is_ok());
    assert!(res
        .unwrap()
        .attributes
        .iter()
        .any(|a| a.key == "result" && a.value == "fees_accrued"));
    assert_eq!(
        HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap(),
        Decimal::from_ratio(1100u128, 1000u128) // 1.1
    );

    // Step 3: Disable fees by setting fee_rate to 0
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

    // Step 4: Simulate yield while fees are disabled (price goes from 1.1 to 1.5)
    // This yield should NOT be subject to fees when fees are re-enabled
    setup_vault_state(&mut deps, 1000, 1500);

    // Step 5: Re-enable fees at 20%
    let info = get_message_info(&deps.api, WHITELIST, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::UpdateFeeConfig {
            fee_rate: Some(Decimal::percent(20)),
            fee_recipient: None, // Keep existing recipient
        },
    );
    assert!(res.is_ok());

    // CRITICAL CHECK: After re-enabling fees, the high-water mark should be reset
    // to the current share price (1.5), NOT remain at the old value (1.1)
    let high_water_mark = HIGH_WATER_MARK_PRICE.load(&deps.storage).unwrap();
    assert_eq!(
        high_water_mark,
        Decimal::from_ratio(1500u128, 1000u128), // Should be 1.5
        "High-water mark should be reset to current price when fees are re-enabled"
    );

    // Step 6: Accrue fees - should report "below_high_water_mark" since there's
    // no NEW yield since fees were re-enabled
    let info = get_message_info(&deps.api, USER1, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::AccrueFees {});
    assert!(res.is_ok());
    assert!(
        res.unwrap()
            .attributes
            .iter()
            .any(|a| a.key == "result" && a.value == "below_high_water_mark"),
        "Should not charge fees on yield that occurred while fees were disabled"
    );
}

/// Tests that proportional fee distribution correctly handles rounding remainders
/// when the last subvault in the iteration order has zero shares.
///
/// Bug scenario (before fix):
/// - 3 subvaults: A (33 shares), B (67 shares), C (0 shares)
/// - shares_to_mint = 10
/// - Vault A gets: 10 * 33/100 = 3 (rounds down)
/// - Vault B gets: 10 * 67/100 = 6 (rounds down)
/// - Vault C is skipped (0 shares) via continue BEFORE remainder logic runs
/// - Total minted: 9, but should be 10 -> 1 share lost!
///
/// The remainder logic at line 431-433 gives the remainder to the "last vault",
/// but if that vault has 0 shares, it's skipped before the remainder calculation.
#[test]
fn test_accrue_fees_remainder_when_last_vault_has_zero_shares() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let treasury_addr = deps.api.addr_make(TREASURY);

    // Use names that will sort alphabetically such that the zero-shares vault
    // comes LAST in ascending order. MockApi.addr_make creates deterministic
    // addresses, so we use prefixes to control the sort order.
    // "aaa_vault" < "bbb_vault" < "zzz_vault" in ascending order
    let subvault_first_addr = deps.api.addr_make("aaa_vault"); // Will be first
    let subvault_second_addr = deps.api.addr_make("bbb_vault"); // Will be second
    let subvault_last_addr = deps.api.addr_make("zzz_vault"); // Will be LAST (has 0 shares)

    let instantiate_msg = get_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_addr,
        vec![
            subvault_first_addr.clone(),
            subvault_second_addr.clone(),
            subvault_last_addr.clone(),
        ],
        Some(FeeConfigInit {
            fee_rate: Decimal::percent(20),
            fee_recipient: treasury_addr.to_string(),
        }),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Setup mock with three vaults where the LAST vault (in sorted order) has ZERO shares:
    // Use numbers that create a clear rounding remainder.
    //
    // First vault: 333 shares (33.3%), balance 366 (to maintain ~1.1 price)
    // Second vault: 667 shares (66.7%), balance 734 (to maintain ~1.1 price)
    // Last vault: 0 shares (0%), balance 0
    // Total: 1000 shares, 1100 balance (10% yield from price 1.0 -> 1.1)
    //
    // Calculation:
    //   yield_per_share = 1.1 - 1.0 = 0.1
    //   total_yield = 0.1 * 1000 = 100
    //   fee_amount = 100 * 0.2 = 20
    //   shares_to_mint = 20 / 1.1 ≈ 18.18 -> 18 shares (floor)
    //
    // Without the bug (correct behavior):
    //   First vault: 18 * 333/1000 = 5.994 -> 5 (rounds down)
    //   Second vault: remainder = 18 - 5 = 13 (should get remainder)
    //   Total: 5 + 13 = 18
    //
    // With the bug (last vault has 0 shares):
    //   First vault: 18 * 333/1000 = 5.994 -> 5 (rounds down)
    //   Second vault: 18 * 667/1000 = 12.006 -> 12 (rounds down, NOT getting remainder!)
    //   Last vault: skipped (0 shares) - remainder logic never runs
    //   Total: 5 + 12 = 17, but should be 18 -> 1 share lost!
    let vault_first_shares = Uint128::new(333);
    let vault_first_balance = Uint128::new(366); // ~333 * 1.1
    let vault_second_shares = Uint128::new(667);
    let vault_second_balance = Uint128::new(734); // ~667 * 1.1
    let vault_last_shares = Uint128::zero(); // Last vault has ZERO shares
    let vault_last_balance = Uint128::zero();

    deps.querier.update_wasm({
        let subvault_first = subvault_first_addr.to_string();
        let subvault_second = subvault_second_addr.to_string();
        let subvault_last = subvault_last_addr.to_string();
        move |query| match query {
            WasmQuery::Smart { contract_addr, .. } => {
                let response = if contract_addr == &subvault_first {
                    to_json_binary(&VaultPoolInfoResponse {
                        shares_issued: vault_first_shares,
                        balance_base_tokens: vault_first_balance,
                        adapter_deposits_base_tokens: Uint128::zero(),
                        withdrawal_queue_base_tokens: Uint128::zero(),
                    })
                } else if contract_addr == &subvault_second {
                    to_json_binary(&VaultPoolInfoResponse {
                        shares_issued: vault_second_shares,
                        balance_base_tokens: vault_second_balance,
                        adapter_deposits_base_tokens: Uint128::zero(),
                        withdrawal_queue_base_tokens: Uint128::zero(),
                    })
                } else if contract_addr == &subvault_last {
                    to_json_binary(&VaultPoolInfoResponse {
                        shares_issued: vault_last_shares,
                        balance_base_tokens: vault_last_balance,
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

    // Verify fees were accrued
    assert!(
        response
            .attributes
            .iter()
            .any(|a| a.key == "result" && a.value == "fees_accrued"),
        "Expected fees_accrued result"
    );

    // Should have exactly 2 mint messages (only for vaults with non-zero shares)
    assert_eq!(
        response.messages.len(),
        2,
        "Expected 2 mint messages (one per vault with shares)"
    );

    // Extract the shares_minted attribute to verify the total
    let shares_minted_attr = response
        .attributes
        .iter()
        .find(|a| a.key == "shares_minted")
        .expect("Should have shares_minted attribute");
    let total_shares_to_mint: Uint128 = shares_minted_attr.value.parse().unwrap();

    // Now parse the mint messages to get the actual amounts minted
    let mut total_actually_minted = Uint128::zero();
    for msg in &response.messages {
        if let cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute { msg, .. }) = &msg.msg
        {
            // Parse the MintFeeShares message to extract the amount
            let mint_msg: interface::inflow_vault::ExecuteMsg = from_json(msg).unwrap();
            if let interface::inflow_vault::ExecuteMsg::MintFeeShares { amount, .. } = mint_msg {
                total_actually_minted = total_actually_minted.checked_add(amount).unwrap();
            }
        }
    }

    // CRITICAL CHECK: The total actually minted should equal the intended shares_to_mint
    // The bug causes total_actually_minted < total_shares_to_mint because the remainder
    // is lost when the last vault has zero shares
    assert_eq!(
        total_actually_minted, total_shares_to_mint,
        "Rounding remainder lost! Expected to mint {} shares but only {} were distributed. \
        The remainder was lost because the last vault has zero shares.",
        total_shares_to_mint, total_actually_minted
    );
}
