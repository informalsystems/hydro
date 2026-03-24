use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
use cosmwasm_std::{coin, coins, Timestamp, Uint128};

use crate::contract::{execute, instantiate, query};
use crate::error::ContractError;
use crate::msg::{ClaimEntry, ExecuteMsg, InstantiateMsg};
use crate::query::{
    ClaimHistoryResponse, ConfigResponse, DistributionResponse, PendingClaimsResponse, QueryMsg,
};

fn setup_contract() -> (
    cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier,
    >,
    cosmwasm_std::Env,
) {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let admin = deps.api.addr_make("admin");
    let treasury = deps.api.addr_make("treasury");

    let msg = InstantiateMsg {
        admin: admin.to_string(),
        treasury: treasury.to_string(),
    };
    let info = message_info(&admin, &[]);
    instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

    (deps, env)
}

#[test]
fn test_instantiate() {
    let (deps, _env) = setup_contract();

    let res: ConfigResponse =
        cosmwasm_std::from_json(query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap())
            .unwrap();

    let admin = deps.api.addr_make("admin");
    let treasury = deps.api.addr_make("treasury");
    assert_eq!(res.config.admin, admin);
    assert_eq!(res.config.treasury, treasury);
}

#[test]
fn test_create_distribution() {
    let (mut deps, env) = setup_contract();
    let admin = deps.api.addr_make("admin");
    let alice = deps.api.addr_make("alice");
    let bob = deps.api.addr_make("bob");

    let expiry = Timestamp::from_seconds(env.block.time.seconds() + 3600);
    let msg = ExecuteMsg::CreateDistribution {
        claims: vec![
            ClaimEntry {
                address: alice.to_string(),
                weight: Uint128::new(3),
            },
            ClaimEntry {
                address: bob.to_string(),
                weight: Uint128::new(7),
            },
        ],
        expiry,
    };

    let info = message_info(&admin, &[coin(1000, "uatom"), coin(500, "uusdc")]);
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert!(res
        .attributes
        .iter()
        .any(|a| a.key == "distribution_id" && a.value == "0"));

    // Query distribution
    let res: DistributionResponse = cosmwasm_std::from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::Distribution { id: 0 }).unwrap(),
    )
    .unwrap();
    assert_eq!(res.distribution.total_weight, Uint128::new(10));
    assert_eq!(res.distribution.original_funds.len(), 2);
}

#[test]
fn test_create_distribution_unauthorized() {
    let (mut deps, env) = setup_contract();
    let not_admin = deps.api.addr_make("not_admin");

    let msg = ExecuteMsg::CreateDistribution {
        claims: vec![ClaimEntry {
            address: not_admin.to_string(),
            weight: Uint128::new(1),
        }],
        expiry: Timestamp::from_seconds(env.block.time.seconds() + 3600),
    };

    let info = message_info(&not_admin, &coins(100, "uatom"));
    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(err, ContractError::Unauthorized);
}

#[test]
fn test_create_distribution_no_funds() {
    let (mut deps, env) = setup_contract();
    let admin = deps.api.addr_make("admin");

    let msg = ExecuteMsg::CreateDistribution {
        claims: vec![ClaimEntry {
            address: admin.to_string(),
            weight: Uint128::new(1),
        }],
        expiry: Timestamp::from_seconds(env.block.time.seconds() + 3600),
    };

    let info = message_info(&admin, &[]);
    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(err, ContractError::NoFundsSent);
}

#[test]
fn test_claim() {
    let (mut deps, env) = setup_contract();
    let admin = deps.api.addr_make("admin");
    let alice = deps.api.addr_make("alice");
    let bob = deps.api.addr_make("bob");

    // Create distribution: 1000 uatom, alice weight 3, bob weight 7
    let expiry = Timestamp::from_seconds(env.block.time.seconds() + 3600);
    let create_msg = ExecuteMsg::CreateDistribution {
        claims: vec![
            ClaimEntry {
                address: alice.to_string(),
                weight: Uint128::new(3),
            },
            ClaimEntry {
                address: bob.to_string(),
                weight: Uint128::new(7),
            },
        ],
        expiry,
    };
    let info = message_info(&admin, &coins(1000, "uatom"));
    execute(deps.as_mut(), env.clone(), info, create_msg).unwrap();

    // Alice claims: should get 300 uatom (1000 * 3/10)
    let info = message_info(&alice, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::Claim {}).unwrap();

    assert_eq!(res.messages.len(), 1);
    let msg = &res.messages[0].msg;
    match msg {
        cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send { to_address, amount }) => {
            assert_eq!(to_address, alice.as_str());
            assert_eq!(amount, &coins(300, "uatom"));
        }
        _ => panic!("Expected BankMsg::Send"),
    }

    // Alice claims again: should fail
    let info = message_info(&alice, &[]);
    let err = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::Claim {}).unwrap_err();
    assert_eq!(err, ContractError::NoPendingClaims);

    // Bob claims: should get 700 uatom (1000 * 7/10)
    let info = message_info(&bob, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::Claim {}).unwrap();

    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send { to_address, amount }) => {
            assert_eq!(to_address, bob.as_str());
            assert_eq!(amount, &coins(700, "uatom"));
        }
        _ => panic!("Expected BankMsg::Send"),
    }
}

#[test]
fn test_claim_multi_denom() {
    let (mut deps, env) = setup_contract();
    let admin = deps.api.addr_make("admin");
    let alice = deps.api.addr_make("alice");

    let expiry = Timestamp::from_seconds(env.block.time.seconds() + 3600);
    let create_msg = ExecuteMsg::CreateDistribution {
        claims: vec![ClaimEntry {
            address: alice.to_string(),
            weight: Uint128::new(1),
        }],
        expiry,
    };
    let info = message_info(&admin, &[coin(100, "uatom"), coin(200, "uusdc")]);
    execute(deps.as_mut(), env.clone(), info, create_msg).unwrap();

    let info = message_info(&alice, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::Claim {}).unwrap();

    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send { amount, .. }) => {
            assert_eq!(amount.len(), 2);
            assert!(amount.contains(&coin(100, "uatom")));
            assert!(amount.contains(&coin(200, "uusdc")));
        }
        _ => panic!("Expected BankMsg::Send"),
    }
}

#[test]
fn test_claim_expired_distribution_skipped() {
    let (mut deps, mut env) = setup_contract();
    let admin = deps.api.addr_make("admin");
    let alice = deps.api.addr_make("alice");

    let expiry = Timestamp::from_seconds(env.block.time.seconds() + 100);
    let create_msg = ExecuteMsg::CreateDistribution {
        claims: vec![ClaimEntry {
            address: alice.to_string(),
            weight: Uint128::new(1),
        }],
        expiry,
    };
    let info = message_info(&admin, &coins(500, "uatom"));
    execute(deps.as_mut(), env.clone(), info, create_msg).unwrap();

    // Advance time past expiry
    env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 200);

    // Alice tries to claim - distribution expired, so nothing to claim
    let info = message_info(&alice, &[]);
    let err = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::Claim {}).unwrap_err();
    assert_eq!(err, ContractError::NoPendingClaims);
}

#[test]
fn test_sweep_expired() {
    let (mut deps, mut env) = setup_contract();
    let admin = deps.api.addr_make("admin");
    let alice = deps.api.addr_make("alice");
    let bob = deps.api.addr_make("bob");
    let treasury = deps.api.addr_make("treasury");
    let anyone = deps.api.addr_make("anyone");

    let expiry = Timestamp::from_seconds(env.block.time.seconds() + 100);
    let create_msg = ExecuteMsg::CreateDistribution {
        claims: vec![
            ClaimEntry {
                address: alice.to_string(),
                weight: Uint128::new(3),
            },
            ClaimEntry {
                address: bob.to_string(),
                weight: Uint128::new(7),
            },
        ],
        expiry,
    };
    let info = message_info(&admin, &coins(1000, "uatom"));
    execute(deps.as_mut(), env.clone(), info, create_msg).unwrap();

    // Alice claims before expiry
    let info = message_info(&alice, &[]);
    execute(deps.as_mut(), env.clone(), info, ExecuteMsg::Claim {}).unwrap();

    // Advance time past expiry
    env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 200);

    // Anyone sweeps: remaining 700 uatom goes to treasury
    let info = message_info(&anyone, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SweepExpired { distribution_id: 0 },
    )
    .unwrap();

    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send { to_address, amount }) => {
            assert_eq!(to_address, treasury.as_str());
            assert_eq!(amount, &coins(700, "uatom"));
        }
        _ => panic!("Expected BankMsg::Send"),
    }

    // Sweep again: should fail
    let info = message_info(&anyone, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::SweepExpired { distribution_id: 0 },
    )
    .unwrap_err();
    assert_eq!(err, ContractError::DistributionAlreadySwept { id: 0 });
}

#[test]
fn test_sweep_not_expired() {
    let (mut deps, env) = setup_contract();
    let admin = deps.api.addr_make("admin");
    let alice = deps.api.addr_make("alice");
    let anyone = deps.api.addr_make("anyone");

    let expiry = Timestamp::from_seconds(env.block.time.seconds() + 3600);
    let create_msg = ExecuteMsg::CreateDistribution {
        claims: vec![ClaimEntry {
            address: alice.to_string(),
            weight: Uint128::new(1),
        }],
        expiry,
    };
    let info = message_info(&admin, &coins(100, "uatom"));
    execute(deps.as_mut(), env.clone(), info, create_msg).unwrap();

    let info = message_info(&anyone, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::SweepExpired { distribution_id: 0 },
    )
    .unwrap_err();
    assert_eq!(err, ContractError::DistributionNotExpired { id: 0 });
}

#[test]
fn test_pending_claims_query() {
    let (mut deps, env) = setup_contract();
    let admin = deps.api.addr_make("admin");
    let alice = deps.api.addr_make("alice");

    let expiry = Timestamp::from_seconds(env.block.time.seconds() + 3600);
    let create_msg = ExecuteMsg::CreateDistribution {
        claims: vec![ClaimEntry {
            address: alice.to_string(),
            weight: Uint128::new(5),
        }],
        expiry,
    };
    let info = message_info(&admin, &coins(1000, "uatom"));
    execute(deps.as_mut(), env.clone(), info, create_msg).unwrap();

    let res: PendingClaimsResponse = cosmwasm_std::from_json(
        query(
            deps.as_ref(),
            env,
            QueryMsg::PendingClaims {
                user: alice.to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(res.claims.len(), 1);
    assert_eq!(res.claims[0].distribution_id, 0);
    assert_eq!(res.claims[0].weight, Uint128::new(5));
    assert_eq!(res.claims[0].estimated_funds, coins(1000, "uatom"));
}

#[test]
fn test_claim_across_multiple_distributions() {
    let (mut deps, env) = setup_contract();
    let admin = deps.api.addr_make("admin");
    let alice = deps.api.addr_make("alice");

    let expiry = Timestamp::from_seconds(env.block.time.seconds() + 3600);

    // Distribution 0: 1000 uatom, alice weight 1 (of 1)
    let info = message_info(&admin, &coins(1000, "uatom"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateDistribution {
            claims: vec![ClaimEntry {
                address: alice.to_string(),
                weight: Uint128::new(1),
            }],
            expiry,
        },
    )
    .unwrap();

    // Distribution 1: 500 uusdc, alice weight 1 (of 2)
    let bob = deps.api.addr_make("bob");
    let info = message_info(&admin, &coins(500, "uusdc"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateDistribution {
            claims: vec![
                ClaimEntry {
                    address: alice.to_string(),
                    weight: Uint128::new(1),
                },
                ClaimEntry {
                    address: bob.to_string(),
                    weight: Uint128::new(1),
                },
            ],
            expiry,
        },
    )
    .unwrap();

    // Alice claims all: 1000 uatom + 250 uusdc
    let info = message_info(&alice, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::Claim {}).unwrap();

    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send { amount, .. }) => {
            assert!(amount.contains(&coin(1000, "uatom")));
            assert!(amount.contains(&coin(250, "uusdc")));
        }
        _ => panic!("Expected BankMsg::Send"),
    }
}

#[test]
fn test_update_config() {
    let (mut deps, env) = setup_contract();
    let admin = deps.api.addr_make("admin");
    let new_admin = deps.api.addr_make("new_admin");
    let new_treasury = deps.api.addr_make("new_treasury");

    let msg = ExecuteMsg::UpdateConfig {
        admin: Some(new_admin.to_string()),
        treasury: Some(new_treasury.to_string()),
    };
    let info = message_info(&admin, &[]);
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let res: ConfigResponse =
        cosmwasm_std::from_json(query(deps.as_ref(), env, QueryMsg::Config {}).unwrap()).unwrap();

    assert_eq!(res.config.admin, new_admin);
    assert_eq!(res.config.treasury, new_treasury);
}

#[test]
fn test_duplicate_addresses_accumulate_weight() {
    let (mut deps, env) = setup_contract();
    let admin = deps.api.addr_make("admin");
    let alice = deps.api.addr_make("alice");
    let bob = deps.api.addr_make("bob");

    // Alice appears twice with weights 3 and 2 => effective weight 5
    // Bob has weight 5 => total 10
    // So alice gets 500/1000, bob gets 500/1000
    let expiry = Timestamp::from_seconds(env.block.time.seconds() + 3600);
    let create_msg = ExecuteMsg::CreateDistribution {
        claims: vec![
            ClaimEntry {
                address: alice.to_string(),
                weight: Uint128::new(3),
            },
            ClaimEntry {
                address: bob.to_string(),
                weight: Uint128::new(5),
            },
            ClaimEntry {
                address: alice.to_string(),
                weight: Uint128::new(2),
            },
        ],
        expiry,
    };
    let info = message_info(&admin, &coins(1000, "uatom"));
    execute(deps.as_mut(), env.clone(), info, create_msg).unwrap();

    // Alice claims: should get 500 (weight 5 out of 10)
    let info = message_info(&alice, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::Claim {}).unwrap();

    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send { amount, .. }) => {
            assert_eq!(amount, &coins(500, "uatom"));
        }
        _ => panic!("Expected BankMsg::Send"),
    }

    // Bob claims: should get 500
    let info = message_info(&bob, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::Claim {}).unwrap();

    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send { amount, .. }) => {
            assert_eq!(amount, &coins(500, "uatom"));
        }
        _ => panic!("Expected BankMsg::Send"),
    }
}

#[test]
fn test_expired_claims_cleaned_up_on_claim() {
    let (mut deps, mut env) = setup_contract();
    let admin = deps.api.addr_make("admin");
    let alice = deps.api.addr_make("alice");

    // Create an expiring distribution
    let expiry = Timestamp::from_seconds(env.block.time.seconds() + 100);
    let info = message_info(&admin, &coins(500, "uatom"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateDistribution {
            claims: vec![ClaimEntry {
                address: alice.to_string(),
                weight: Uint128::new(1),
            }],
            expiry,
        },
    )
    .unwrap();

    // Create a non-expiring distribution
    let expiry2 = Timestamp::from_seconds(env.block.time.seconds() + 10000);
    let info = message_info(&admin, &coins(300, "uatom"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateDistribution {
            claims: vec![ClaimEntry {
                address: alice.to_string(),
                weight: Uint128::new(1),
            }],
            expiry: expiry2,
        },
    )
    .unwrap();

    // Advance past first distribution's expiry
    env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 200);

    // Alice claims: should only get 300 from dist 1, dist 0 expired and cleaned up
    let info = message_info(&alice, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::Claim {}).unwrap();

    match &res.messages[0].msg {
        cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send { amount, .. }) => {
            assert_eq!(amount, &coins(300, "uatom"));
        }
        _ => panic!("Expected BankMsg::Send"),
    }

    // Alice claims again: no pending claims (expired one was cleaned up too)
    let info = message_info(&alice, &[]);
    let err = execute(deps.as_mut(), env.clone(), info, ExecuteMsg::Claim {}).unwrap_err();
    assert_eq!(err, ContractError::NoPendingClaims);
}

#[test]
fn test_claim_history_recorded() {
    let (mut deps, env) = setup_contract();
    let admin = deps.api.addr_make("admin");
    let alice = deps.api.addr_make("alice");
    let bob = deps.api.addr_make("bob");

    let expiry = Timestamp::from_seconds(env.block.time.seconds() + 3600);

    // Create two distributions
    let info = message_info(&admin, &coins(1000, "uatom"));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateDistribution {
            claims: vec![
                ClaimEntry {
                    address: alice.to_string(),
                    weight: Uint128::new(1),
                },
                ClaimEntry {
                    address: bob.to_string(),
                    weight: Uint128::new(1),
                },
            ],
            expiry,
        },
    )
    .unwrap();

    let info = message_info(&admin, &[coin(500, "uusdc")]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreateDistribution {
            claims: vec![ClaimEntry {
                address: alice.to_string(),
                weight: Uint128::new(1),
            }],
            expiry,
        },
    )
    .unwrap();

    // Alice claims all
    let info = message_info(&alice, &[]);
    execute(deps.as_mut(), env.clone(), info, ExecuteMsg::Claim {}).unwrap();

    // Query claim history
    let res: ClaimHistoryResponse = cosmwasm_std::from_json(
        query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::ClaimHistory {
                user: alice.to_string(),
                start_after: None,
                limit: None,
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(res.claims.len(), 2);
    assert_eq!(res.claims[0].distribution_id, 0);
    assert_eq!(res.claims[0].funds_claimed, coins(500, "uatom"));
    assert_eq!(res.claims[0].claimed_at, env.block.time);
    assert_eq!(res.claims[1].distribution_id, 1);
    assert_eq!(res.claims[1].funds_claimed, coins(500, "uusdc"));
}

#[test]
fn test_claim_history_pagination() {
    let (mut deps, env) = setup_contract();
    let admin = deps.api.addr_make("admin");
    let alice = deps.api.addr_make("alice");

    let expiry = Timestamp::from_seconds(env.block.time.seconds() + 3600);

    // Create 5 distributions for alice
    for _ in 0..5 {
        let info = message_info(&admin, &coins(100, "uatom"));
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::CreateDistribution {
                claims: vec![ClaimEntry {
                    address: alice.to_string(),
                    weight: Uint128::new(1),
                }],
                expiry,
            },
        )
        .unwrap();
    }

    // Alice claims all
    let info = message_info(&alice, &[]);
    execute(deps.as_mut(), env.clone(), info, ExecuteMsg::Claim {}).unwrap();

    // Page 1: limit 2
    let res: ClaimHistoryResponse = cosmwasm_std::from_json(
        query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::ClaimHistory {
                user: alice.to_string(),
                start_after: None,
                limit: Some(2),
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(res.claims.len(), 2);
    assert_eq!(res.claims[0].distribution_id, 0);
    assert_eq!(res.claims[1].distribution_id, 1);

    // Page 2: start_after last id from page 1
    let res: ClaimHistoryResponse = cosmwasm_std::from_json(
        query(
            deps.as_ref(),
            env.clone(),
            QueryMsg::ClaimHistory {
                user: alice.to_string(),
                start_after: Some(1),
                limit: Some(2),
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(res.claims.len(), 2);
    assert_eq!(res.claims[0].distribution_id, 2);
    assert_eq!(res.claims[1].distribution_id, 3);

    // Page 3: only 1 remaining
    let res: ClaimHistoryResponse = cosmwasm_std::from_json(
        query(
            deps.as_ref(),
            env,
            QueryMsg::ClaimHistory {
                user: alice.to_string(),
                start_after: Some(3),
                limit: Some(2),
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(res.claims.len(), 1);
    assert_eq!(res.claims[0].distribution_id, 4);
}
