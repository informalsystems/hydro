use cosmwasm_std::{testing::mock_env, Coin, Decimal, Uint128};

use crate::{
    contract::{execute, instantiate},
    msg::{ExecuteMsg, LockupToMint},
    state::{
        LOCKED_TOKENS, LOCKS_MAP, LOCK_ID, TOKEN_IDS, TOKEN_INFO_PROVIDERS,
        TOTAL_VOTING_POWER_PER_ROUND, USER_LOCKS, USER_LOCKS_FOR_CLAIM,
    },
    testing::{
        get_default_instantiate_msg, get_message_info, setup_lsm_token_info_provider_mock,
        LSM_TOKEN_PROVIDER_ADDR, ONE_MONTH_IN_NANO_SECONDS, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1,
    },
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock},
    token_manager::{TokenInfoProvider, TokenInfoProviderBase},
};

// Mirrors the hardcoded Hub-native denom used in contract::mint_lockups() to decide
// TOKEN_IDS membership. Duplicated here (rather than imported) since it's private
// to the contract module.
const D_ATOM_HUB_DENOM: &str =
    "ibc/AFC2F1B2FD45D549E34445E63921ECDECF1EAC68DA72412C2E087BEB503693F2";

fn register_base_token_info_provider(
    storage: &mut dyn cosmwasm_std::Storage,
    provider_id: &str,
    token_group_id: &str,
    denom: &str,
) {
    TOKEN_INFO_PROVIDERS
        .save(
            storage,
            provider_id.to_string(),
            &TokenInfoProvider::Base(TokenInfoProviderBase {
                token_group_id: token_group_id.to_string(),
                denom: denom.to_string(),
                ratio: Decimal::one(),
            }),
        )
        .unwrap();
}

#[test]
fn mint_lockups_unauthorized_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let admin_info = get_message_info(&deps.api, "admin", &[]);
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), admin_info.clone(), msg);
    assert!(res.is_ok());

    // "admin" is a whitelist admin, not a WHITELIST member, so it should be rejected.
    let owner = get_message_info(&deps.api, "addr0000", &[])
        .sender
        .to_string();
    let res = execute(
        deps.as_mut(),
        env,
        admin_info,
        ExecuteMsg::MintLockups {
            lockups: vec![LockupToMint {
                lock_id: 1,
                owner,
                funds: Coin::new(1000u128, D_ATOM_HUB_DENOM.to_string()),
                lock_start: mock_env().block.time,
                lock_end: mock_env()
                    .block
                    .time
                    .plus_nanos(ONE_MONTH_IN_NANO_SECONDS * 6),
            }],
        },
    );

    assert!(res.unwrap_err().to_string().contains("Unauthorized"));
}

#[test]
fn mint_lockups_basic_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let admin_info = get_message_info(&deps.api, "admin", &[]);
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), admin_info, msg);
    assert!(res.is_ok());

    register_base_token_info_provider(&mut deps.storage, "base_datom", "datom", D_ATOM_HUB_DENOM);

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr,
        vec![(0, vec![(VALIDATOR_1.to_string(), Decimal::one())])],
        true,
    );

    // "addr0000" is the default WHITELIST member set up by get_default_instantiate_msg().
    let whitelisted_info = get_message_info(&deps.api, "addr0000", &[]);
    let owner_1 = get_message_info(&deps.api, "addr0000", &[])
        .sender
        .to_string();
    let owner_2 = get_message_info(&deps.api, "addr0001", &[])
        .sender
        .to_string();

    let lock_start = env.block.time;
    let lock_end = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS * 6);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        whitelisted_info,
        ExecuteMsg::MintLockups {
            lockups: vec![
                LockupToMint {
                    lock_id: 500,
                    owner: owner_1.clone(),
                    funds: Coin::new(1000u128, D_ATOM_HUB_DENOM.to_string()),
                    lock_start,
                    lock_end,
                },
                LockupToMint {
                    lock_id: 501,
                    owner: owner_2.clone(),
                    funds: Coin::new(2000u128, VALIDATOR_1_LST_DENOM_1.to_string()),
                    lock_start,
                    lock_end,
                },
            ],
        },
    );
    assert!(res.is_ok(), "mint_lockups failed: {res:?}");

    let lock_500 = LOCKS_MAP.load(&deps.storage, 500).unwrap();
    assert_eq!(lock_500.owner.to_string(), owner_1);
    assert_eq!(
        lock_500.funds,
        Coin::new(1000u128, D_ATOM_HUB_DENOM.to_string())
    );

    let lock_501 = LOCKS_MAP.load(&deps.storage, 501).unwrap();
    assert_eq!(lock_501.owner.to_string(), owner_2);
    assert_eq!(
        lock_501.funds,
        Coin::new(2000u128, VALIDATOR_1_LST_DENOM_1.to_string())
    );

    // Only the dATOM/stATOM-style lockup should be tracked in TOKEN_IDS.
    assert!(TOKEN_IDS.has(&deps.storage, 500));
    assert!(!TOKEN_IDS.has(&deps.storage, 501));

    assert_eq!(
        USER_LOCKS
            .load(&deps.storage, lock_500.owner.clone())
            .unwrap(),
        vec![500]
    );
    assert_eq!(
        USER_LOCKS_FOR_CLAIM
            .load(&deps.storage, lock_500.owner.clone())
            .unwrap(),
        vec![500]
    );
    assert_eq!(
        USER_LOCKS
            .load(&deps.storage, lock_501.owner.clone())
            .unwrap(),
        vec![501]
    );

    assert_eq!(
        LOCKED_TOKENS.load(&deps.storage).unwrap(),
        1000u128 + 2000u128
    );

    // mint_lockups() does not touch the LOCK_ID counter: the starting point for future,
    // regularly-locked tokens is set once at instantiate time via migrate_info.lock_id.
    assert_eq!(LOCK_ID.load(&deps.storage).unwrap(), 0);

    let total_voting_power = TOTAL_VOTING_POWER_PER_ROUND.load(&deps.storage, 0).unwrap();
    assert!(total_voting_power > Uint128::zero());
}

#[test]
fn mint_lockups_duplicate_lock_id_in_batch_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let admin_info = get_message_info(&deps.api, "admin", &[]);
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), admin_info, msg);
    assert!(res.is_ok());

    register_base_token_info_provider(&mut deps.storage, "base_datom", "datom", D_ATOM_HUB_DENOM);

    let whitelisted_info = get_message_info(&deps.api, "addr0000", &[]);
    let owner = get_message_info(&deps.api, "addr0000", &[])
        .sender
        .to_string();
    let lock_start = env.block.time;
    let lock_end = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS * 6);

    let lockup = LockupToMint {
        lock_id: 42,
        owner,
        funds: Coin::new(1000u128, D_ATOM_HUB_DENOM.to_string()),
        lock_start,
        lock_end,
    };

    let res = execute(
        deps.as_mut(),
        env,
        whitelisted_info,
        ExecuteMsg::MintLockups {
            lockups: vec![lockup.clone(), lockup],
        },
    );

    assert!(res.unwrap_err().to_string().contains("Duplicate lock_id"));
}

#[test]
fn mint_lockups_lock_id_already_exists_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let admin_info = get_message_info(&deps.api, "admin", &[]);
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), admin_info, msg);
    assert!(res.is_ok());

    register_base_token_info_provider(&mut deps.storage, "base_datom", "datom", D_ATOM_HUB_DENOM);

    let whitelisted_info = get_message_info(&deps.api, "addr0000", &[]);
    let owner = get_message_info(&deps.api, "addr0000", &[])
        .sender
        .to_string();
    let lock_start = env.block.time;
    let lock_end = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS * 6);

    let lockup = LockupToMint {
        lock_id: 7,
        owner,
        funds: Coin::new(1000u128, D_ATOM_HUB_DENOM.to_string()),
        lock_start,
        lock_end,
    };

    let res = execute(
        deps.as_mut(),
        env.clone(),
        whitelisted_info.clone(),
        ExecuteMsg::MintLockups {
            lockups: vec![lockup.clone()],
        },
    );
    assert!(res.is_ok());

    let res = execute(
        deps.as_mut(),
        env,
        whitelisted_info,
        ExecuteMsg::MintLockups {
            lockups: vec![lockup],
        },
    );
    assert!(res.unwrap_err().to_string().contains("already exists"));
}
