use cosmwasm_std::{testing::mock_env, Uint128};
use cw2::set_contract_version;
use cw_storage_plus::Item;

use crate::{
    contract::CONTRACT_NAME,
    migration::{
        migrate::{migrate, MigrateMsg},
        v_3_6_1::ConfigV3_6_1,
    },
    state::{Config, CONFIG},
    testing::{get_message_info, mock_dependencies},
};

const ATOM_ON_NEUTRON: &str =
    "ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9";

#[test]
fn migrate_config_test() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let inflow_address = deps.api.addr_make("inflow");

    // Old config data
    let deposit_denom = String::from(ATOM_ON_NEUTRON);
    let max_withdrawals_per_user = 10;
    let deposit_cap = Uint128::new(150000);
    let vault_shares_denom = format!("factory/{}/inflow_uatom_share", inflow_address).to_string();

    // New config info
    let control_center_addr = deps.api.addr_make("control_center");
    let token_info_provider_addr = None;

    let old_config = ConfigV3_6_1 {
        deposit_denom: deposit_denom.clone(),
        vault_shares_denom: vault_shares_denom.clone(),
        max_withdrawals_per_user,
        deposit_cap,
    };

    let expected_new_config = Config {
        deposit_denom: old_config.deposit_denom.clone(),
        vault_shares_denom: old_config.vault_shares_denom.clone(),
        control_center_contract: control_center_addr.clone(),
        token_info_provider_contract: token_info_provider_addr.clone(),
        max_withdrawals_per_user,
    };

    // Save old version of config into store
    const OLD_CONFIG: Item<ConfigV3_6_1> = Item::new("config");
    OLD_CONFIG.save(&mut deps.storage, &old_config).unwrap();

    // Set old contract version to be able to perform the migration
    set_contract_version(&mut deps.storage, CONTRACT_NAME, "3.6.1").unwrap();

    let info = get_message_info(&deps.api, "admin", &[]);

    migrate(
        deps.as_mut(),
        env,
        info,
        MigrateMsg {
            control_center_addr: control_center_addr.to_string(),
            token_info_provider_addr: token_info_provider_addr.map(|addr| addr.to_string()),
        },
    )
    .unwrap();

    let new_config = CONFIG.load(&deps.storage).unwrap();
    assert_eq!(new_config, expected_new_config);
}
