use std::{collections::HashMap, str::FromStr};

use cosmwasm_std::{
    testing::{mock_env, MockApi, MockStorage},
    Binary, Coin, CosmosMsg, Decimal, Env, IbcMsg, IbcTimeout, OwnedDeps,
};
use cw2::{get_contract_version, set_contract_version};

use crate::{
    contract::{
        execute, instantiate, query_all_user_lockups, query_can_lock_denom,
        query_user_voting_power, CONTRACT_NAME, CONTRACT_VERSION,
    },
    error::ContractError,
    migration::migrate::{migrate, MigrateMsg, DEFAULT_STRIDE_TRANSFER_TIMEOUT_SECONDS},
    msg::ExecuteMsg,
    state::BLOCKED_LOCK_DENOMS,
    testing::{
        get_default_instantiate_msg, get_message_info, get_st_atom_denom_info_mock_data,
        get_validator_info_mock_data, setup_multiple_token_info_provider_mocks,
        LSM_TOKEN_PROVIDER_ADDR, ST_ATOM_ON_NEUTRON, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1,
    },
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock, MockQuerier},
};

const STRIDE_CHANNEL_ID: &str = "channel-391";
const STRIDE_RECEIVER: &str = "stride14ug0e20ljt62lqz9qw27z2fcxyd8lch26dt7zcnr2qlhwe6azqjsgq6lwm";
const DEPLOYED_VERSION: &str = "3.6.16";
const CONTRACT_STATOM_BALANCE: u128 = 13_658_715_720;
const USER: &str = "addr0002";

type Deps = OwnedDeps<MockStorage, MockApi, MockQuerier>;
type MsgMutation = Box<dyn Fn(&mut MigrateMsg)>;

fn get_migrate_msg() -> MigrateMsg {
    MigrateMsg {
        statom_denom: ST_ATOM_ON_NEUTRON.to_string(),
        stride_channel_id: STRIDE_CHANNEL_ID.to_string(),
        stride_receiver: STRIDE_RECEIVER.to_string(),
        transfer_timeout_seconds: None,
    }
}

// Instantiates the contract with stATOM and LSM token info providers, has a user lock
// stATOM and LSM tokens, and sets the stored contract version to the deployed one.
fn setup() -> (Deps, Env, u64) {
    let (mut deps, mut env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());

    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = instantiate_msg.lock_epoch_length;
    let lock_epoch_length = instantiate_msg.lock_epoch_length;

    let admin_info = get_message_info(&deps.api, "addr0001", &[]);
    instantiate(deps.as_mut(), env.clone(), admin_info, instantiate_msg).unwrap();

    let st_token_info_provider_addr = deps.api.addr_make("sttoken_info_provider");
    let derivative_providers = HashMap::from([get_st_atom_denom_info_mock_data(
        st_token_info_provider_addr.to_string(),
        (0..=3)
            .map(|round_id: u64| (round_id, Decimal::from_str("1.6").unwrap()))
            .collect(),
    )]);

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    let lsm_provider = Some((
        lsm_token_info_provider_addr.to_string(),
        HashMap::from_iter((0..=3).map(|round_id: u64| {
            (
                round_id,
                HashMap::from([get_validator_info_mock_data(
                    VALIDATOR_1.to_string(),
                    Decimal::one(),
                )]),
            )
        })),
    ));

    setup_multiple_token_info_provider_mocks(&mut deps, derivative_providers, lsm_provider, true);

    env.block.time = env.block.time.plus_days(1);

    // Existing stATOM lockup (lock id 0) and LSM lockup (lock id 1)
    for denom in [ST_ATOM_ON_NEUTRON, VALIDATOR_1_LST_DENOM_1] {
        let info = get_message_info(&deps.api, USER, &[Coin::new(1000u128, denom)]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::LockTokens {
                lock_duration: 3 * lock_epoch_length,
                proof: None,
            },
        )
        .unwrap();
    }

    set_contract_version(&mut deps.storage, CONTRACT_NAME, DEPLOYED_VERSION).unwrap();

    (deps, env, lock_epoch_length)
}

fn lock(deps: &mut Deps, env: &Env, denom: &str, duration: u64) -> Result<(), ContractError> {
    let info = get_message_info(&deps.api, USER, &[Coin::new(500u128, denom)]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::LockTokens {
            lock_duration: duration,
            proof: None,
        },
    )
    .map(|_| ())
}

#[test]
fn migrate_sends_all_statom_to_stride_and_blocks_statom_locks() {
    let (mut deps, env, lock_epoch_length) = setup();

    let contract_addr = env.contract.address.to_string();
    deps.querier.update_balance(
        contract_addr.clone(),
        vec![
            Coin::new(CONTRACT_STATOM_BALANCE, ST_ATOM_ON_NEUTRON),
            Coin::new(1000u128, VALIDATOR_1_LST_DENOM_1),
        ],
    );

    let user = deps.api.addr_make(USER).to_string();
    let lockups_before = query_all_user_lockups(&deps.as_ref(), &env, user.clone(), 0, 100)
        .unwrap()
        .lockups;
    let voting_power_before = query_user_voting_power(deps.as_ref(), env.clone(), user.clone())
        .unwrap()
        .voting_power;
    assert_eq!(lockups_before.len(), 2);
    assert!(voting_power_before > 0);

    let res = migrate(deps.as_mut(), env.clone(), get_migrate_msg()).unwrap();

    // Exactly one message: an ICS-20 transfer of the full stATOM balance to Stride
    assert_eq!(res.messages.len(), 1);
    assert_eq!(
        res.messages[0].msg,
        CosmosMsg::Ibc(IbcMsg::Transfer {
            channel_id: STRIDE_CHANNEL_ID.to_string(),
            to_address: STRIDE_RECEIVER.to_string(),
            amount: Coin::new(CONTRACT_STATOM_BALANCE, ST_ATOM_ON_NEUTRON),
            timeout: IbcTimeout::with_timestamp(
                env.block
                    .time
                    .plus_seconds(DEFAULT_STRIDE_TRANSFER_TIMEOUT_SECONDS)
            ),
            memo: None,
        })
    );
    assert_eq!(DEFAULT_STRIDE_TRANSFER_TIMEOUT_SECONDS, 14 * 24 * 60 * 60);

    assert_eq!(
        BLOCKED_LOCK_DENOMS.load(&deps.storage).unwrap(),
        vec![ST_ATOM_ON_NEUTRON.to_string()]
    );
    assert_eq!(
        get_contract_version(&deps.storage).unwrap().version,
        CONTRACT_VERSION
    );

    // Existing lockups (including the stATOM one) and voting power are unchanged
    let lockups_after = query_all_user_lockups(&deps.as_ref(), &env, user.clone(), 0, 100)
        .unwrap()
        .lockups;
    assert_eq!(lockups_before, lockups_after);
    assert_eq!(
        voting_power_before,
        query_user_voting_power(deps.as_ref(), env.clone(), user.clone())
            .unwrap()
            .voting_power
    );

    // New stATOM locks are rejected
    let err = lock(&mut deps, &env, ST_ATOM_ON_NEUTRON, 3 * lock_epoch_length).unwrap_err();
    assert!(
        matches!(err, ContractError::LockDenomBlocked { ref denom } if denom == ST_ATOM_ON_NEUTRON)
    );
    assert!(err.to_string().contains(&format!(
        "Denom {ST_ATOM_ON_NEUTRON} can no longer be locked"
    )));

    // Locking stATOM and sending the lockup NFT is rejected as well
    let info = get_message_info(&deps.api, USER, &[Coin::new(500u128, ST_ATOM_ON_NEUTRON)]);
    let nft_receiver = deps.api.addr_make("nft_receiver").to_string();
    let err = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::LockTokensThenSendNft {
            lock_duration: 3 * lock_epoch_length,
            proof: None,
            contract: nft_receiver,
            msg: Binary::default(),
        },
    )
    .unwrap_err();
    assert!(matches!(err, ContractError::LockDenomBlocked { .. }));

    // Converting an existing (LSM) lockup into stATOM is rejected
    let info = get_message_info(&deps.api, USER, &[]);
    let err = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ConvertLockup {
            lock_id: 1,
            target_denom: ST_ATOM_ON_NEUTRON.to_string(),
        },
    )
    .unwrap_err();
    assert!(matches!(err, ContractError::LockDenomBlocked { .. }));

    // CanLockDenom reports stATOM as no longer lockable, LSM still lockable
    assert!(
        !query_can_lock_denom(&deps.as_ref(), &env, ST_ATOM_ON_NEUTRON.to_string())
            .unwrap()
            .can_be_locked
    );
    assert!(
        query_can_lock_denom(&deps.as_ref(), &env, VALIDATOR_1_LST_DENOM_1.to_string())
            .unwrap()
            .can_be_locked
    );

    // Other denoms (LSM) can still be locked
    lock(
        &mut deps,
        &env,
        VALIDATOR_1_LST_DENOM_1,
        3 * lock_epoch_length,
    )
    .unwrap();
    assert_eq!(
        query_all_user_lockups(&deps.as_ref(), &env, user, 0, 100)
            .unwrap()
            .lockups
            .len(),
        3
    );

    // Migrating again to the same version fails
    assert!(migrate(deps.as_mut(), env.clone(), get_migrate_msg()).is_err());
}

#[test]
fn migrate_with_zero_statom_balance_sends_nothing() {
    let (mut deps, env, lock_epoch_length) = setup();

    let res = migrate(deps.as_mut(), env.clone(), get_migrate_msg()).unwrap();
    assert!(res.messages.is_empty());

    // stATOM is blocked nonetheless
    assert_eq!(
        BLOCKED_LOCK_DENOMS.load(&deps.storage).unwrap(),
        vec![ST_ATOM_ON_NEUTRON.to_string()]
    );
    assert!(matches!(
        lock(&mut deps, &env, ST_ATOM_ON_NEUTRON, lock_epoch_length).unwrap_err(),
        ContractError::LockDenomBlocked { .. }
    ));
}

#[test]
fn migrate_uses_custom_timeout() {
    let (mut deps, env, _) = setup();
    deps.querier.update_balance(
        env.contract.address.to_string(),
        vec![Coin::new(42u128, ST_ATOM_ON_NEUTRON)],
    );

    let mut msg = get_migrate_msg();
    msg.transfer_timeout_seconds = Some(3600);

    let res = migrate(deps.as_mut(), env.clone(), msg).unwrap();
    assert_eq!(res.messages.len(), 1);
    match &res.messages[0].msg {
        CosmosMsg::Ibc(IbcMsg::Transfer {
            amount, timeout, ..
        }) => {
            assert_eq!(amount, &Coin::new(42u128, ST_ATOM_ON_NEUTRON));
            assert_eq!(
                timeout,
                &IbcTimeout::with_timestamp(env.block.time.plus_seconds(3600))
            );
        }
        other => panic!("unexpected message: {other:?}"),
    }
}

#[test]
fn migrate_msg_validation() {
    let cases: Vec<(MsgMutation, &str)> = vec![
        (
            Box::new(|m| m.statom_denom = "".to_string()),
            "statom_denom must not be empty",
        ),
        (
            Box::new(|m| m.stride_channel_id = " ".to_string()),
            "stride_channel_id must not be empty",
        ),
        (
            Box::new(|m| m.stride_receiver = "".to_string()),
            "stride_receiver must be a Stride address",
        ),
        (
            Box::new(|m| m.stride_receiver = "stride1".to_string()),
            "stride_receiver must be a Stride address",
        ),
        (
            Box::new(|m| {
                m.stride_receiver =
                    "cosmos1tj45y3729uu07pv8hq50pfx9u790rh6e0qssucs3yusxaag32t8svhlxgw".to_string()
            }),
            "stride_receiver must be a Stride address",
        ),
        (
            Box::new(|m| m.transfer_timeout_seconds = Some(0)),
            "transfer_timeout_seconds must be greater than zero",
        ),
    ];

    for (mutate, expected_error) in cases {
        let (mut deps, env, _) = setup();
        let mut msg = get_migrate_msg();
        mutate(&mut msg);

        let err = migrate(deps.as_mut(), env, msg).unwrap_err();
        assert!(
            err.to_string().contains(expected_error),
            "expected '{expected_error}', got '{err}'"
        );

        // Nothing was stored
        assert!(BLOCKED_LOCK_DENOMS
            .may_load(&deps.storage)
            .unwrap()
            .is_none());
        assert_eq!(
            get_contract_version(&deps.storage).unwrap().version,
            DEPLOYED_VERSION
        );
    }
}
