use cosmwasm_std::testing::mock_dependencies;
use cosmwasm_std::{Addr, Decimal, Storage};

use crate::migration::migrate::{migrate_fee_config, FeeConfigV1, FEE_CONFIG_V1};
use crate::state::FEE_CONFIG;

fn save_v1(storage: &mut dyn Storage, fee_rate: Decimal, fee_recipient: &str) {
    FEE_CONFIG_V1
        .save(
            storage,
            &FeeConfigV1 {
                fee_rate,
                fee_recipient: Addr::unchecked(fee_recipient),
            },
        )
        .unwrap();
}

#[test]
fn test_migrate_fee_config_empty_recipient_becomes_none() {
    let mut deps = mock_dependencies();
    save_v1(deps.as_mut().storage, Decimal::zero(), "");

    migrate_fee_config(deps.as_mut().storage).unwrap();

    let config = FEE_CONFIG.load(deps.as_ref().storage).unwrap();
    assert_eq!(config.fee_rate, Decimal::zero());
    assert_eq!(config.fee_recipient, None);
}

#[test]
fn test_migrate_fee_config_existing_recipient_becomes_some() {
    let mut deps = mock_dependencies();
    let addr = "neutron1treasury000000000000000000000000000000";
    save_v1(deps.as_mut().storage, Decimal::percent(20), addr);

    migrate_fee_config(deps.as_mut().storage).unwrap();

    let config = FEE_CONFIG.load(deps.as_ref().storage).unwrap();
    assert_eq!(config.fee_rate, Decimal::percent(20));
    assert_eq!(config.fee_recipient, Some(Addr::unchecked(addr)));
}
