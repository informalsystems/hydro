// Tests for adapter management functionality
use super::testing::{get_message_info, mock_dependencies};
use crate::{
    contract::{execute, instantiate, query},
    msg::{DenomMetadata, InstantiateMsg},
    state::{ADAPTERS, CONFIG},
    testing_mocks::{
        mock_address_balance, setup_adapter_mock, setup_control_center_mock,
        setup_token_info_provider_mock, update_contract_mock, MockAdapterConfig, MockWasmQuerier,
    },
};
use cosmwasm_std::{
    testing::{mock_env, MockApi, MockQuerier, MockStorage},
    Addr, Coin, CosmosMsg, Decimal, OwnedDeps, Uint128, WasmMsg,
};
use interface::{
    inflow_adapter::deserialize_adapter_interface_msg,
    inflow_vault::{
        AdapterInfoResponse, AdaptersListResponse, AllocationMode, DeploymentTracking, ExecuteMsg,
        QueryMsg,
    },
};
use std::collections::HashMap;

const DEPOSIT_DENOM: &str = "ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9";
const WHITELIST_ADDR: &str = "whitelist1";
const NON_WHITELIST_ADDR: &str = "nonwhitelist";
const ADAPTER_ADDR: &str = "adapter1";
const CONTROL_CENTER: &str = "control_center";
const TOKEN_INFO_PROVIDER: &str = "token_info_provider";
const DEFAULT_DEPOSIT_CAP: Uint128 = Uint128::new(10_000_000);

/// Minimum initial deposit required for instantiation (matches contract constant)
const MINIMUM_INITIAL_DEPOSIT: u128 = 1_000_000;

/// Helper to get the initial deposit funds required for instantiation
fn get_initial_deposit_funds(denom: &str) -> Vec<cosmwasm_std::Coin> {
    vec![cosmwasm_std::Coin::new(MINIMUM_INITIAL_DEPOSIT, denom)]
}

fn get_default_instantiate_msg(
    api: &cosmwasm_std::testing::MockApi,
    deposit_denom: &str,
    whitelist_addr: Addr,
    control_center_contract: Addr,
    _token_info_provider_contract: Addr,
) -> InstantiateMsg {
    InstantiateMsg {
        deposit_denom: deposit_denom.to_string(),
        whitelist: vec![whitelist_addr.to_string()],
        subdenom: "hydro_inflow_uatom".to_string(),
        token_metadata: DenomMetadata {
            display: "hydro_inflow_atom".to_string(),
            exponent: 6,
            name: "Hydro Inflow ATOM".to_string(),
            description: "Hydro Inflow ATOM".to_string(),
            symbol: "hydro_inflow_atom".to_string(),
            uri: None,
            uri_hash: None,
        },
        control_center_contract: control_center_contract.to_string(),
        // Set to None to avoid needing token info provider mocks during instantiate
        token_info_provider_contract: None,
        max_withdrawals_per_user: 10,
        initial_shares_recipient: api.addr_make("initial_shares_recipient").to_string(),
    }
}

#[test]
fn register_adapter_success() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter from whitelisted address
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: Some("Mars lending protocol adapter".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Verify response attributes
    assert_eq!(res.attributes.len(), 7);
    assert_eq!(res.attributes[0].value, "register_adapter");
    assert_eq!(res.attributes[2].value, "mars_adapter");
    assert_eq!(res.attributes[3].value, adapter_addr.as_str());
    assert_eq!(res.attributes[4].value, "Automated");
    assert_eq!(res.attributes[5].value, "NotTracked");

    // Verify adapter was saved correctly
    let adapter_info = ADAPTERS
        .load(&deps.storage, "mars_adapter".to_string())
        .unwrap();
    assert_eq!(adapter_info.address, adapter_addr);
    assert!(matches!(
        adapter_info.allocation_mode,
        AllocationMode::Automated
    ));
    assert_eq!(adapter_info.name, "mars_adapter");
    assert_eq!(
        adapter_info.description,
        Some("Mars lending protocol adapter".to_string())
    );
}

#[test]
fn register_adapter_unauthorized() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Try to register adapter from non-whitelisted address
    let info = get_message_info(&deps.api, NON_WHITELIST_ADDR, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

#[test]
fn register_adapter_duplicate_name() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Try to register adapter with same name but different address
    let another_adapter_addr = deps.api.addr_make("another_adapter");
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: another_adapter_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("Adapter already exists: mars_adapter"));
}

#[test]
fn unregister_adapter_success() {
    let deps = mock_dependencies();

    let vault_contract_addr = deps.api.addr_make("inflow");
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Set up adapter mock with zero position
    let mut adapter_configs = HashMap::new();
    adapter_configs.insert(
        adapter_addr.clone(),
        MockAdapterConfig::new(0, 0, 0), // current_deposit = 0
    );

    let control_center_config =
        ControlCenterMockConfig::new(control_center_contract_addr.clone(), 0, 0);
    let token_info_provider_config = TokenInfoProviderMockConfig::new(
        token_info_provider_contract_addr.clone(),
        DEPOSIT_DENOM.to_string(),
    );
    let (mut deps, _wasm_querier) = mock_dependencies_with_adapters(
        adapter_configs,
        control_center_config,
        token_info_provider_config,
    );
    let mut env = mock_env();
    env.contract.address = vault_contract_addr;

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Verify adapter exists
    assert!(ADAPTERS.has(&deps.storage, "mars_adapter".to_string()));

    // Unregister adapter (should succeed since position is zero)
    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::UnregisterAdapter {
            name: "mars_adapter".to_string(),
        },
    )
    .unwrap();

    // Verify response attributes
    assert_eq!(res.attributes[0].value, "unregister_adapter");
    assert_eq!(res.attributes[2].value, "mars_adapter");
    assert_eq!(res.attributes[3].value, adapter_addr.as_str());

    // Verify adapter was removed
    assert!(!ADAPTERS.has(&deps.storage, "mars_adapter".to_string()));
}

#[test]
fn unregister_adapter_fails_with_non_zero_position() {
    let deps = mock_dependencies();

    let vault_contract_addr = deps.api.addr_make("inflow");
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Set up adapter mock with non-zero position
    let mut adapter_configs = HashMap::new();
    adapter_configs.insert(
        adapter_addr.clone(),
        MockAdapterConfig::new(0, 0, 1000), // current_deposit = 1000 (non-zero)
    );

    let control_center_config =
        ControlCenterMockConfig::new(control_center_contract_addr.clone(), 0, 0);
    let token_info_provider_config = TokenInfoProviderMockConfig::new(
        token_info_provider_contract_addr.clone(),
        DEPOSIT_DENOM.to_string(),
    );
    let (mut deps, _wasm_querier) = mock_dependencies_with_adapters(
        adapter_configs,
        control_center_config,
        token_info_provider_config,
    );
    let mut env = mock_env();
    env.contract.address = vault_contract_addr;

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Verify adapter exists
    assert!(ADAPTERS.has(&deps.storage, "mars_adapter".to_string()));

    // Try to unregister adapter (should fail since position is non-zero)
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::UnregisterAdapter {
            name: "mars_adapter".to_string(),
        },
    )
    .unwrap_err();

    // Verify error message
    assert!(
        err.to_string().contains("non-zero position"),
        "Expected error about non-zero position, got: {}",
        err
    );

    // Verify adapter was NOT removed
    assert!(ADAPTERS.has(&deps.storage, "mars_adapter".to_string()));
}

#[test]
fn unregister_adapter_not_found() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Try to unregister non-existent adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::UnregisterAdapter {
            name: "nonexistent_adapter".to_string(),
        },
    )
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("Adapter not found: nonexistent_adapter"));
}

#[test]
fn unregister_adapter_unauthorized() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Try to unregister from non-whitelisted address
    let info = get_message_info(&deps.api, NON_WHITELIST_ADDR, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::UnregisterAdapter {
            name: "mars_adapter".to_string(),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

#[test]
fn toggle_adapter_success() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter (auto_allocation = true by default)
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Verify adapter is included in automated allocation
    let adapter_info = ADAPTERS
        .load(&deps.storage, "mars_adapter".to_string())
        .unwrap();
    assert!(matches!(
        adapter_info.allocation_mode,
        AllocationMode::Automated
    ));

    // Set adapter to exclude from automated allocation
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::SetAdapterAllocationMode {
            name: "mars_adapter".to_string(),
            allocation_mode: AllocationMode::Manual,
        },
    )
    .unwrap();

    // Verify response attributes
    assert_eq!(res.attributes[0].value, "set_adapter_allocation_mode");
    assert_eq!(res.attributes[2].value, "mars_adapter");
    assert_eq!(res.attributes[3].value, "Manual");

    // Verify adapter is now excluded from automated allocation
    let adapter_info = ADAPTERS
        .load(&deps.storage, "mars_adapter".to_string())
        .unwrap();
    assert!(matches!(
        adapter_info.allocation_mode,
        AllocationMode::Manual
    ));

    // Set adapter back to include in automated allocation
    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::SetAdapterAllocationMode {
            name: "mars_adapter".to_string(),
            allocation_mode: AllocationMode::Automated,
        },
    )
    .unwrap();

    // Verify adapter is included in automated allocation again
    assert_eq!(res.attributes[3].value, "Automated");
    let adapter_info = ADAPTERS
        .load(&deps.storage, "mars_adapter".to_string())
        .unwrap();
    assert!(matches!(
        adapter_info.allocation_mode,
        AllocationMode::Automated
    ));
}

#[test]
fn set_deployment_tracking_syncs_deployed_amount_on_toggle() {
    let deps = mock_dependencies();

    let vault_contract_addr = deps.api.addr_make("inflow");
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Set up adapter mock with non-zero position (1000 tokens)
    let mut adapter_configs = HashMap::new();
    adapter_configs.insert(
        adapter_addr.clone(),
        MockAdapterConfig::new(10000, 10000, 1000), // current_deposit = 1000
    );

    let control_center_config =
        ControlCenterMockConfig::new(control_center_contract_addr.clone(), 0, 0);
    let token_info_provider_config = TokenInfoProviderMockConfig::new(
        token_info_provider_contract_addr.clone(),
        DEPOSIT_DENOM.to_string(),
    );
    let (mut deps, _wasm_querier) = mock_dependencies_with_adapters(
        adapter_configs,
        control_center_config,
        token_info_provider_config,
    );
    let mut env = mock_env();
    env.contract.address = vault_contract_addr;

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter with Tracked deployment
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::Tracked,
        },
    )
    .unwrap();

    // Toggle from Tracked -> NotTracked: Should send Subtract message
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::SetAdapterDeploymentTracking {
            name: "mars_adapter".to_string(),
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Verify response has UpdateDeployedAmount message
    assert_eq!(res.messages.len(), 1);
    match &res.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg: _,
            funds: _,
        }) => {
            assert_eq!(contract_addr, &control_center_contract_addr.to_string());
        }
        _ => panic!("Expected WasmMsg::Execute for UpdateDeployedAmount"),
    }

    // Verify synced_amount attribute
    let synced_amount_attr = res
        .attributes
        .iter()
        .find(|a| a.key == "synced_amount")
        .unwrap();
    assert_eq!(synced_amount_attr.value, "1000");

    // Toggle from NotTracked -> Tracked: Should send Add message
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::SetAdapterDeploymentTracking {
            name: "mars_adapter".to_string(),
            deployment_tracking: DeploymentTracking::Tracked,
        },
    )
    .unwrap();

    // Verify response has UpdateDeployedAmount message
    assert_eq!(res.messages.len(), 1);

    // Verify deployment tracking was updated
    let adapter_info = ADAPTERS
        .load(&deps.storage, "mars_adapter".to_string())
        .unwrap();
    assert!(matches!(
        adapter_info.deployment_tracking,
        DeploymentTracking::Tracked
    ));
}

#[test]
fn set_deployment_tracking_same_mode_is_noop() {
    let deps = mock_dependencies();

    let vault_contract_addr = deps.api.addr_make("inflow");
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Set up adapter mock with non-zero position
    let mut adapter_configs = HashMap::new();
    adapter_configs.insert(
        adapter_addr.clone(),
        MockAdapterConfig::new(10000, 10000, 1000),
    );

    let control_center_config =
        ControlCenterMockConfig::new(control_center_contract_addr.clone(), 0, 0);
    let token_info_provider_config = TokenInfoProviderMockConfig::new(
        token_info_provider_contract_addr.clone(),
        DEPOSIT_DENOM.to_string(),
    );
    let (mut deps, _wasm_querier) = mock_dependencies_with_adapters(
        adapter_configs,
        control_center_config,
        token_info_provider_config,
    );
    let mut env = mock_env();
    env.contract.address = vault_contract_addr;

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter with Tracked deployment
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::Tracked,
        },
    )
    .unwrap();

    // Toggle to the same mode (Tracked -> Tracked): Should be no-op
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::SetAdapterDeploymentTracking {
            name: "mars_adapter".to_string(),
            deployment_tracking: DeploymentTracking::Tracked,
        },
    )
    .unwrap();

    // Verify no messages sent (no-op)
    assert_eq!(res.messages.len(), 0);

    // Verify "no_change" result attribute
    let result_attr = res.attributes.iter().find(|a| a.key == "result").unwrap();
    assert_eq!(result_attr.value, "no_change");
}

#[test]
fn set_deployment_tracking_zero_position_no_message() {
    let deps = mock_dependencies();

    let vault_contract_addr = deps.api.addr_make("inflow");
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Set up adapter mock with ZERO position
    let mut adapter_configs = HashMap::new();
    adapter_configs.insert(
        adapter_addr.clone(),
        MockAdapterConfig::new(10000, 10000, 0), // current_deposit = 0
    );

    let control_center_config =
        ControlCenterMockConfig::new(control_center_contract_addr.clone(), 0, 0);
    let token_info_provider_config = TokenInfoProviderMockConfig::new(
        token_info_provider_contract_addr.clone(),
        DEPOSIT_DENOM.to_string(),
    );
    let (mut deps, _wasm_querier) = mock_dependencies_with_adapters(
        adapter_configs,
        control_center_config,
        token_info_provider_config,
    );
    let mut env = mock_env();
    env.contract.address = vault_contract_addr;

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter with Tracked deployment
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::Tracked,
        },
    )
    .unwrap();

    // Toggle from Tracked -> NotTracked with zero position: Should not send message
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::SetAdapterDeploymentTracking {
            name: "mars_adapter".to_string(),
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Verify no messages sent (position is zero)
    assert_eq!(res.messages.len(), 0);

    // Verify synced_amount is 0
    let synced_amount_attr = res
        .attributes
        .iter()
        .find(|a| a.key == "synced_amount")
        .unwrap();
    assert_eq!(synced_amount_attr.value, "0");

    // Verify deployment tracking was still updated
    let adapter_info = ADAPTERS
        .load(&deps.storage, "mars_adapter".to_string())
        .unwrap();
    assert!(matches!(
        adapter_info.deployment_tracking,
        DeploymentTracking::NotTracked
    ));
}

#[test]
fn set_adapter_allocation_mode_not_found() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Try to set allocation mode for non-existent adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::SetAdapterAllocationMode {
            name: "nonexistent_adapter".to_string(),
            allocation_mode: AllocationMode::Manual,
        },
    )
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("Adapter not found: nonexistent_adapter"));
}

#[test]
fn set_adapter_allocation_mode_unauthorized() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Try to set allocation mode from non-whitelisted address
    let info = get_message_info(&deps.api, NON_WHITELIST_ADDR, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::SetAdapterAllocationMode {
            name: "mars_adapter".to_string(),
            allocation_mode: AllocationMode::Manual,
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

#[test]
fn register_multiple_adapters() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter1_addr = deps.api.addr_make("adapter1");
    let adapter2_addr = deps.api.addr_make("adapter2");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register first adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter1_addr.to_string(),
            description: Some("Mars Protocol".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Register second adapter
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "osmosis_adapter".to_string(),
            address: adapter2_addr.to_string(),
            description: Some("Osmosis DEX".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Verify both adapters exist
    let adapter1 = ADAPTERS
        .load(&deps.storage, "mars_adapter".to_string())
        .unwrap();
    let adapter2 = ADAPTERS
        .load(&deps.storage, "osmosis_adapter".to_string())
        .unwrap();

    assert_eq!(adapter1.address, adapter1_addr);
    assert_eq!(adapter1.name, "mars_adapter");
    assert_eq!(adapter2.address, adapter2_addr);
    assert_eq!(adapter2.name, "osmosis_adapter");
}

// ============================================================================
// Adapter Query Tests
// ============================================================================

#[test]
fn query_list_adapters_empty() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Query adapters (should be empty)
    let res = query(deps.as_ref(), env, QueryMsg::ListAdapters {}).unwrap();
    let response: AdaptersListResponse = cosmwasm_std::from_json(&res).unwrap();

    assert_eq!(response.adapters.len(), 0);
}

#[test]
fn query_list_adapters_with_adapters() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter1_addr = deps.api.addr_make("adapter1");
    let adapter2_addr = deps.api.addr_make("adapter2");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register two adapters
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter1_addr.to_string(),
            description: Some("Mars Protocol".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "osmosis_adapter".to_string(),
            address: adapter2_addr.to_string(),
            description: Some("Osmosis DEX".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Query adapters
    let res = query(deps.as_ref(), env, QueryMsg::ListAdapters {}).unwrap();
    let response: AdaptersListResponse = cosmwasm_std::from_json(&res).unwrap();

    assert_eq!(response.adapters.len(), 2);

    // Verify first adapter
    let (name1, info1) = &response.adapters[0];
    assert_eq!(name1, "mars_adapter");
    assert_eq!(info1.address, adapter1_addr);
    assert!(matches!(info1.allocation_mode, AllocationMode::Automated));
    assert_eq!(info1.name, "mars_adapter");
    assert_eq!(info1.description, Some("Mars Protocol".to_string()));

    // Verify second adapter
    let (name2, info2) = &response.adapters[1];
    assert_eq!(name2, "osmosis_adapter");
    assert_eq!(info2.address, adapter2_addr);
    assert!(matches!(info2.allocation_mode, AllocationMode::Automated));
}

#[test]
fn query_adapter_info_success() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: Some("Mars Protocol".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Query adapter info
    let res = query(
        deps.as_ref(),
        env,
        QueryMsg::AdapterInfo {
            name: "mars_adapter".to_string(),
        },
    )
    .unwrap();
    let response: AdapterInfoResponse = cosmwasm_std::from_json(&res).unwrap();

    assert_eq!(response.info.address, adapter_addr);
    assert!(matches!(
        response.info.allocation_mode,
        AllocationMode::Automated
    ));
    assert_eq!(response.info.name, "mars_adapter");
    assert_eq!(response.info.description, Some("Mars Protocol".to_string()));
}

#[test]
fn query_adapter_info_not_found() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Query non-existent adapter
    let err = query(
        deps.as_ref(),
        env,
        QueryMsg::AdapterInfo {
            name: "nonexistent_adapter".to_string(),
        },
    )
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("Adapter not found: nonexistent_adapter"));
}

// ============================================================================
// Adapter Integration Tests - Mock Adapter Infrastructure
// ============================================================================

const USER1: &str = "user1";

/// Configuration for mocking the control center contract
#[derive(Clone, Debug)]
struct ControlCenterMockConfig {
    address: Addr,
    total_pool_value: Uint128,
    total_shares_issued: Uint128,
    deposit_cap: Uint128,
}

impl ControlCenterMockConfig {
    fn new(address: Addr, total_pool_value: u128, total_shares_issued: u128) -> Self {
        Self {
            address,
            total_pool_value: Uint128::new(total_pool_value),
            total_shares_issued: Uint128::new(total_shares_issued),
            deposit_cap: DEFAULT_DEPOSIT_CAP,
        }
    }
}

/// Configuration for mocking the token info provider contract
#[derive(Clone, Debug)]
struct TokenInfoProviderMockConfig {
    address: Addr,
    token_denom: String,
    token_ratio: Decimal,
}

impl TokenInfoProviderMockConfig {
    fn new(address: Addr, token_denom: String) -> Self {
        Self {
            address,
            token_denom,
            token_ratio: Decimal::one(), // Use 1:1 ratio for adapter tests
        }
    }
}

/// Creates mock dependencies with custom adapter, control center, and token info provider query responses
/// Returns both the deps and the MockWasmQuerier so the mocks can be updated later
fn mock_dependencies_with_adapters(
    adapter_configs: HashMap<Addr, MockAdapterConfig>,
    control_center_config: ControlCenterMockConfig,
    token_info_provider_config: TokenInfoProviderMockConfig,
) -> (
    OwnedDeps<MockStorage, MockApi, MockQuerier, neutron_sdk::bindings::query::NeutronQuery>,
    MockWasmQuerier,
) {
    let mut deps = mock_dependencies();

    // Build the mocks HashMap
    let mut mocks = Vec::new();

    // Add control center mock
    mocks.push(setup_control_center_mock(
        control_center_config.address,
        control_center_config.deposit_cap,
        control_center_config.total_pool_value,
        control_center_config.total_shares_issued,
    ));

    // Add token info provider mock
    mocks.push(setup_token_info_provider_mock(
        token_info_provider_config.address,
        token_info_provider_config.token_denom,
        token_info_provider_config.token_ratio,
    ));

    // Add adapter mocks
    for (addr, config) in adapter_configs {
        mocks.push(setup_adapter_mock(addr, config));
    }

    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter(mocks));
    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    (deps, wasm_querier)
}

/// Helper to set up contract with vault shares denom configured
fn setup_contract_with_vault_denom(
    deps: &mut OwnedDeps<
        MockStorage,
        MockApi,
        MockQuerier,
        neutron_sdk::bindings::query::NeutronQuery,
    >,
    vault_contract_addr: &Addr,
) {
    let vault_shares_denom_str: String =
        format!("factory/{vault_contract_addr}/hydro_inflow_uatom");

    CONFIG
        .update(
            &mut deps.storage,
            |mut config| -> Result<_, crate::error::ContractError> {
                config.vault_shares_denom = vault_shares_denom_str;
                Ok(config)
            },
        )
        .unwrap();
}

// ============================================================================
// Test Group 1: Deposit with Adapter Allocation
// ============================================================================

#[test]
fn test_deposit_with_single_adapter_auto_allocation() {
    let deps = mock_dependencies();

    let vault_contract_addr = deps.api.addr_make("inflow");
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make("adapter1");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    let mut adapter_configs = HashMap::new();
    adapter_configs.insert(adapter_addr.clone(), MockAdapterConfig::new(10000, 0, 0));
    let control_center_config =
        ControlCenterMockConfig::new(control_center_contract_addr.clone(), 0, 0);
    let token_info_provider_config = TokenInfoProviderMockConfig::new(
        token_info_provider_contract_addr.clone(),
        DEPOSIT_DENOM.to_string(),
    );
    let (mut deps, _wasm_querier) = mock_dependencies_with_adapters(
        adapter_configs,
        control_center_config,
        token_info_provider_config,
    );
    let mut env = mock_env();

    env.contract.address = vault_contract_addr.clone();

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    setup_contract_with_vault_denom(&mut deps, &vault_contract_addr);

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: Some("Mars Protocol".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Mock that contract received deposit
    mock_address_balance(
        &mut deps,
        vault_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::new(1000),
    );

    // User deposits 1000 tokens
    let info = get_message_info(
        &deps.api,
        USER1,
        &[Coin {
            denom: DEPOSIT_DENOM.to_string(),
            amount: Uint128::new(1000),
        }],
    );

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Deposit { on_behalf_of: None },
    )
    .unwrap();

    // Should have 2 messages: adapter deposit + mint shares
    assert_eq!(res.messages.len(), 2);

    // First message should be adapter deposit
    match &res.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds,
        }) => {
            assert_eq!(contract_addr, &adapter_addr.to_string());
            assert_eq!(funds.len(), 1);
            assert_eq!(funds[0].amount, Uint128::new(1000));
            assert_eq!(funds[0].denom, DEPOSIT_DENOM);
            // Verify it's a Deposit message (wrapped in interface structure)
            let adapter_msg = deserialize_adapter_interface_msg(msg).unwrap();
            assert!(matches!(
                adapter_msg,
                interface::inflow_adapter::AdapterInterfaceMsg::Deposit { .. }
            ));
        }
        _ => panic!("Expected WasmMsg::Execute for adapter deposit"),
    }

    // Verify attributes
    assert_eq!(res.attributes[0].value, "deposit");
    assert_eq!(res.attributes[3].value, "1000");
}

#[test]
fn test_deposit_with_single_adapter_no_auto_allocation() {
    let deps = mock_dependencies();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make("adapter1");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    let mut adapter_configs = HashMap::new();
    adapter_configs.insert(adapter_addr.clone(), MockAdapterConfig::new(10000, 0, 0));
    let control_center_config =
        ControlCenterMockConfig::new(control_center_contract_addr.clone(), 0, 0);
    let token_info_provider_config = TokenInfoProviderMockConfig::new(
        token_info_provider_contract_addr.clone(),
        DEPOSIT_DENOM.to_string(),
    );
    let (mut deps, _wasm_querier) = mock_dependencies_with_adapters(
        adapter_configs,
        control_center_config,
        token_info_provider_config,
    );
    let env = mock_env();

    let vault_contract_addr = env.contract.address.clone();

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    setup_contract_with_vault_denom(&mut deps, &vault_contract_addr);

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: Some("Mars Protocol".to_string()),
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Mock that contract received deposit
    mock_address_balance(
        &mut deps,
        vault_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::new(1000),
    );

    // User deposits - should succeed but funds stay in contract (no adapter deposit)
    let info = get_message_info(
        &deps.api,
        USER1,
        &[Coin {
            denom: DEPOSIT_DENOM.to_string(),
            amount: Uint128::new(1000),
        }],
    );

    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Deposit { on_behalf_of: None },
    )
    .unwrap();

    // Should only have mint message, no adapter deposit (since adapter is excluded from automated allocation)
    assert_eq!(res.messages.len(), 1);

    // Verify it's the mint message only
    match &res.messages[0].msg {
        CosmosMsg::Custom(neutron_sdk::bindings::msg::NeutronMsg::MintTokens {
            denom,
            amount,
            mint_to_address: _,
        }) => {
            // Verify it's a mint tokens message
            assert!(denom.contains("hydro_inflow_uatom"));
            assert_eq!(*amount, Uint128::new(1000));
        }
        _ => panic!("Expected only mint message when adapter is inactive"),
    }
}

#[test]
fn test_deposit_no_adapters_stays_in_contract() {
    let deps = mock_dependencies();

    let vault_contract_addr = deps.api.addr_make("inflow");
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    let adapter_configs = HashMap::new();
    let control_center_config =
        ControlCenterMockConfig::new(control_center_contract_addr.clone(), 0, 0);
    let token_info_provider_config = TokenInfoProviderMockConfig::new(
        token_info_provider_contract_addr.clone(),
        DEPOSIT_DENOM.to_string(),
    );
    let (mut deps, _wasm_querier) = mock_dependencies_with_adapters(
        adapter_configs,
        control_center_config,
        token_info_provider_config,
    );
    let mut env = mock_env();

    env.contract.address = vault_contract_addr.clone();

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    setup_contract_with_vault_denom(&mut deps, &vault_contract_addr);

    // Mock that contract received deposit
    mock_address_balance(
        &mut deps,
        vault_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::new(1000),
    );

    // User deposits 1000 tokens
    let info = get_message_info(
        &deps.api,
        USER1,
        &[Coin {
            denom: DEPOSIT_DENOM.to_string(),
            amount: Uint128::new(1000),
        }],
    );

    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Deposit { on_behalf_of: None },
    )
    .unwrap();

    // Should only have mint message, no adapter deposit
    assert_eq!(res.messages.len(), 1);

    // Verify it's the mint message
    match &res.messages[0].msg {
        CosmosMsg::Custom(neutron_sdk::bindings::msg::NeutronMsg::MintTokens {
            denom,
            amount,
            mint_to_address: _,
        }) => {
            // Verify it's a mint tokens message
            assert!(denom.contains("hydro_inflow_uatom"));
            assert_eq!(*amount, Uint128::new(1000));
        }
        _ => panic!("Expected mint message only"),
    }
}

#[test]
fn test_deposit_with_failing_adapter_stays_in_contract() {
    let deps = mock_dependencies();

    let vault_contract_addr = deps.api.addr_make("inflow");
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make("adapter1");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Configure adapter to fail queries
    let mut adapter_configs = HashMap::new();
    adapter_configs.insert(adapter_addr.clone(), MockAdapterConfig::failing());

    let control_center_config =
        ControlCenterMockConfig::new(control_center_contract_addr.clone(), 0, 0);
    let token_info_provider_config = TokenInfoProviderMockConfig::new(
        token_info_provider_contract_addr.clone(),
        DEPOSIT_DENOM.to_string(),
    );
    let (mut deps, _wasm_querier) = mock_dependencies_with_adapters(
        adapter_configs,
        control_center_config,
        token_info_provider_config,
    );
    let mut env = mock_env();

    env.contract.address = vault_contract_addr.clone();

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    setup_contract_with_vault_denom(&mut deps, &vault_contract_addr);

    // Register the failing adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "failing_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: Some("Adapter that fails".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Mock that contract received deposit
    mock_address_balance(
        &mut deps,
        vault_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::new(1000),
    );

    // User deposits 1000 tokens
    let info = get_message_info(
        &deps.api,
        USER1,
        &[Coin {
            denom: DEPOSIT_DENOM.to_string(),
            amount: Uint128::new(1000),
        }],
    );

    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Deposit { on_behalf_of: None },
    )
    .unwrap();

    // Should only have mint message, no adapter deposit (adapter query failed)
    assert_eq!(res.messages.len(), 1);

    // Verify it's the mint message only - funds stayed in contract
    match &res.messages[0].msg {
        CosmosMsg::Custom(neutron_sdk::bindings::msg::NeutronMsg::MintTokens {
            denom,
            amount,
            mint_to_address: _,
        }) => {
            assert!(denom.contains("hydro_inflow_uatom"));
            assert_eq!(*amount, Uint128::new(1000));
        }
        _ => panic!("Expected only mint message when adapter query fails"),
    }
}

#[test]
fn test_deposit_skips_failing_adapter() {
    let deps = mock_dependencies();

    let vault_contract_addr = deps.api.addr_make("inflow");
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let failing_adapter_addr = deps.api.addr_make("adapter1");
    let working_adapter_addr = deps.api.addr_make("adapter2");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Configure first adapter to fail, second to work
    let mut adapter_configs = HashMap::new();
    adapter_configs.insert(failing_adapter_addr.clone(), MockAdapterConfig::failing());
    adapter_configs.insert(
        working_adapter_addr.clone(),
        MockAdapterConfig::new(10000, 0, 0),
    );

    let control_center_config =
        ControlCenterMockConfig::new(control_center_contract_addr.clone(), 0, 0);
    let token_info_provider_config = TokenInfoProviderMockConfig::new(
        token_info_provider_contract_addr.clone(),
        DEPOSIT_DENOM.to_string(),
    );
    let (mut deps, _wasm_querier) = mock_dependencies_with_adapters(
        adapter_configs,
        control_center_config,
        token_info_provider_config,
    );
    let mut env = mock_env();

    env.contract.address = vault_contract_addr.clone();

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    setup_contract_with_vault_denom(&mut deps, &vault_contract_addr);

    // Register first adapter (failing)
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "failing_adapter".to_string(),
            address: failing_adapter_addr.to_string(),
            description: Some("First adapter that fails".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Register second adapter (working)
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "working_adapter".to_string(),
            address: working_adapter_addr.to_string(),
            description: Some("Second adapter that works".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Mock that contract received deposit
    mock_address_balance(
        &mut deps,
        vault_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::new(1000),
    );

    // User deposits 1000 tokens
    let info = get_message_info(
        &deps.api,
        USER1,
        &[Coin {
            denom: DEPOSIT_DENOM.to_string(),
            amount: Uint128::new(1000),
        }],
    );

    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Deposit { on_behalf_of: None },
    )
    .unwrap();

    // Should have 2 messages: adapter deposit (to working adapter only) + mint shares
    assert_eq!(res.messages.len(), 2);

    // First message should be adapter deposit to the working adapter (second one)
    match &res.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds,
        }) => {
            // Verify it's sent to the working adapter, not the failing one
            assert_eq!(contract_addr, &working_adapter_addr.to_string());
            assert_eq!(funds.len(), 1);
            assert_eq!(funds[0].amount, Uint128::new(1000));
            assert_eq!(funds[0].denom, DEPOSIT_DENOM);
            // Verify it's a Deposit message (wrapped in interface structure)
            let adapter_msg = deserialize_adapter_interface_msg(msg).unwrap();
            assert!(matches!(
                adapter_msg,
                interface::inflow_adapter::AdapterInterfaceMsg::Deposit { .. }
            ));
        }
        _ => panic!("Expected WasmMsg::Execute for adapter deposit to working adapter"),
    }

    // Verify attributes
    assert_eq!(res.attributes[0].value, "deposit");

    // Verify failed_adapter_queries attribute is present and contains the failing adapter name
    let failed_adapters_attr = res
        .attributes
        .iter()
        .find(|a| a.key == "failed_adapter_queries");
    assert!(
        failed_adapters_attr.is_some(),
        "Expected failed_adapter_queries attribute to be present"
    );
    assert!(
        failed_adapters_attr
            .unwrap()
            .value
            .contains("failing_adapter"),
        "Expected failing adapter name in the attribute value"
    );
}

// ============================================================================
// Test Group 2: Withdrawal with Adapter Tests
// ============================================================================

#[test]
fn test_withdraw_all_or_nothing_can_fulfill() {
    let deps = mock_dependencies();

    let vault_contract_addr = deps.api.addr_make("inflow");
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make("adapter1");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Configure adapter with 3000 tokens deposited and available for withdrawal
    let mut adapter_configs = HashMap::new();
    adapter_configs.insert(
        adapter_addr.clone(),
        MockAdapterConfig::new(0, 3000, 3000), // 3000 available for withdraw, 3000 current deposit
    );
    // Control center tracks total pool value = 10000 (2000 contract + 3000 adapter + 5000 deployed elsewhere)
    let control_center_config = ControlCenterMockConfig::new(
        control_center_contract_addr.clone(),
        10000, // total_pool_value includes 5000 "deployed" amount
        0,     // no shares issued yet
    );
    let token_info_provider_config = TokenInfoProviderMockConfig::new(
        token_info_provider_contract_addr.clone(),
        DEPOSIT_DENOM.to_string(),
    );
    let (mut deps, wasm_querier) = mock_dependencies_with_adapters(
        adapter_configs,
        control_center_config,
        token_info_provider_config,
    );
    let mut env = mock_env();

    env.contract.address = vault_contract_addr.clone();

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    setup_contract_with_vault_denom(&mut deps, &vault_contract_addr);

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: Some("Mars Protocol".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Mock contract balance with 2000 tokens
    mock_address_balance(
        &mut deps,
        vault_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::new(2000),
    );

    // User deposits 2000 tokens
    // Since there are no shares yet (total supply = 0), they get 2000 shares (1:1 ratio)
    // These 2000 shares represent 100% of the vault
    // Total vault value = 2000 (contract) + 3000 (adapter) + 5000 (deployed) = 10000 tokens
    let info = get_message_info(
        &deps.api,
        USER1,
        &[Coin {
            denom: DEPOSIT_DENOM.to_string(),
            amount: Uint128::new(2000),
        }],
    );
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Deposit { on_behalf_of: None },
    )
    .unwrap();

    // Update the control center mock to reflect post-deposit state (2000 shares issued)
    update_contract_mock(
        &mut deps,
        &wasm_querier,
        setup_control_center_mock(
            control_center_contract_addr.clone(),
            DEFAULT_DEPOSIT_CAP,
            Uint128::new(10000), // total_pool_value stays the same
            Uint128::new(2000),  // 2000 shares were minted
        ),
    );

    // Mock the vault shares balance for the user (2000 shares were minted during deposit)
    // update_balance automatically recalculates the supply
    let vault_shares_denom = format!("factory/{}/hydro_inflow_uatom", vault_contract_addr);
    mock_address_balance(&mut deps, USER1, &vault_shares_denom, Uint128::new(2000));

    // User now tries to withdraw half their shares (1000 shares)
    // Their 1000 shares = 50% of vault = 5000 tokens worth
    // Available immediately: 2000 (contract) + 3000 (adapter) = 5000 tokens
    // All-or-nothing: We CAN fulfill entirely (5000 >= 5000), so withdraw and send
    let info = get_message_info(
        &deps.api,
        USER1,
        &[Coin {
            denom: format!("factory/{}/hydro_inflow_uatom", vault_contract_addr),
            amount: Uint128::new(1000), // Half of user's shares
        }],
    );

    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Withdraw { on_behalf_of: None },
    )
    .unwrap();

    // Should have 3 messages: adapter withdrawal + bank send + burn shares
    assert_eq!(res.messages.len(), 3);

    // First message should be adapter withdrawal for 3000 (remaining after contract balance)
    match &res.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds,
        }) => {
            assert_eq!(contract_addr, &adapter_addr.to_string());
            assert_eq!(funds.len(), 0);
            let adapter_msg = deserialize_adapter_interface_msg(msg).unwrap();
            match adapter_msg {
                interface::inflow_adapter::AdapterInterfaceMsg::Withdraw { coin } => {
                    assert_eq!(coin.denom, DEPOSIT_DENOM);
                    assert_eq!(coin.amount, Uint128::new(3000));
                }
                _ => panic!("Expected AdapterInterfaceMsg::Withdraw"),
            }
        }
        _ => panic!("Expected WasmMsg::Execute for adapter withdrawal"),
    }

    // Second message should be bank send for full amount (5000)
    match &res.messages[1].msg {
        CosmosMsg::Bank(cosmwasm_std::BankMsg::Send { to_address, amount }) => {
            assert_eq!(to_address, &deps.api.addr_make(USER1).to_string());
            assert_eq!(amount.len(), 1);
            assert_eq!(amount[0].denom, DEPOSIT_DENOM);
            assert_eq!(amount[0].amount, Uint128::new(5000));
        }
        _ => panic!("Expected BankMsg::Send"),
    }

    // Third message should be burn shares
    match &res.messages[2].msg {
        CosmosMsg::Custom(neutron_sdk::bindings::msg::NeutronMsg::BurnTokens {
            denom,
            amount,
            burn_from_address: _,
        }) => {
            assert!(denom.contains("hydro_inflow_uatom"));
            assert_eq!(*amount, Uint128::new(1000));
        }
        _ => panic!("Expected BurnTokens message"),
    }

    // Verify response attributes
    // attributes[0]: action = "withdraw"
    // attributes[1]: sender = USER1
    // attributes[2]: withdrawer = USER1
    // attributes[3]: vault_shares_sent = "1000"
    assert_eq!(res.attributes[0].value, "withdraw");
    assert_eq!(
        res.attributes[1].value,
        deps.api.addr_make(USER1).to_string()
    );
    assert_eq!(res.attributes[3].value, "1000"); // vault_shares_sent

    // Should have paid_out_amount = 5000
    let paid_out_attr = res
        .attributes
        .iter()
        .find(|attr| attr.key == "paid_out_amount")
        .unwrap();
    assert_eq!(paid_out_attr.value, "5000");

    // Should NOT have withdrawal_id or amount_queued_for_withdrawal (everything was fulfilled)
    assert!(!res
        .attributes
        .iter()
        .any(|attr| attr.key == "withdrawal_id"));
    assert!(!res
        .attributes
        .iter()
        .any(|attr| attr.key == "amount_queued_for_withdrawal"));
}

#[test]
fn test_withdraw_all_or_nothing_cannot_fulfill() {
    let deps = mock_dependencies();

    let vault_contract_addr = deps.api.addr_make("inflow");
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make("adapter1");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Configure adapter with 3000 tokens deposited and available for withdrawal
    let mut adapter_configs = HashMap::new();
    adapter_configs.insert(
        adapter_addr.clone(),
        MockAdapterConfig::new(0, 3000, 3000), // 3000 available for withdraw, 3000 current deposit
    );
    // Control center tracks total pool value = 10000 (2000 contract + 3000 adapter + 5000 deployed elsewhere)
    let control_center_config = ControlCenterMockConfig::new(
        control_center_contract_addr.clone(),
        10000, // total_pool_value includes 5000 "deployed" amount
        0,     // no shares issued yet
    );
    let token_info_provider_config = TokenInfoProviderMockConfig::new(
        token_info_provider_contract_addr.clone(),
        DEPOSIT_DENOM.to_string(),
    );
    let (mut deps, wasm_querier) = mock_dependencies_with_adapters(
        adapter_configs,
        control_center_config,
        token_info_provider_config,
    );
    let mut env = mock_env();

    env.contract.address = vault_contract_addr.clone();

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    setup_contract_with_vault_denom(&mut deps, &vault_contract_addr);

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: Some("Mars Protocol".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Mock contract balance with 2000 tokens
    mock_address_balance(
        &mut deps,
        vault_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::new(2000),
    );

    // User deposits 2000 tokens
    // Since there are no shares yet (total supply = 0), they get 2000 shares (1:1 ratio)
    // These 2000 shares represent 100% of the vault
    // Total vault value = 2000 (contract) + 3000 (adapter) + 5000 (deployed) = 10000 tokens
    let info = get_message_info(
        &deps.api,
        USER1,
        &[Coin {
            denom: DEPOSIT_DENOM.to_string(),
            amount: Uint128::new(2000),
        }],
    );
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Deposit { on_behalf_of: None },
    )
    .unwrap();

    // Update the control center mock to reflect post-deposit state (2000 shares issued)
    update_contract_mock(
        &mut deps,
        &wasm_querier,
        setup_control_center_mock(
            control_center_contract_addr.clone(),
            DEFAULT_DEPOSIT_CAP,
            Uint128::new(10000), // total_pool_value stays the same
            Uint128::new(2000),  // 2000 shares were minted
        ),
    );

    // Mock the vault shares balance for the user (2000 shares were minted during deposit)
    // update_balance automatically recalculates the supply
    let vault_shares_denom = format!("factory/{}/hydro_inflow_uatom", vault_contract_addr);
    mock_address_balance(&mut deps, USER1, &vault_shares_denom, Uint128::new(2000));

    // User now tries to withdraw all their shares
    // Their 2000 shares = 100% of vault = 10000 tokens worth
    // Available immediately: 2000 (contract) + 3000 (adapter) = 5000 tokens
    // All-or-nothing: We CANNOT fulfill entirely (5000 < 10000), so queue everything
    let info = get_message_info(
        &deps.api,
        USER1,
        &[Coin {
            denom: format!("factory/{}/hydro_inflow_uatom", vault_contract_addr),
            amount: Uint128::new(2000), // All user's shares
        }],
    );

    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Withdraw { on_behalf_of: None },
    )
    .unwrap();

    // Should have only 1 message: burn shares (no adapter withdrawal, no bank send)
    assert_eq!(res.messages.len(), 1);

    // Only message should be burn all shares
    match &res.messages[0].msg {
        CosmosMsg::Custom(neutron_sdk::bindings::msg::NeutronMsg::BurnTokens {
            denom,
            amount,
            burn_from_address: _,
        }) => {
            assert!(denom.contains("hydro_inflow_uatom"));
            assert_eq!(*amount, Uint128::new(2000));
        }
        _ => panic!("Expected BurnTokens message"),
    }

    // Verify response attributes
    assert_eq!(res.attributes[0].value, "withdraw");
    assert_eq!(
        res.attributes[1].value,
        deps.api.addr_make(USER1).to_string()
    );
    assert_eq!(res.attributes[3].value, "2000"); // vault_shares_sent

    // Should NOT have paid_out_amount (nothing was paid out)
    assert!(!res
        .attributes
        .iter()
        .any(|attr| attr.key == "paid_out_amount"));

    // Should have withdrawal_id and amount_queued_for_withdrawal = 10000 (entire amount)
    let withdrawal_id_attr = res
        .attributes
        .iter()
        .find(|attr| attr.key == "withdrawal_id")
        .unwrap();
    assert_eq!(withdrawal_id_attr.value, "0"); // First withdrawal

    let amount_queued_attr = res
        .attributes
        .iter()
        .find(|attr| attr.key == "amount_queued_for_withdrawal")
        .unwrap();
    assert_eq!(amount_queued_attr.value, "10000"); // Entire amount queued
}

// ============================================================================
// Test Group 3: WithdrawFromAdapter Tests
// ============================================================================

#[test]
fn test_withdraw_from_adapter_success() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: Some("Mars Protocol".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Withdraw from adapter
    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::WithdrawFromAdapter {
            adapter_name: "mars_adapter".to_string(),
            amount: Uint128::new(5000),
        },
    )
    .unwrap();

    // Should have 1 message: adapter withdrawal
    assert_eq!(res.messages.len(), 1);

    // Verify the withdrawal message
    match &res.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds,
        }) => {
            assert_eq!(contract_addr, &adapter_addr.to_string());
            assert_eq!(funds.len(), 0); // No funds sent with withdrawal

            // Verify it's a Withdraw message with correct params
            let adapter_msg = deserialize_adapter_interface_msg(msg).unwrap();
            match adapter_msg {
                interface::inflow_adapter::AdapterInterfaceMsg::Withdraw { coin } => {
                    assert_eq!(coin.denom, DEPOSIT_DENOM);
                    assert_eq!(coin.amount, Uint128::new(5000));
                }
                _ => panic!("Expected AdapterInterfaceMsg::Withdraw"),
            }
        }
        _ => panic!("Expected WasmMsg::Execute for adapter withdrawal"),
    }

    // Verify attributes
    assert_eq!(res.attributes[0].value, "withdraw_from_adapter");
    assert_eq!(res.attributes[1].value, whitelist_addr.as_str());
    assert_eq!(res.attributes[2].value, "mars_adapter");
    assert_eq!(res.attributes[3].value, "5000");
}

#[test]
fn test_withdraw_from_adapter_unauthorized() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter_addr = deps.api.addr_make(ADAPTER_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: Some("Mars Protocol".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Try to withdraw from non-whitelisted address
    let info = get_message_info(&deps.api, NON_WHITELIST_ADDR, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::WithdrawFromAdapter {
            adapter_name: "mars_adapter".to_string(),
            amount: Uint128::new(5000),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

#[test]
fn test_withdraw_from_adapter_not_found() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Try to withdraw from non-existent adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::WithdrawFromAdapter {
            adapter_name: "nonexistent_adapter".to_string(),
            amount: Uint128::new(5000),
        },
    )
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("Adapter not found: nonexistent_adapter"));
}

#[test]
fn test_deposit_to_adapter_success() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);
    let adapter_addr = deps.api.addr_make("adapter1");

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: Some("Mars lending protocol adapter".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Mock vault balance
    mock_address_balance(
        &mut deps,
        env.contract.address.as_ref(),
        DEPOSIT_DENOM,
        Uint128::new(10000),
    );

    // Deposit to adapter
    let res = execute(
        deps.as_mut(),
        env,
        info.clone(),
        ExecuteMsg::DepositToAdapter {
            adapter_name: "mars_adapter".to_string(),
            amount: Uint128::new(5000),
        },
    )
    .unwrap();

    // Verify response attributes
    assert_eq!(res.attributes.len(), 5);
    assert_eq!(res.attributes[0].value, "deposit_to_adapter");
    assert_eq!(res.attributes[1].value, whitelist_addr.as_str());
    assert_eq!(res.attributes[2].value, "mars_adapter");
    assert_eq!(res.attributes[3].value, "5000");
    assert_eq!(res.attributes[4].value, "NotTracked");

    // Verify wasm message was created
    assert_eq!(res.messages.len(), 1);

    // Verify the message content
    match &res.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds,
        }) => {
            assert_eq!(contract_addr, &adapter_addr.to_string());

            // Should send funds with the deposit
            assert_eq!(funds.len(), 1);
            assert_eq!(funds[0].denom, DEPOSIT_DENOM);
            assert_eq!(funds[0].amount, Uint128::new(5000));

            // Verify it's a Deposit message
            let adapter_msg = deserialize_adapter_interface_msg(msg).unwrap();
            match adapter_msg {
                interface::inflow_adapter::AdapterInterfaceMsg::Deposit { .. } => {
                    // Success - this is the expected message type
                }
                _ => panic!("Expected AdapterInterfaceMsg::Deposit"),
            }
        }
        _ => panic!("Expected WasmMsg::Execute for adapter deposit"),
    }
}

#[test]
fn test_deposit_to_adapter_insufficient_balance() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);
    let adapter_addr = deps.api.addr_make("adapter1");

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: Some("Mars lending protocol adapter".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Mock vault balance (less than requested)
    mock_address_balance(
        &mut deps,
        env.contract.address.as_ref(),
        DEPOSIT_DENOM,
        Uint128::new(1000),
    );

    // Try to deposit more than available
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::DepositToAdapter {
            adapter_name: "mars_adapter".to_string(),
            amount: Uint128::new(5000),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Insufficient vault balance"));
}

#[test]
fn test_deposit_to_adapter_not_whitelisted() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);
    let adapter_addr = deps.api.addr_make("adapter1");

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: Some("Mars lending protocol adapter".to_string()),
            allocation_mode: AllocationMode::Automated,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Try to deposit from non-whitelisted address
    let info = get_message_info(&deps.api, NON_WHITELIST_ADDR, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::DepositToAdapter {
            adapter_name: "mars_adapter".to_string(),
            amount: Uint128::new(5000),
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

#[test]
fn test_deposit_to_adapter_adapter_not_found() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Try to deposit to non-existent adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::DepositToAdapter {
            adapter_name: "nonexistent_adapter".to_string(),
            amount: Uint128::new(5000),
        },
    )
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("Adapter not found: nonexistent_adapter"));
}

#[test]
fn test_deposit_to_adapter_works_regardless_of_allocation_flag() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);
    let adapter_addr = deps.api.addr_make("adapter1");

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapter with auto_allocation = false
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter_addr.to_string(),
            description: Some("Mars lending protocol adapter".to_string()),
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Mock vault balance
    mock_address_balance(
        &mut deps,
        env.contract.address.as_ref(),
        DEPOSIT_DENOM,
        Uint128::new(10000),
    );

    // Deposit to adapter should work even though it's excluded from automated allocation
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::DepositToAdapter {
            adapter_name: "mars_adapter".to_string(),
            amount: Uint128::new(5000),
        },
    )
    .unwrap();

    // Verify it succeeded
    assert_eq!(res.attributes[0].value, "deposit_to_adapter");
    assert_eq!(res.messages.len(), 1);
}

// ============================================================================
// Test Group 4: MoveAdapterFunds Tests
// ============================================================================

#[test]
fn test_move_adapter_funds_deposit_denom_success() {
    let mut deps = mock_dependencies();
    let mut env = mock_env();

    let vault_contract_addr = deps.api.addr_make("vault");
    env.contract.address = vault_contract_addr.clone();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter1_addr = deps.api.addr_make("adapter1");
    let adapter2_addr = deps.api.addr_make("adapter2");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register two adapters
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter1_addr.to_string(),
            description: Some("Mars Protocol".to_string()),
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "osmosis_adapter".to_string(),
            address: adapter2_addr.to_string(),
            description: Some("Osmosis DEX".to_string()),
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Mock vault balance to ensure deposit_to_adapter has sufficient funds
    mock_address_balance(
        &mut deps,
        vault_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::new(10000),
    );

    // Move funds between adapters
    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::MoveAdapterFunds {
            from_adapter: "mars_adapter".to_string(),
            to_adapter: "osmosis_adapter".to_string(),
            coin: Coin {
                denom: DEPOSIT_DENOM.to_string(),
                amount: Uint128::new(5000),
            },
        },
    )
    .unwrap();

    // Verify we have 2 messages (withdraw + deposit)
    assert_eq!(res.messages.len(), 2);

    // Verify first message is withdraw from mars_adapter
    match &res.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds,
        }) => {
            assert_eq!(contract_addr, &adapter1_addr.to_string());
            assert_eq!(funds.len(), 0);

            let adapter_msg = deserialize_adapter_interface_msg(msg).unwrap();
            match adapter_msg {
                interface::inflow_adapter::AdapterInterfaceMsg::Withdraw { coin } => {
                    assert_eq!(coin.denom, DEPOSIT_DENOM);
                    assert_eq!(coin.amount, Uint128::new(5000));
                }
                _ => panic!("Expected AdapterInterfaceMsg::Withdraw"),
            }
        }
        _ => panic!("Expected WasmMsg::Execute for adapter withdrawal"),
    }

    // Verify second message is deposit to osmosis_adapter
    match &res.messages[1].msg {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds,
        }) => {
            assert_eq!(contract_addr, &adapter2_addr.to_string());
            assert_eq!(funds.len(), 1);
            assert_eq!(funds[0].denom, DEPOSIT_DENOM);
            assert_eq!(funds[0].amount, Uint128::new(5000));

            let adapter_msg = deserialize_adapter_interface_msg(msg).unwrap();
            match adapter_msg {
                interface::inflow_adapter::AdapterInterfaceMsg::Deposit { .. } => {
                    // Success
                }
                _ => panic!("Expected AdapterInterfaceMsg::Deposit"),
            }
        }
        _ => panic!("Expected WasmMsg::Execute for adapter deposit"),
    }

    // Verify response attributes
    assert_eq!(res.attributes[0].value, "move_adapter_funds");
    assert_eq!(res.attributes[1].value, whitelist_addr.as_str());
    assert_eq!(res.attributes[2].value, "mars_adapter");
    assert_eq!(res.attributes[3].value, "osmosis_adapter");
    assert_eq!(res.attributes[4].value, DEPOSIT_DENOM);
    assert_eq!(res.attributes[5].value, "5000");
}

#[test]
fn test_move_adapter_funds_non_deposit_denom_matching_tracking() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter1_addr = deps.api.addr_make("adapter1");
    let adapter2_addr = deps.api.addr_make("adapter2");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register two adapters with SAME DeploymentTracking (NotTracked)
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter1_addr.to_string(),
            description: Some("Mars Protocol".to_string()),
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "osmosis_adapter".to_string(),
            address: adapter2_addr.to_string(),
            description: Some("Osmosis DEX".to_string()),
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Move non-deposit_denom funds
    let other_denom = "ibc/OTHER";
    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::MoveAdapterFunds {
            from_adapter: "mars_adapter".to_string(),
            to_adapter: "osmosis_adapter".to_string(),
            coin: Coin {
                denom: other_denom.to_string(),
                amount: Uint128::new(3000),
            },
        },
    )
    .unwrap();

    // Verify we have 2 messages (withdraw + deposit)
    assert_eq!(res.messages.len(), 2);

    // Verify first message is withdraw
    match &res.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds,
        }) => {
            assert_eq!(contract_addr, &adapter1_addr.to_string());
            assert_eq!(funds.len(), 0);

            let adapter_msg = deserialize_adapter_interface_msg(msg).unwrap();
            match adapter_msg {
                interface::inflow_adapter::AdapterInterfaceMsg::Withdraw { coin } => {
                    assert_eq!(coin.denom, other_denom);
                    assert_eq!(coin.amount, Uint128::new(3000));
                }
                _ => panic!("Expected AdapterInterfaceMsg::Withdraw"),
            }
        }
        _ => panic!("Expected WasmMsg::Execute"),
    }

    // Verify second message is deposit
    match &res.messages[1].msg {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds,
        }) => {
            assert_eq!(contract_addr, &adapter2_addr.to_string());
            assert_eq!(funds.len(), 1);
            assert_eq!(funds[0].denom, other_denom);
            assert_eq!(funds[0].amount, Uint128::new(3000));

            let adapter_msg = deserialize_adapter_interface_msg(msg).unwrap();
            match adapter_msg {
                interface::inflow_adapter::AdapterInterfaceMsg::Deposit { .. } => {}
                _ => panic!("Expected AdapterInterfaceMsg::Deposit"),
            }
        }
        _ => panic!("Expected WasmMsg::Execute"),
    }

    // Verify attributes
    assert_eq!(res.attributes[0].value, "move_adapter_funds");
    assert_eq!(res.attributes[4].value, other_denom);
}

#[test]
fn test_move_adapter_funds_non_deposit_denom_tracking_mismatch() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter1_addr = deps.api.addr_make("adapter1");
    let adapter2_addr = deps.api.addr_make("adapter2");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register two adapters with DIFFERENT DeploymentTracking
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter1_addr.to_string(),
            description: Some("Mars Protocol".to_string()),
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::Tracked, // Tracked
        },
    )
    .unwrap();

    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "osmosis_adapter".to_string(),
            address: adapter2_addr.to_string(),
            description: Some("Osmosis DEX".to_string()),
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::NotTracked, // NotTracked
        },
    )
    .unwrap();

    // Try to move non-deposit_denom funds - should fail
    let other_denom = "ibc/OTHER";
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::MoveAdapterFunds {
            from_adapter: "mars_adapter".to_string(),
            to_adapter: "osmosis_adapter".to_string(),
            coin: Coin {
                denom: other_denom.to_string(),
                amount: Uint128::new(3000),
            },
        },
    )
    .unwrap_err();

    // Verify error message contains tracking mismatch
    assert!(err
        .to_string()
        .contains("Adapter deployment tracking mismatch"));
    assert!(err.to_string().contains("mars_adapter"));
    assert!(err.to_string().contains("osmosis_adapter"));
}

#[test]
fn test_move_adapter_funds_unauthorized() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter1_addr = deps.api.addr_make("adapter1");
    let adapter2_addr = deps.api.addr_make("adapter2");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapters
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter1_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RegisterAdapter {
            name: "osmosis_adapter".to_string(),
            address: adapter2_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Try to move funds from non-whitelisted address
    let info = get_message_info(&deps.api, NON_WHITELIST_ADDR, &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::MoveAdapterFunds {
            from_adapter: "mars_adapter".to_string(),
            to_adapter: "osmosis_adapter".to_string(),
            coin: Coin {
                denom: DEPOSIT_DENOM.to_string(),
                amount: Uint128::new(5000),
            },
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

#[test]
fn test_move_adapter_funds_zero_amount() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter1_addr = deps.api.addr_make("adapter1");
    let adapter2_addr = deps.api.addr_make("adapter2");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register adapters
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter1_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "osmosis_adapter".to_string(),
            address: adapter2_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Try to move zero amount
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::MoveAdapterFunds {
            from_adapter: "mars_adapter".to_string(),
            to_adapter: "osmosis_adapter".to_string(),
            coin: Coin {
                denom: DEPOSIT_DENOM.to_string(),
                amount: Uint128::zero(),
            },
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("Zero amount"));
}

#[test]
fn test_move_adapter_funds_from_adapter_not_found() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter2_addr = deps.api.addr_make("adapter2");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register only destination adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "osmosis_adapter".to_string(),
            address: adapter2_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Try to move from non-existent adapter
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::MoveAdapterFunds {
            from_adapter: "nonexistent_adapter".to_string(),
            to_adapter: "osmosis_adapter".to_string(),
            coin: Coin {
                denom: DEPOSIT_DENOM.to_string(),
                amount: Uint128::new(5000),
            },
        },
    )
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("Adapter not found: nonexistent_adapter"));
}

#[test]
fn test_move_adapter_funds_to_adapter_not_found() {
    let mut deps = mock_dependencies();
    let env = mock_env();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter1_addr = deps.api.addr_make("adapter1");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register only source adapter
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter1_addr.to_string(),
            description: None,
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::NotTracked,
        },
    )
    .unwrap();

    // Try to move to non-existent adapter
    let err = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::MoveAdapterFunds {
            from_adapter: "mars_adapter".to_string(),
            to_adapter: "nonexistent_adapter".to_string(),
            coin: Coin {
                denom: DEPOSIT_DENOM.to_string(),
                amount: Uint128::new(5000),
            },
        },
    )
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("Adapter not found: nonexistent_adapter"));
}

#[test]
fn test_move_adapter_funds_deposit_denom_with_tracked_deployment() {
    let mut deps = mock_dependencies();
    let mut env = mock_env();

    let vault_contract_addr = deps.api.addr_make("vault");
    env.contract.address = vault_contract_addr.clone();

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let adapter1_addr = deps.api.addr_make("adapter1");
    let adapter2_addr = deps.api.addr_make("adapter2");
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    // Instantiate contract
    let instantiate_msg = get_default_instantiate_msg(
        &deps.api,
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(
        &deps.api,
        "creator",
        &get_initial_deposit_funds(DEPOSIT_DENOM),
    );
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Register two adapters with Tracked deployment
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "mars_adapter".to_string(),
            address: adapter1_addr.to_string(),
            description: Some("Mars Protocol".to_string()),
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::Tracked,
        },
    )
    .unwrap();

    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RegisterAdapter {
            name: "osmosis_adapter".to_string(),
            address: adapter2_addr.to_string(),
            description: Some("Osmosis DEX".to_string()),
            allocation_mode: AllocationMode::Manual,
            deployment_tracking: DeploymentTracking::Tracked,
        },
    )
    .unwrap();

    // Setup mocks for token info provider
    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter([
        setup_control_center_mock(
            control_center_contract_addr.clone(),
            DEFAULT_DEPOSIT_CAP,
            Uint128::zero(),
            Uint128::zero(),
        ),
        setup_token_info_provider_mock(
            token_info_provider_contract_addr,
            DEPOSIT_DENOM.to_string(),
            Decimal::one(),
        ),
    ]));

    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    // Mock vault balance
    mock_address_balance(
        &mut deps,
        vault_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::new(10000),
    );

    // Move deposit_denom funds between tracked adapters
    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::MoveAdapterFunds {
            from_adapter: "mars_adapter".to_string(),
            to_adapter: "osmosis_adapter".to_string(),
            coin: Coin {
                denom: DEPOSIT_DENOM.to_string(),
                amount: Uint128::new(5000),
            },
        },
    )
    .unwrap();

    // Should have 4 messages:
    // 1. Withdraw from mars_adapter
    // 2. Update deployed amount (subtract)
    // 3. Deposit to osmosis_adapter
    // 4. Update deployed amount (add)
    assert_eq!(res.messages.len(), 4);

    // Verify withdraw message
    match &res.messages[0].msg {
        CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, .. }) => {
            assert_eq!(contract_addr, &adapter1_addr.to_string());
        }
        _ => panic!("Expected WasmMsg::Execute for withdraw"),
    }

    // Verify subtract deployed amount message
    match &res.messages[1].msg {
        CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, .. }) => {
            assert_eq!(contract_addr, &control_center_contract_addr.to_string());
        }
        _ => panic!("Expected WasmMsg::Execute for update deployed amount"),
    }

    // Verify deposit message
    match &res.messages[2].msg {
        CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, .. }) => {
            assert_eq!(contract_addr, &adapter2_addr.to_string());
        }
        _ => panic!("Expected WasmMsg::Execute for deposit"),
    }

    // Verify add deployed amount message
    match &res.messages[3].msg {
        CosmosMsg::Wasm(WasmMsg::Execute { contract_addr, .. }) => {
            assert_eq!(contract_addr, &control_center_contract_addr.to_string());
        }
        _ => panic!("Expected WasmMsg::Execute for update deployed amount"),
    }

    // Verify attributes
    assert_eq!(res.attributes[0].value, "move_adapter_funds");
    assert_eq!(res.attributes[2].value, "mars_adapter");
    assert_eq!(res.attributes[3].value, "osmosis_adapter");
}
