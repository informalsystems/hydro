use std::str::FromStr;

use cosmwasm_std::{testing::mock_env, Addr, Decimal, Timestamp, Uint128};
use cw2::{get_contract_version, set_contract_version};
use cw_storage_plus::Item;

use crate::{
    contract::{instantiate, CONTRACT_NAME},
    migration::{
        migrate::{migrate, CONTRACT_VERSION_V2_0_4, CONTRACT_VERSION_V2_1_0},
        v2_1_0::{ConstantsV2_0_4, ConstantsV2_1_0, MigrateMsgV2_1_0},
    },
    state::{Proposal, RoundLockPowerSchedule, Vote, PROPOSAL_MAP, VOTE_MAP, VOTING_ALLOWED_ROUND},
    testing::{
        get_default_instantiate_msg, get_message_info, ONE_MONTH_IN_NANO_SECONDS, VALIDATOR_1,
    },
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock},
};

use super::v2_1_0::VoteMigrationInfo;

#[test]
fn test_constants_migration() {
    let (mut deps, mut env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());

    let first_round_start = Timestamp::from_nanos(1730851140000000000);
    env.block.time = first_round_start;

    let user_addr = "addr0000";
    let info = get_message_info(&deps.api, user_addr, &[]);

    // Instantiate the contract
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.first_round_start = first_round_start;

    instantiate(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        instantiate_msg.clone(),
    )
    .unwrap();

    // Override contract version so that we can run the migration
    let res = set_contract_version(&mut deps.storage, CONTRACT_NAME, CONTRACT_VERSION_V2_0_4);
    assert!(
        res.is_ok(),
        "failed to set contract version before running the migration"
    );

    const OLD_CONSTANTS: Item<ConstantsV2_0_4> = Item::new("constants");
    const NEW_CONSTANTS: Item<ConstantsV2_1_0> = Item::new("constants");

    // Override the constants so that they have old data structure stored before running the migration
    let old_constants = ConstantsV2_0_4 {
        round_length: 2628000000000000,
        lock_epoch_length: 2628000000000000,
        first_round_start,
        max_locked_tokens: 20000000000,
        max_validator_shares_participating: 500,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-1".to_string(),
        icq_update_period: 109000,
        paused: false,
        is_in_pilot_mode: true,
        max_deployment_duration: 12,
    };

    let res = OLD_CONSTANTS.save(&mut deps.storage, &old_constants);
    assert!(
        res.is_ok(),
        "failed to save old constants before running the migration"
    );

    // advance the chain to move to the round 1
    env.block.time = env.block.time.plus_nanos(instantiate_msg.round_length + 1);

    // Run the migration
    let migrate_msg = MigrateMsgV2_1_0 {};
    let res = migrate(deps.as_mut(), env.clone(), migrate_msg.clone());
    assert!(res.is_ok(), "migration failed: {}", res.unwrap_err());

    // Verify that the Constants got migrated properly
    let expected_new_constants = ConstantsV2_1_0 {
        round_length: old_constants.round_length,
        lock_epoch_length: old_constants.lock_epoch_length,
        first_round_start: old_constants.first_round_start,
        max_locked_tokens: old_constants.max_locked_tokens,
        max_validator_shares_participating: old_constants.max_validator_shares_participating,
        hub_connection_id: old_constants.hub_connection_id,
        hub_transfer_channel_id: old_constants.hub_transfer_channel_id,
        icq_update_period: old_constants.icq_update_period,
        paused: old_constants.paused,
        max_deployment_duration: old_constants.max_deployment_duration,
        round_lock_power_schedule: RoundLockPowerSchedule::new(vec![
            (1, Decimal::from_str("1").unwrap()),
            (2, Decimal::from_str("1.25").unwrap()),
            (3, Decimal::from_str("1.5").unwrap()),
        ]),
    };
    let res = NEW_CONSTANTS.load(&deps.storage);
    assert!(
        res.is_ok(),
        "failed to load new constants after running the migration"
    );
    let new_constants = res.unwrap();
    assert_eq!(
        new_constants, expected_new_constants,
        "migrated constants not equal to expected ones"
    );

    // Verify the contract version after running the migration
    let res = get_contract_version(&deps.storage);
    assert_eq!(res.unwrap().version, CONTRACT_VERSION_V2_1_0.to_string());
}

#[test]
fn test_voting_allowed_info_migration() {
    let (mut deps, mut env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());

    let first_round_start = Timestamp::from_nanos(1730851140000000000);
    env.block.time = first_round_start;

    let user_addr = "addr0000";
    let info = get_message_info(&deps.api, user_addr, &[]);

    // Instantiate the contract
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.first_round_start = first_round_start;
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;

    instantiate(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        instantiate_msg.clone(),
    )
    .unwrap();

    // Override contract version so that we can run the migration
    let res = set_contract_version(&mut deps.storage, CONTRACT_NAME, CONTRACT_VERSION_V2_0_4);
    assert!(
        res.is_ok(),
        "failed to set contract version before running the migration"
    );

    const OLD_CONSTANTS: Item<ConstantsV2_0_4> = Item::new("constants");

    // Override the constants so that they have old data structure stored before running the migration
    let old_constants = ConstantsV2_0_4 {
        round_length: 2628000000000000,
        lock_epoch_length: 2628000000000000,
        first_round_start,
        max_locked_tokens: 20000000000,
        max_validator_shares_participating: 500,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-1".to_string(),
        icq_update_period: 109000,
        paused: false,
        is_in_pilot_mode: true,
        max_deployment_duration: 12,
    };

    let res = OLD_CONSTANTS.save(&mut deps.storage, &old_constants);
    assert!(
        res.is_ok(),
        "failed to save old constants before running the migration"
    );

    // advance the chain to move to the round 1
    env.block.time = env.block.time.plus_nanos(instantiate_msg.round_length + 1);

    let round_id = 1;
    let tranche_id = 1;

    let first_proposal_id = 9;
    let second_proposal_id = 10;

    let first_lock_id = 103;
    let second_lock_id = 104;
    let third_lock_id = 105;
    let fourt_lock_id = 106;
    let fifth_lock_id = 107;

    // save some proposals into the storage
    let proposals = vec![
        Proposal {
            round_id,
            tranche_id,
            proposal_id: first_proposal_id,
            power: Uint128::zero(),
            percentage: Uint128::zero(),
            title: "Proposal 9".to_string(),
            description: "Proposal 9 Description".to_string(),
            deployment_duration: 3,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
        Proposal {
            round_id,
            tranche_id,
            proposal_id: second_proposal_id,
            power: Uint128::zero(),
            percentage: Uint128::zero(),
            title: "Proposal 10".to_string(),
            description: "Proposal 10 Description".to_string(),
            deployment_duration: 4,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    ];

    for proposal in &proposals {
        let res = PROPOSAL_MAP.save(
            &mut deps.storage,
            (round_id, tranche_id, proposal.proposal_id),
            proposal,
        );
        assert!(
            res.is_ok(),
            "failed to save proposal before running the migration: {}",
            res.unwrap_err()
        );
    }

    let voting_allowed_prop_1 = round_id + proposals[0].deployment_duration;
    let voting_allowed_prop_2 = round_id + proposals[1].deployment_duration;

    let user_addr_1 = deps.api.addr_make("addr0001");
    let user_addr_2 = deps.api.addr_make("addr0002");

    let test_infos = vec![
        // user 1, lock 1, voting info already in the store
        VotingInfoMigrationTest {
            vote: (
                user_addr_1.clone(),
                VoteMigrationInfo {
                    lock_id: first_lock_id,
                    proposal_id: first_proposal_id,
                },
            ),
            voting_info_before: Some(VotingAllowedInfoTest {
                lock_id: first_lock_id,
                round_id: voting_allowed_prop_1,
            }),
            voting_info_after: VotingAllowedInfoTest {
                lock_id: first_lock_id,
                round_id: voting_allowed_prop_1,
            },
        },
        // user 1, lock 2, voting info not in the store
        VotingInfoMigrationTest {
            vote: (
                user_addr_1.clone(),
                VoteMigrationInfo {
                    lock_id: second_lock_id,
                    proposal_id: first_proposal_id,
                },
            ),
            voting_info_before: None,
            voting_info_after: VotingAllowedInfoTest {
                lock_id: second_lock_id,
                round_id: voting_allowed_prop_1,
            },
        },
        // user 2, lock 3, voting info already in the store
        VotingInfoMigrationTest {
            vote: (
                user_addr_2.clone(),
                VoteMigrationInfo {
                    lock_id: third_lock_id,
                    proposal_id: second_proposal_id,
                },
            ),
            voting_info_before: Some(VotingAllowedInfoTest {
                lock_id: third_lock_id,
                round_id: voting_allowed_prop_2,
            }),
            voting_info_after: VotingAllowedInfoTest {
                lock_id: third_lock_id,
                round_id: voting_allowed_prop_2,
            },
        },
        // user 2, lock 4, voting info not in the store
        VotingInfoMigrationTest {
            vote: (
                user_addr_2.clone(),
                VoteMigrationInfo {
                    lock_id: fourt_lock_id,
                    proposal_id: second_proposal_id,
                },
            ),
            voting_info_before: None,
            voting_info_after: VotingAllowedInfoTest {
                lock_id: fourt_lock_id,
                round_id: voting_allowed_prop_2,
            },
        },
        // user 2, lock 5, voting info not in the store
        VotingInfoMigrationTest {
            vote: (
                user_addr_2.clone(),
                VoteMigrationInfo {
                    lock_id: fifth_lock_id,
                    proposal_id: second_proposal_id,
                },
            ),
            voting_info_before: None,
            voting_info_after: VotingAllowedInfoTest {
                lock_id: fifth_lock_id,
                round_id: voting_allowed_prop_2,
            },
        },
    ];

    for test_case in &test_infos {
        let time_weighted_shares = (VALIDATOR_1.to_string(), Decimal::one());
        let res = VOTE_MAP.save(
            &mut deps.storage,
            (
                (round_id, tranche_id),
                test_case.vote.0.clone(),
                test_case.vote.1.lock_id,
            ),
            &Vote {
                prop_id: test_case.vote.1.proposal_id,
                time_weighted_shares,
            },
        );
        assert!(
            res.is_ok(),
            "failed to save vote before running the migration: {}",
            res.unwrap_err()
        );

        if let Some(vote_info_before) = &test_case.voting_info_before {
            let res = VOTING_ALLOWED_ROUND.save(
                &mut deps.storage,
                (tranche_id, vote_info_before.lock_id),
                &vote_info_before.round_id,
            );
            assert!(
                res.is_ok(),
                "failed to save voting allowed info before running the migration: {}",
                res.unwrap_err()
            );
        }
    }

    // Run the migration
    let migrate_msg = MigrateMsgV2_1_0 {};
    let res = migrate(deps.as_mut(), env.clone(), migrate_msg.clone());
    assert!(res.is_ok(), "migration failed: {}", res.unwrap_err());

    for test_case in &test_infos {
        let res = VOTING_ALLOWED_ROUND.load(
            &deps.storage,
            (tranche_id, test_case.voting_info_after.lock_id),
        );
        assert!(
            res.is_ok(),
            "voting allowed round not populated after migration: {}",
            res.unwrap_err()
        );

        let voting_allowed_round = res.unwrap();
        assert_eq!(
            voting_allowed_round, test_case.voting_info_after.round_id,
            "voting allowed round doesn't match expected value; got: {}, expected: {}",
            voting_allowed_round, test_case.voting_info_after.round_id
        );
    }
}

struct VotingInfoMigrationTest {
    pub vote: (Addr, VoteMigrationInfo),
    // (lock_id, round_id)
    pub voting_info_before: Option<VotingAllowedInfoTest>,
    // (lock_id, round_id)
    pub voting_info_after: VotingAllowedInfoTest,
}

struct VotingAllowedInfoTest {
    pub lock_id: u64,
    pub round_id: u64,
}
