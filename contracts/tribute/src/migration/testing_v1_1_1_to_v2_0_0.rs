use cosmwasm_std::{
    testing::{mock_dependencies, mock_env},
    Coin, Timestamp, Uint128,
};
use cw2::{get_contract_version, set_contract_version};
use cw_storage_plus::{Item, Map};
use hydro::state::Constants;

use crate::{
    contract::{instantiate, CONTRACT_NAME},
    migration::{
        migrate::{migrate, CONTRACT_VERSION_V1_1_1, CONTRACT_VERSION_V2_0_0},
        v1_1_1::{ConfigV1_1_1, TributeV1_1_1},
        v2_0_0::{ConfigV2_0_0, MigrateMsgV2_0_0, TributeV2_0_0},
    },
    testing::{get_instantiate_msg, get_message_info, MockWasmQuerier},
};

#[test]
fn test_migrate() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let round_id = 0;
    let tranche_id = 1;

    let user_addr = "addr0000";
    let hydro_addr = "addr0001";

    let hydro_contract_address = deps.api.addr_make(hydro_addr);
    let user_address = deps.api.addr_make(user_addr);
    let info = get_message_info(&deps.api, user_addr, &[]);

    let first_round_start = 1730851140000000000;
    let lock_epoch_length = 2628000000000000;

    let constants_mock = Constants {
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
        max_bid_duration: 12,
    };

    let mock_querier = MockWasmQuerier::new(
        hydro_contract_address.to_string(),
        round_id,
        vec![],
        vec![],
        vec![],
        Some(constants_mock.clone()),
    );
    deps.querier.update_wasm(move |q| mock_querier.handler(q));

    let msg = get_instantiate_msg(hydro_contract_address.to_string());
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // Override contract version so that we can run the migration
    let res = set_contract_version(&mut deps.storage, CONTRACT_NAME, CONTRACT_VERSION_V1_1_1);
    assert!(
        res.is_ok(),
        "failed to set contract version before running the migration"
    );

    const OLD_CONFIG: Item<ConfigV1_1_1> = Item::new("config");
    const NEW_CONFIG: Item<ConfigV2_0_0> = Item::new("config");

    const OLD_ID_TO_TRIBUTE_MAP: Map<u64, TributeV1_1_1> = Map::new("id_to_tribute_map");
    const NEW_ID_TO_TRIBUTE_MAP: Map<u64, TributeV2_0_0> = Map::new("id_to_tribute_map");

    // Override the config so it has the old data structure stored before running the migration
    let old_config = ConfigV1_1_1 {
        hydro_contract: hydro_contract_address.clone(),
        top_n_props_count: 11,
        min_prop_percent_for_claimable_tributes: Uint128::new(7),
    };

    let res = OLD_CONFIG.save(&mut deps.storage, &old_config);
    assert!(
        res.is_ok(),
        "failed to save old config before running the migration"
    );

    let old_tributes = vec![
        TributeV1_1_1 {
            round_id,
            tranche_id,
            proposal_id: 0,
            tribute_id: 0,
            depositor: user_address.clone(),
            funds: Coin::new(100u128, "token1"),
            refunded: false,
        },
        TributeV1_1_1 {
            round_id,
            tranche_id,
            proposal_id: 1,
            tribute_id: 1,
            depositor: user_address.clone(),
            funds: Coin::new(200u128, "token2"),
            refunded: false,
        },
        TributeV1_1_1 {
            round_id: round_id + 1,
            tranche_id,
            proposal_id: 2,
            tribute_id: 2,
            depositor: user_address.clone(),
            funds: Coin::new(300u128, "token3"),
            refunded: true,
        },
    ];

    for old_tribute in &old_tributes {
        let res =
            OLD_ID_TO_TRIBUTE_MAP.save(&mut deps.storage, old_tribute.tribute_id, old_tribute);
        assert!(
            res.is_ok(),
            "failed to save old proposals before running the migration"
        )
    }

    // Run the migration
    let res = migrate(deps.as_mut(), env.clone(), MigrateMsgV2_0_0 {});
    assert!(res.is_ok(), "migration failed!");

    // Verify that the Config got migrated properly
    let res = NEW_CONFIG.load(&deps.storage);
    assert!(
        res.is_ok(),
        "failed to load new config after running the migration!"
    );
    assert_eq!(res.unwrap().hydro_contract, old_config.hydro_contract);

    // Verify that the tributes got migrated properly
    for old_tribute in &old_tributes {
        let res = NEW_ID_TO_TRIBUTE_MAP.load(&deps.storage, old_tribute.tribute_id);
        assert!(
            res.is_ok(),
            "failed to load new tribute after running the migration!"
        );

        let new_tribute = res.unwrap();
        assert_eq!(new_tribute.round_id, old_tribute.round_id);
        assert_eq!(new_tribute.tranche_id, old_tribute.tranche_id);
        assert_eq!(new_tribute.proposal_id, old_tribute.proposal_id);
        assert_eq!(new_tribute.tribute_id, old_tribute.tribute_id);
        assert_eq!(new_tribute.depositor, old_tribute.depositor);
        assert_eq!(new_tribute.funds, old_tribute.funds);
        assert_eq!(new_tribute.refunded, old_tribute.refunded);

        assert_eq!(new_tribute.creation_round, old_tribute.round_id);

        let expected_creation_time = constants_mock
            .first_round_start
            .plus_nanos(new_tribute.round_id * constants_mock.lock_epoch_length);
        assert_eq!(new_tribute.creation_time, expected_creation_time);
    }

    // Verify the contract version after running the migration
    let res = get_contract_version(&deps.storage);
    assert_eq!(res.unwrap().version, CONTRACT_VERSION_V2_0_0.to_string());
}
