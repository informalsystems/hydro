use std::collections::HashMap;

use cosmwasm_std::{testing::mock_env, Addr, Coin, Decimal, Timestamp, Uint128};
use cw2::{get_contract_version, set_contract_version};
use cw_storage_plus::{Item, Map};

use crate::{
    contract::{instantiate, CONTRACT_NAME},
    migration::{
        migrate::{migrate, CONTRACT_VERSION_V1_1_0, CONTRACT_VERSION_V2_0_2},
        v1_1_0::{ConstantsV1_1_0, ProposalV1_1_0, VoteV1_1_0},
        v2_0_1::{ConstantsV2_0_1, MigrateMsgV2_0_1, ProposalV2_0_1, VoteV2_0_1},
    },
    state::{LockEntry, LOCKS_MAP},
    testing::{
        get_default_instantiate_msg, get_message_info, IBC_DENOM_1, IBC_DENOM_2, IBC_DENOM_3,
        VALIDATOR_1, VALIDATOR_1_LST_DENOM_1, VALIDATOR_2, VALIDATOR_2_LST_DENOM_1,
        VALIDATOR_3_LST_DENOM_1,
    },
    testing_lsm_integration::set_validator_infos_for_round,
    testing_mocks::{denom_trace_grpc_query_mock, mock_dependencies, no_op_grpc_query_mock},
};

struct LockToInsert {
    pub voter: Addr,
    pub lock: LockEntry,
}

struct ExpectedUserVote {
    pub round_id: u64,
    pub tranche_id: u64,
    pub voter: Addr,
    pub lock_id: u64,
    pub proposal_id: Option<u64>,
    pub time_weighted_shares: Option<(String, Decimal)>,
}

#[test]
fn test_constants_and_proposals_migration() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());

    let user_addr = "addr0000";
    let info = get_message_info(&deps.api, user_addr, &[]);

    // Instantiate the contract
    let instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Override contract version so that we can run the migration
    let res = set_contract_version(&mut deps.storage, CONTRACT_NAME, CONTRACT_VERSION_V1_1_0);
    assert!(
        res.is_ok(),
        "failed to set contract version before running the migration"
    );

    const OLD_CONSTANTS: Item<ConstantsV1_1_0> = Item::new("constants");
    const NEW_CONSTANTS: Item<ConstantsV2_0_1> = Item::new("constants");

    const OLD_PROPOSAL_MAP: Map<(u64, u64, u64), ProposalV1_1_0> = Map::new("prop_map");
    const NEW_PROPOSAL_MAP: Map<(u64, u64, u64), ProposalV2_0_1> = Map::new("prop_map");

    // Override the constants so that they have old data structure stored before running the migration
    let old_constants = ConstantsV1_1_0 {
        round_length: 2628000000000000,
        lock_epoch_length: 2628000000000000,
        first_round_start: Timestamp::from_nanos(1730851140000000000),
        max_locked_tokens: 20000000000,
        max_validator_shares_participating: 500,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-1".to_string(),
        icq_update_period: 109000,
        paused: false,
        is_in_pilot_mode: true,
    };

    let res = OLD_CONSTANTS.save(&mut deps.storage, &old_constants);
    assert!(
        res.is_ok(),
        "failed to save old constants before running the migration"
    );

    let old_proposals = vec![
        ProposalV1_1_0 {
            round_id: 0,
            tranche_id: 1,
            proposal_id: 0,
            power: Uint128::new(250),
            percentage: Uint128::zero(),
            title: "proposal 1 title".to_string(),
            description: "proposal 1 description".to_string(),
        },
        ProposalV1_1_0 {
            round_id: 0,
            tranche_id: 1,
            proposal_id: 1,
            power: Uint128::new(750),
            percentage: Uint128::zero(),
            title: "proposal 2 title".to_string(),
            description: "proposal 2 description".to_string(),
        },
        ProposalV1_1_0 {
            round_id: 1,
            tranche_id: 1,
            proposal_id: 2,
            power: Uint128::new(10000),
            percentage: Uint128::zero(),
            title: "proposal 3 title".to_string(),
            description: "proposal 3 description".to_string(),
        },
    ];

    for old_proposal in &old_proposals {
        let res = OLD_PROPOSAL_MAP.save(
            &mut deps.storage,
            (
                old_proposal.round_id,
                old_proposal.tranche_id,
                old_proposal.proposal_id,
            ),
            old_proposal,
        );
        assert!(
            res.is_ok(),
            "failed to save old proposals before running the migration"
        )
    }

    // Run the migration
    let migrate_msg = MigrateMsgV2_0_1 {
        max_deployment_duration: 12,
    };
    let res = migrate(deps.as_mut(), env.clone(), migrate_msg.clone());
    assert!(res.is_ok(), "migration failed!");

    // Verify that the Constants got migrated properly
    let expected_new_constants = ConstantsV2_0_1 {
        round_length: old_constants.round_length,
        lock_epoch_length: old_constants.lock_epoch_length,
        first_round_start: old_constants.first_round_start,
        max_locked_tokens: old_constants.max_locked_tokens,
        max_validator_shares_participating: old_constants.max_validator_shares_participating,
        hub_connection_id: old_constants.hub_connection_id,
        hub_transfer_channel_id: old_constants.hub_transfer_channel_id,
        icq_update_period: old_constants.icq_update_period,
        paused: old_constants.paused,
        is_in_pilot_mode: old_constants.is_in_pilot_mode,
        max_deployment_duration: migrate_msg.max_deployment_duration,
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

    // Verify that the proposals got migrated properly
    for old_proposal in old_proposals {
        let res = NEW_PROPOSAL_MAP.load(
            &deps.storage,
            (
                old_proposal.round_id,
                old_proposal.tranche_id,
                old_proposal.proposal_id,
            ),
        );
        assert!(
            res.is_ok(),
            "failed to load proposal from store after running the migration"
        );
        let new_proposal = res.unwrap();

        let expected_new_proposal = ProposalV2_0_1 {
            round_id: old_proposal.round_id,
            tranche_id: old_proposal.tranche_id,
            proposal_id: old_proposal.proposal_id,
            power: old_proposal.power,
            percentage: old_proposal.percentage,
            title: old_proposal.title,
            description: old_proposal.description,
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        };
        assert_eq!(
            new_proposal, expected_new_proposal,
            "migrated proposal not equal to expected one"
        );
    }

    // Verify the contract version after running the migration
    let res = get_contract_version(&deps.storage);
    assert_eq!(res.unwrap().version, CONTRACT_VERSION_V2_0_2.to_string());
}

#[test]
fn test_votes_migration() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-1".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
            (IBC_DENOM_3.to_string(), VALIDATOR_3_LST_DENOM_1.to_string()),
        ]),
    );

    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let round_id = 0;
    let tranche_id = 1;

    // Set top N validator info
    let result = set_validator_infos_for_round(
        &mut deps.storage,
        round_id,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    );
    assert!(result.is_ok());

    let instantiate_addr = "addr0000";
    let info = get_message_info(&deps.api, instantiate_addr, &[]);

    // Instantiate the contract
    let instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Override contract version so that the migration can be run
    let res = set_contract_version(&mut deps.storage, CONTRACT_NAME, CONTRACT_VERSION_V1_1_0);
    assert!(
        res.is_ok(),
        "failed to set contract version before running the migration"
    );

    // Override the constants so that they have old data structure stored before running the migration
    let first_round_start = 1730851140000000000;
    let lock_epoch_length = 2628000000000000;

    let old_constants = ConstantsV1_1_0 {
        round_length: lock_epoch_length,
        lock_epoch_length,
        first_round_start: Timestamp::from_nanos(first_round_start),
        max_locked_tokens: 20000000000,
        max_validator_shares_participating: 500,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-1".to_string(),
        icq_update_period: 109000,
        paused: false,
        is_in_pilot_mode: true,
    };

    const OLD_CONSTANTS: Item<ConstantsV1_1_0> = Item::new("constants");
    let res = OLD_CONSTANTS.save(&mut deps.storage, &old_constants);
    assert!(
        res.is_ok(),
        "failed to save old constants before running the migration"
    );

    let user1_address = deps.api.addr_make("addr0001");
    let user2_address = deps.api.addr_make("addr0002");

    let user_lockups = vec![
        // lockup that gives 1x power
        LockToInsert {
            voter: user1_address.clone(),
            lock: LockEntry {
                lock_id: 0,
                funds: Coin::new(11u128, IBC_DENOM_1),
                lock_start: Timestamp::from_nanos(first_round_start + 10000),
                lock_end: Timestamp::from_nanos(first_round_start + 10000 + lock_epoch_length),
            },
        },
        // lockup that gives 1x power
        LockToInsert {
            voter: user1_address.clone(),
            lock: LockEntry {
                lock_id: 1,
                funds: Coin::new(32u128, IBC_DENOM_1),
                lock_start: Timestamp::from_nanos(first_round_start + 20000),
                lock_end: Timestamp::from_nanos(first_round_start + 20000 + lock_epoch_length),
            },
        },
        // lockup that gives 1x power
        LockToInsert {
            voter: user1_address.clone(),
            lock: LockEntry {
                lock_id: 2,
                funds: Coin::new(53u128, IBC_DENOM_2),
                lock_start: Timestamp::from_nanos(first_round_start + 30000),
                lock_end: Timestamp::from_nanos(first_round_start + 30000 + lock_epoch_length),
            },
        },
        // simulates an expired lockup, even though we will not have such lockups in the first round
        LockToInsert {
            voter: user1_address.clone(),
            lock: LockEntry {
                lock_id: 3,
                funds: Coin::new(74u128, IBC_DENOM_2),
                lock_start: Timestamp::from_nanos(first_round_start - 40000),
                lock_end: Timestamp::from_nanos(first_round_start - 40000 + lock_epoch_length),
            },
        },
        // lockup that gives 1x power
        LockToInsert {
            voter: user2_address.clone(),
            lock: LockEntry {
                lock_id: 4,
                funds: Coin::new(1u128, IBC_DENOM_1),
                lock_start: Timestamp::from_nanos(first_round_start + 10000),
                lock_end: Timestamp::from_nanos(first_round_start + 10000 + lock_epoch_length),
            },
        },
        // lockup that gives 1x power
        LockToInsert {
            voter: user2_address.clone(),
            lock: LockEntry {
                lock_id: 5,
                funds: Coin::new(2u128, IBC_DENOM_1),
                lock_start: Timestamp::from_nanos(first_round_start + 20000),
                lock_end: Timestamp::from_nanos(first_round_start + 20000 + lock_epoch_length),
            },
        },
        // lockup that gives 1x power
        LockToInsert {
            voter: user2_address.clone(),
            lock: LockEntry {
                lock_id: 6,
                funds: Coin::new(3u128, IBC_DENOM_2),
                lock_start: Timestamp::from_nanos(first_round_start + 30000),
                lock_end: Timestamp::from_nanos(first_round_start + 30000 + lock_epoch_length),
            },
        },
        // simulates an expired lockup, even though we will not have such lockups in the first round
        LockToInsert {
            voter: user2_address.clone(),
            lock: LockEntry {
                lock_id: 7,
                funds: Coin::new(4u128, IBC_DENOM_2),
                lock_start: Timestamp::from_nanos(first_round_start - 40000),
                lock_end: Timestamp::from_nanos(first_round_start - 40000 + lock_epoch_length),
            },
        },
    ];

    for user_lockup in &user_lockups {
        let res = LOCKS_MAP.save(
            &mut deps.storage,
            (user_lockup.voter.clone(), user_lockup.lock.lock_id),
            &user_lockup.lock,
        );
        assert!(
            res.is_ok(),
            "failed to save lockup into the store before running the migration"
        );
    }

    // Create user votes for arbitrary proposals and with the scaled voting power summed up from all user lockups
    let user1_validator1_voting_power =
        user_lockups[0].lock.funds.amount + user_lockups[1].lock.funds.amount;
    let user1_validator2_voting_power = user_lockups[2].lock.funds.amount;

    let user2_validator1_voting_power =
        user_lockups[4].lock.funds.amount + user_lockups[5].lock.funds.amount;
    let user2_validator2_voting_power = user_lockups[6].lock.funds.amount;

    let user1_voted_proposal = 3;
    let user2_voted_proposal = 7;

    let old_user_votes = vec![
        (
            user1_address.clone(),
            VoteV1_1_0 {
                prop_id: user1_voted_proposal,
                time_weighted_shares: HashMap::from([
                    (
                        VALIDATOR_1.to_string(),
                        Decimal::from_ratio(user1_validator1_voting_power, Uint128::one()),
                    ),
                    (
                        VALIDATOR_2.to_string(),
                        Decimal::from_ratio(user1_validator2_voting_power, Uint128::one()),
                    ),
                ]),
            },
        ),
        (
            user2_address.clone(),
            VoteV1_1_0 {
                prop_id: user2_voted_proposal,
                time_weighted_shares: HashMap::from([
                    (
                        VALIDATOR_1.to_string(),
                        Decimal::from_ratio(user2_validator1_voting_power, Uint128::one()),
                    ),
                    (
                        VALIDATOR_2.to_string(),
                        Decimal::from_ratio(user2_validator2_voting_power, Uint128::one()),
                    ),
                ]),
            },
        ),
    ];

    const OLD_VOTE_MAP: Map<(u64, u64, Addr), VoteV1_1_0> = Map::new("vote_map");
    const NEW_VOTE_MAP: Map<((u64, u64), Addr, u64), VoteV2_0_1> = Map::new("vote_map");

    for user_vote in &old_user_votes {
        let res = OLD_VOTE_MAP.save(
            &mut deps.storage,
            (round_id, tranche_id, user_vote.0.clone()),
            &user_vote.1,
        );
        assert!(
            res.is_ok(),
            "failed to save user vote into the store before running the migration"
        );
    }

    // Run the migration
    let migrate_msg = MigrateMsgV2_0_1 {
        max_deployment_duration: 12,
    };
    let res = migrate(deps.as_mut(), env.clone(), migrate_msg);
    assert!(res.is_ok(), "migration failed!");

    let expected_votes = vec![
        ExpectedUserVote {
            round_id,
            tranche_id,
            voter: user1_address.clone(),
            lock_id: 0,
            proposal_id: Some(user1_voted_proposal),
            time_weighted_shares: Some((
                VALIDATOR_1.to_string(),
                Decimal::from_ratio(user_lockups[0].lock.funds.amount, Uint128::one()),
            )),
        },
        ExpectedUserVote {
            round_id,
            tranche_id,
            voter: user1_address.clone(),
            lock_id: 1,
            proposal_id: Some(user1_voted_proposal),
            time_weighted_shares: Some((
                VALIDATOR_1.to_string(),
                Decimal::from_ratio(user_lockups[1].lock.funds.amount, Uint128::one()),
            )),
        },
        ExpectedUserVote {
            round_id,
            tranche_id,
            voter: user1_address.clone(),
            lock_id: 2,
            proposal_id: Some(user1_voted_proposal),
            time_weighted_shares: Some((
                VALIDATOR_2.to_string(),
                Decimal::from_ratio(user_lockups[2].lock.funds.amount, Uint128::one()),
            )),
        },
        ExpectedUserVote {
            round_id,
            tranche_id,
            voter: user1_address.clone(),
            lock_id: 3,
            proposal_id: None,
            time_weighted_shares: None,
        },
        ExpectedUserVote {
            round_id,
            tranche_id,
            voter: user2_address.clone(),
            lock_id: 4,
            proposal_id: Some(user2_voted_proposal),
            time_weighted_shares: Some((
                VALIDATOR_1.to_string(),
                Decimal::from_ratio(user_lockups[4].lock.funds.amount, Uint128::one()),
            )),
        },
        ExpectedUserVote {
            round_id,
            tranche_id,
            voter: user2_address.clone(),
            lock_id: 5,
            proposal_id: Some(user2_voted_proposal),
            time_weighted_shares: Some((
                VALIDATOR_1.to_string(),
                Decimal::from_ratio(user_lockups[5].lock.funds.amount, Uint128::one()),
            )),
        },
        ExpectedUserVote {
            round_id,
            tranche_id,
            voter: user2_address.clone(),
            lock_id: 6,
            proposal_id: Some(user2_voted_proposal),
            time_weighted_shares: Some((
                VALIDATOR_2.to_string(),
                Decimal::from_ratio(user_lockups[6].lock.funds.amount, Uint128::one()),
            )),
        },
        ExpectedUserVote {
            round_id,
            tranche_id,
            voter: user2_address.clone(),
            lock_id: 7,
            proposal_id: None,
            time_weighted_shares: None,
        },
    ];

    for expected_vote in expected_votes {
        let res = NEW_VOTE_MAP.load(
            &deps.storage,
            (
                (expected_vote.round_id, expected_vote.tranche_id),
                expected_vote.voter,
                expected_vote.lock_id,
            ),
        );

        match expected_vote.proposal_id {
            Some(expected_proposal_id) => {
                assert!(
                    res.is_ok(),
                    "failed to load expected user vote after running the migration"
                );

                let new_vote = res.unwrap();
                let expected_time_weighted_shares = expected_vote.time_weighted_shares.unwrap();

                assert_eq!(new_vote.prop_id, expected_proposal_id);
                assert_eq!(
                    new_vote.time_weighted_shares.0,
                    expected_time_weighted_shares.0
                );
                assert_eq!(
                    new_vote.time_weighted_shares.1,
                    expected_time_weighted_shares.1
                );
            }
            None => {
                assert!(
                    res.is_err(),
                    "loaded user vote that shouldn't exist after running the migration"
                );
            }
        }
    }

    // Verify that old votes are removed
    for user_vote in old_user_votes {
        let res = OLD_VOTE_MAP.load(&deps.storage, (round_id, tranche_id, user_vote.0));
        assert!(
            res.unwrap_err().to_string().contains("not found"),
            "loaded old user vote from the store after running the migration"
        );
    }
}
