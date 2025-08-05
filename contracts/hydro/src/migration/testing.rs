#[cfg(test)]
mod tests {
    use cosmwasm_std::{testing::mock_env, Addr, Coin, Timestamp, Uint128};
    use cw2::set_contract_version;
    use std::collections::HashMap;

    use crate::{
        contract::{CONTRACT_NAME, CONTRACT_VERSION},
        migration::{
            migrate::{migrate, MigrateMsgV3_5_3, CONTRACT_VERSION_V3_5_2},
            v3_5_2::TOKEN_IDS_MIGRATION_PROGRESS,
        },
        state::{LockEntryV2, CONSTANTS, LOCKS_MAP_V2, TOKEN_IDS, TOKEN_INFO_PROVIDERS},
        testing::{get_default_lsm_token_info_provider, IBC_DENOM_1, VALIDATOR_1_LST_DENOM_1},
        testing_lsm_integration::get_default_constants,
        testing_mocks::{denom_trace_grpc_query_mock, mock_dependencies},
        token_manager::LSM_TOKEN_INFO_PROVIDER_ID,
    };

    #[test]
    fn test_migration_with_lsm_and_non_lsm_lockups() {
        // Set up LSM mocking to properly identify LSM denoms
        let grpc_query = denom_trace_grpc_query_mock(
            "transfer/channel-0".to_string(),
            HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
        );
        let mut deps = mock_dependencies(grpc_query);
        let env = mock_env();

        // Set up the contract version to v3.5.2 (the version we're migrating from)
        set_contract_version(
            deps.as_mut().storage,
            CONTRACT_NAME,
            CONTRACT_VERSION_V3_5_2,
        )
        .unwrap();

        // Set up constants (required for pause/unpause functionality)
        let constants = get_default_constants();
        CONSTANTS
            .save(deps.as_mut().storage, env.block.time.nanos(), &constants)
            .unwrap();

        // Set up LSM token info provider to identify LSM denoms
        let lsm_provider = get_default_lsm_token_info_provider();
        TOKEN_INFO_PROVIDERS
            .save(
                deps.as_mut().storage,
                LSM_TOKEN_INFO_PROVIDER_ID.to_string(),
                &lsm_provider,
            )
            .unwrap();

        // Create two lockups:
        // 1. Non-LSM lockup (should become a NFT/token)
        let non_lsm_lockup = LockEntryV2 {
            lock_id: 0,
            owner: Addr::unchecked("user1"),
            funds: Coin {
                denom: "uatom".to_string(), // Regular token
                amount: Uint128::new(1000),
            },
            lock_start: Timestamp::from_nanos(1000),
            lock_end: Timestamp::from_nanos(2000),
        };

        // 2. LSM lockup (should NOT become a NFT/token)
        // Using the mocked IBC denom that will be identified as LSM
        let lsm_lockup = LockEntryV2 {
            lock_id: 1,
            owner: Addr::unchecked("user2"),
            funds: Coin {
                denom: IBC_DENOM_1.to_string(), // IBC/LSM token that maps to VALIDATOR_1_LST_DENOM_1
                amount: Uint128::new(2000),
            },
            lock_start: Timestamp::from_nanos(1000),
            lock_end: Timestamp::from_nanos(2000),
        };

        // Save lockups to storage
        LOCKS_MAP_V2
            .save(deps.as_mut().storage, 0, &non_lsm_lockup, env.block.height)
            .unwrap();
        LOCKS_MAP_V2
            .save(deps.as_mut().storage, 1, &lsm_lockup, env.block.height)
            .unwrap();

        // Verify initial state: no tokens in TOKEN_IDS
        assert_eq!(
            TOKEN_IDS
                .range(
                    deps.as_ref().storage,
                    None,
                    None,
                    cosmwasm_std::Order::Ascending
                )
                .count(),
            0
        );
        assert!(TOKEN_IDS_MIGRATION_PROGRESS
            .may_load(deps.as_ref().storage)
            .unwrap()
            .is_none());

        // First migration call with limit=1 (should process only the first lockup)
        let msg = MigrateMsgV3_5_3::PopulateTokenIds { limit: Some(1) };
        let result = migrate(deps.as_mut(), env.clone(), msg).unwrap();

        // Check that migration is incomplete (contract should still be paused)
        let migration_status = result
            .attributes
            .iter()
            .find(|attr| attr.key == "migration_status")
            .unwrap();
        assert_eq!(migration_status.value, "incomplete");

        // Check that one lockup was processed
        let processed_count = result
            .attributes
            .iter()
            .find(|attr| attr.key == "processed_count")
            .unwrap();
        assert_eq!(processed_count.value, "1");

        // Check that one token was added (the non-LSM lockup)
        let added_count = result
            .attributes
            .iter()
            .find(|attr| attr.key == "added_count")
            .unwrap();
        assert_eq!(added_count.value, "1");

        // Verify TOKEN_IDS contains the non-LSM lockup
        assert!(TOKEN_IDS.has(deps.as_ref().storage, 0)); // Non-LSM lockup should be added
        assert!(!TOKEN_IDS.has(deps.as_ref().storage, 1)); // LSM lockup should not be added yet

        // Check migration progress is saved
        let progress = TOKEN_IDS_MIGRATION_PROGRESS
            .load(deps.as_ref().storage)
            .unwrap();
        assert_eq!(progress, 0); // Last processed lock_id

        // Verify contract is still paused
        let (_, constants_after_first) =
            crate::utils::load_constants_active_at_timestamp(&deps.as_ref(), env.block.time)
                .unwrap();
        assert!(constants_after_first.paused);

        // Second migration call with limit=1 (should process the second lockup)
        let msg = MigrateMsgV3_5_3::PopulateTokenIds { limit: Some(1) };
        let result = migrate(deps.as_mut(), env.clone(), msg).unwrap();

        // Check that migration is now complete (contract should be unpaused)
        let migration_status = result
            .attributes
            .iter()
            .find(|attr| attr.key == "migration_status")
            .unwrap();
        assert_eq!(migration_status.value, "complete");

        // Check that one more lockup was processed
        let processed_count = result
            .attributes
            .iter()
            .find(|attr| attr.key == "processed_count")
            .unwrap();
        assert_eq!(processed_count.value, "1");

        // Check that no tokens were added (LSM lockup should be skipped)
        let added_count = result
            .attributes
            .iter()
            .find(|attr| attr.key == "added_count")
            .unwrap();
        assert_eq!(added_count.value, "0");

        // Verify final TOKEN_IDS state
        assert!(TOKEN_IDS.has(deps.as_ref().storage, 0)); // Non-LSM lockup should still be there
        assert!(!TOKEN_IDS.has(deps.as_ref().storage, 1)); // LSM lockup should not be added

        // Check migration progress is cleaned up
        assert!(TOKEN_IDS_MIGRATION_PROGRESS
            .may_load(deps.as_ref().storage)
            .unwrap()
            .is_none());

        // Verify contract is now unpaused
        let (_, constants_final) =
            crate::utils::load_constants_active_at_timestamp(&deps.as_ref(), env.block.time)
                .unwrap();
        assert!(!constants_final.paused);

        // Verify contract version is updated
        let contract_version = cw2::get_contract_version(deps.as_ref().storage).unwrap();
        assert_eq!(contract_version.version, CONTRACT_VERSION);
    }
}
