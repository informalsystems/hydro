use cosmwasm_std::{testing::mock_env, Addr, Decimal, MessageInfo, Order, Uint128};
use cw2::set_contract_version;
use interface::lsm::ValidatorInfo;

use crate::{
    contract::{execute, CONTRACT_NAME},
    migrate::{migrate, MigrateMsg},
    msg::ExecuteMsg,
    state::{Config, CONFIG, VALIDATORS_INFO, VALIDATORS_PER_ROUND, VALIDATORS_STORE_INITIALIZED},
    testing::{hydro_current_round_mock, staking_validator_grpc_mock, VALIDATOR_1},
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock},
    utils::load_validators_infos,
};

const OLD_ROUND: u64 = 496;
const CURRENT_ROUND: u64 = 516;
const OLD_MAX_VALIDATORS: u64 = 500;
const NEW_MAX_VALIDATORS: u64 = 20;

#[test]
fn test_migrate_shrinks_top_n_and_propagates_forward() {
    let mut deps = mock_dependencies(no_op_grpc_query_mock());
    let env = mock_env();

    set_contract_version(deps.as_mut().storage, CONTRACT_NAME, "0.0.0").unwrap();

    CONFIG
        .save(
            deps.as_mut().storage,
            &Config {
                hydro_contract_address: Addr::unchecked("hydro_contract"),
                max_validator_shares_participating: OLD_MAX_VALIDATORS,
            },
        )
        .unwrap();

    VALIDATORS_STORE_INITIALIZED
        .save(deps.as_mut().storage, OLD_ROUND, &true)
        .unwrap();

    // VALIDATOR_1 gets the highest amount of delegated tokens, guaranteeing it survives the
    // shrink to NEW_MAX_VALIDATORS, so it can be used in the UpdateValidatorsRatios call below.
    // The remaining 499 validators get distinct, much smaller token amounts.
    let mut all_validators = vec![ValidatorInfo::new(
        VALIDATOR_1.to_string(),
        Uint128::new(1_000_000),
        Decimal::one(),
    )];
    for i in 0..(OLD_MAX_VALIDATORS - 1) {
        all_validators.push(ValidatorInfo::new(
            format!("validator-{i}"),
            Uint128::new((i + 1) as u128),
            Decimal::one(),
        ));
    }

    for validator in &all_validators {
        VALIDATORS_INFO
            .save(
                deps.as_mut().storage,
                (OLD_ROUND, validator.address.clone()),
                validator,
            )
            .unwrap();
        VALIDATORS_PER_ROUND
            .save(
                deps.as_mut().storage,
                (
                    OLD_ROUND,
                    validator.delegated_tokens.u128(),
                    validator.address.clone(),
                ),
                &validator.address,
            )
            .unwrap();
    }

    migrate(
        deps.as_mut(),
        env.clone(),
        MigrateMsg {
            max_validator_shares_participating: NEW_MAX_VALIDATORS,
        },
    )
    .unwrap();

    let config = CONFIG.load(deps.as_ref().storage).unwrap();
    assert_eq!(
        config.max_validator_shares_participating,
        NEW_MAX_VALIDATORS
    );

    let mut expected_survivors = all_validators.clone();
    expected_survivors.sort_by_key(|v| std::cmp::Reverse(v.delegated_tokens));
    expected_survivors.truncate(NEW_MAX_VALIDATORS as usize);
    let mut expected_addresses: Vec<String> = expected_survivors
        .iter()
        .map(|v| v.address.clone())
        .collect();
    expected_addresses.sort();

    let remaining_infos = load_validators_infos(deps.as_ref().storage, OLD_ROUND);
    assert_eq!(remaining_infos.len(), NEW_MAX_VALIDATORS as usize);
    let mut remaining_addresses: Vec<String> =
        remaining_infos.iter().map(|v| v.address.clone()).collect();
    remaining_addresses.sort();
    assert_eq!(remaining_addresses, expected_addresses);

    let remaining_per_round_count = VALIDATORS_PER_ROUND
        .sub_prefix(OLD_ROUND)
        .range(deps.as_ref().storage, None, None, Order::Ascending)
        .count();
    assert_eq!(remaining_per_round_count, NEW_MAX_VALIDATORS as usize);

    // Now drive a transaction on the current round so the lazily-initialized rounds between
    // OLD_ROUND and CURRENT_ROUND get seeded from the (now shrunk) OLD_ROUND state.
    deps.querier
        .update_wasm(hydro_current_round_mock(CURRENT_ROUND));
    deps.querier.update_grpc(staking_validator_grpc_mock(
        [(
            VALIDATOR_1.to_string(),
            (Uint128::new(2_000_000), Uint128::new(2_000_000)),
        )]
        .into_iter()
        .collect(),
    ));

    let info = MessageInfo {
        sender: Addr::unchecked("sender"),
        funds: vec![],
    };

    execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::UpdateValidatorsRatios {
            validators: vec![VALIDATOR_1.to_string()],
        },
    )
    .unwrap();

    for round_id in (OLD_ROUND + 1)..=CURRENT_ROUND {
        let infos = load_validators_infos(deps.as_ref().storage, round_id);
        assert_eq!(
            infos.len(),
            NEW_MAX_VALIDATORS as usize,
            "round {round_id} does not have exactly {NEW_MAX_VALIDATORS} validators"
        );
    }
}
