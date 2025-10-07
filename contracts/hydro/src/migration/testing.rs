use std::{collections::HashMap, str::FromStr};

use cosmwasm_std::{
    testing::{mock_env, MockApi},
    Decimal, MemoryStorage, OwnedDeps, Uint128,
};
use cw2::set_contract_version;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    contract::{instantiate, CONTRACT_NAME},
    migration::migrate::{migrate, MigrateMsg},
    state::{Proposal, Vote, PROPOSAL_MAP, PROPOSAL_TOTAL_MAP, VOTE_MAP_V2},
    testing::{
        get_default_instantiate_msg, get_message_info, setup_st_atom_token_info_provider_mock,
        IBC_DENOM_1, IBC_DENOM_2, ST_ATOM_TOKEN_GROUP, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1,
        VALIDATOR_2, VALIDATOR_2_LST_DENOM_1,
    },
    testing_lsm_integration::set_validator_infos_for_round,
    testing_mocks::{denom_trace_grpc_query_mock, mock_dependencies, MockQuerier},
};

#[test]
fn update_proposals_powers_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
        ]),
    );

    let mut deps = mock_dependencies(grpc_query);
    let env = mock_env();

    let info = get_message_info(&deps.api, "user1", &[]);
    let instantiate_msg = get_default_instantiate_msg(&deps.api);

    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Set the contract version to v3.5.3 in order to be able to run the migration
    set_contract_version(deps.as_mut().storage, CONTRACT_NAME, "3.5.3").unwrap();

    set_validator_infos_for_round(
        &mut deps.storage,
        9,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    )
    .unwrap();

    let st_token_info_provider = deps.api.addr_make("sttoken_info_provider");
    setup_st_atom_token_info_provider_mock(
        &mut deps,
        st_token_info_provider,
        Decimal::from_str("1.7").unwrap(),
    );

    let round_id = 9;
    let tranche_id = 1;

    let proposal1_id = 82;
    let proposal2_id = 83;
    let proposal3_id = 84;
    let proposal4_id = 85;

    let proposal1_initial_power = Uint128::new(15000);
    let proposal2_initial_power = Uint128::new(25000);
    let proposal3_initial_power = Uint128::new(35000);
    let proposal4_initial_power = Uint128::new(5000);

    let proposal_power_to_decrease = Proposal {
        round_id,
        tranche_id,
        proposal_id: proposal1_id,
        title: "Proposal 1".to_string(),
        description: "Proposal 1 Desc".to_string(),
        power: proposal1_initial_power,
        percentage: Uint128::zero(),
        deployment_duration: 3,
        minimum_atom_liquidity_request: Uint128::zero(),
    };

    let proposal_power_to_increase = Proposal {
        round_id,
        tranche_id,
        proposal_id: proposal2_id,
        title: "Proposal 2".to_string(),
        description: "Proposal 2 Desc".to_string(),
        power: proposal2_initial_power,
        percentage: Uint128::zero(),
        deployment_duration: 3,
        minimum_atom_liquidity_request: Uint128::zero(),
    };

    let proposal_power_unchanged = Proposal {
        round_id,
        tranche_id,
        proposal_id: proposal3_id,
        title: "Proposal 3".to_string(),
        description: "Proposal 3 Desc".to_string(),
        power: proposal3_initial_power,
        percentage: Uint128::zero(),
        deployment_duration: 3,
        minimum_atom_liquidity_request: Uint128::zero(),
    };

    let proposal_power_goes_to_zero = Proposal {
        round_id,
        tranche_id,
        proposal_id: proposal4_id,
        title: "Proposal 4".to_string(),
        description: "Proposal 4 Desc".to_string(),
        power: proposal4_initial_power,
        percentage: Uint128::zero(),
        deployment_duration: 3,
        minimum_atom_liquidity_request: Uint128::zero(),
    };

    for proposal in [
        proposal_power_to_decrease,
        proposal_power_to_increase,
        proposal_power_unchanged,
        proposal_power_goes_to_zero,
    ] {
        PROPOSAL_MAP
            .save(
                &mut deps.storage,
                (round_id, tranche_id, proposal.proposal_id),
                &proposal,
            )
            .unwrap();

        PROPOSAL_TOTAL_MAP
            .save(
                &mut deps.storage,
                proposal.proposal_id,
                &Decimal::from_ratio(proposal.power, Uint128::one()),
            )
            .unwrap();
    }

    let proposal1_expected_power = Uint128::new(10700u128);
    let proposal2_expected_power = Uint128::new(35500u128);
    let proposal3_expected_power = Uint128::new(35000u128);
    let proposal4_expected_power = Uint128::new(0u128);

    let mut lockup_id = 783;

    for vote in [
        (proposal1_id, VALIDATOR_1, 3000u128),
        (proposal1_id, VALIDATOR_1, 1000u128),
        (proposal1_id, VALIDATOR_2, 5000u128),
        (proposal1_id, ST_ATOM_TOKEN_GROUP, 1000u128),
        (proposal2_id, ST_ATOM_TOKEN_GROUP, 5000u128),
        (proposal2_id, ST_ATOM_TOKEN_GROUP, 5000u128),
        (proposal2_id, ST_ATOM_TOKEN_GROUP, 5000u128),
        (proposal2_id, VALIDATOR_2, 5000u128),
        (proposal2_id, VALIDATOR_2, 5000u128),
        (proposal3_id, ST_ATOM_TOKEN_GROUP, 10000u128),
        (proposal3_id, ST_ATOM_TOKEN_GROUP, 1000u128),
        (proposal3_id, VALIDATOR_2, 9000u128),
        (proposal3_id, VALIDATOR_2, 7000u128),
        (proposal3_id, VALIDATOR_1, 300u128),
    ] {
        VOTE_MAP_V2
            .save(
                &mut deps.storage,
                ((round_id, tranche_id), lockup_id),
                &Vote {
                    prop_id: vote.0,
                    time_weighted_shares: (
                        vote.1.to_string(),
                        Decimal::from_ratio(vote.2, Uint128::one()),
                    ),
                },
            )
            .unwrap();

        lockup_id += 1;
    }

    // Verify proposals powers before the migration is run
    verify_expected_proposals_powers(
        &deps,
        round_id,
        tranche_id,
        &[
            (proposal1_id, proposal1_initial_power),
            (proposal2_id, proposal2_initial_power),
            (proposal3_id, proposal3_initial_power),
            (proposal4_id, proposal4_initial_power),
        ],
    );

    migrate(
        deps.as_mut(),
        env.clone(),
        MigrateMsg {},
    )
    .unwrap();

    // Verify proposals powers after the migration is run
    verify_expected_proposals_powers(
        &deps,
        round_id,
        tranche_id,
        &[
            (proposal1_id, proposal1_expected_power),
            (proposal2_id, proposal2_expected_power),
            (proposal3_id, proposal3_expected_power),
            (proposal4_id, proposal4_expected_power),
        ],
    );
}

fn verify_expected_proposals_powers(
    deps: &OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    round_id: u64,
    tranche_id: u64,
    expected_proposals_powers: &[(u64, Uint128)],
) {
    for proposal_power in expected_proposals_powers {
        assert_eq!(
            PROPOSAL_MAP
                .load(&deps.storage, (round_id, tranche_id, proposal_power.0))
                .unwrap()
                .power,
            proposal_power.1
        );

        assert_eq!(
            PROPOSAL_TOTAL_MAP
                .load(&deps.storage, proposal_power.0)
                .unwrap(),
            Decimal::from_ratio(proposal_power.1, Uint128::one())
        );
    }
}
