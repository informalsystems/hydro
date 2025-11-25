use std::{
    str::FromStr,
    thread,
    time::{Duration, UNIX_EPOCH},
};

use cosmwasm_std::{Binary, Decimal, Timestamp, Uint128};

use cw_orch::{anyhow, prelude::*};

use cw_orch_interface::{hydro::*, tribute::*};
use hydro::{
    msg::{TokenInfoProviderInstantiateMsg, TrancheInfo},
    query::QueryMsgFns as HydroQueryMsgFn,
};
use tribute::query::QueryMsgFns as TributeQueryMsgFns;

pub fn get_default_power_schedule_vec() -> Vec<(u64, Decimal)> {
    vec![
        (1, Decimal::from_str("1").unwrap()),
        (2, Decimal::from_str("1.25").unwrap()),
        (3, Decimal::from_str("1.5").unwrap()),
        (6, Decimal::from_str("2").unwrap()),
        (12, Decimal::from_str("4").unwrap()),
    ]
}

pub fn get_lsm_token_info_provider_init_info(
    hub_transfer_channel_id: String,
) -> TokenInfoProviderInstantiateMsg {
    TokenInfoProviderInstantiateMsg::LSM {
        code_id: 0,
        msg: Binary::default(),
        admin: None,
        label: "LSM Token Information Provider".to_string(),
        hub_transfer_channel_id,
    }
}

#[test]
pub fn e2e_basic_test() -> anyhow::Result<()> {
    let mut mnemonic = String::new();
    for arg in std::env::args() {
        if arg.starts_with("mnemonic: ") {
            mnemonic = arg.strip_prefix("mnemonic: ").unwrap().to_string();
            break;
        }
    }

    if mnemonic.is_empty() {
        panic!("mnemonic is required, but it wasn't set");
    }
    std::env::set_var("TEST_MNEMONIC", mnemonic);

    let (network, whitelist_admin_address) = get_neutron_testnet_chain_config();
    let chain = DaemonBuilder::new(network).build()?;
    let hydro = Hydro::new(chain.clone());

    let first_round_start = Timestamp::from_nanos(
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_nanos() as u64
            + 15000000000,
    );
    let round_length = 3_600_000_000_000; // 1 hour

    // neutrond q ibc channel channels --node https://rpc-falcron.pion-1.ntrn.tech
    // find the provider-consumer channel and use its connection-id in next command
    // neutrond q ibc channel connections [CONNECTION-ID] --node https://rpc-falcron.pion-1.ntrn.tech
    // let hub_connection_id = "connection-42".to_string();
    let hub_transfer_channel_id = "channel-96".to_string();

    hydro.upload()?;
    hydro.instantiate(
        &hydro::msg::InstantiateMsg {
            first_round_start,
            round_length,
            lock_epoch_length: round_length,
            max_locked_tokens: Uint128::new(1000000000),
            tranches: vec![
                TrancheInfo {
                    name: "tranche 1".to_string(),
                    metadata: "tranche 1 metadata".to_string(),
                },
                TrancheInfo {
                    name: "tranche 2".to_string(),
                    metadata: "tranche 2 metadata".to_string(),
                },
            ],
            whitelist_admins: vec![whitelist_admin_address.clone()],
            initial_whitelist: vec![whitelist_admin_address.clone()],
            max_deployment_duration: 12,
            round_lock_power_schedule: get_default_power_schedule_vec(),
            token_info_providers: vec![get_lsm_token_info_provider_init_info(
                hub_transfer_channel_id,
            )],
            gatekeeper: None,
            cw721_collection_info: None,
            lock_depth_limit: 50,
            lock_expiry_duration_seconds: 60 * 60 * 24 * 30 * 6, // 6 months,
            slash_percentage_threshold: Decimal::percent(50),
            slash_tokens_receiver_addr: String::new(),
            lockup_conversion_fee_percent: Decimal::percent(2),
        },
        Some(&Addr::unchecked(whitelist_admin_address.clone())),
        &[],
    )?;

    let constants_response = hydro.constants()?;
    assert_eq!(constants_response.constants.round_length, round_length);

    // wait for the first round to start
    thread::sleep(Duration::from_secs(15));

    let tribute = Tribute::new(chain.clone());
    tribute.upload()?;

    tribute.instantiate(
        &tribute::msg::InstantiateMsg {
            hydro_contract: hydro.addr_str()?,
        },
        None,
        &[],
    )?;

    let config_response = tribute.config()?;
    assert_eq!(config_response.config.hydro_contract, hydro.address()?);

    Ok(())
}

fn get_neutron_testnet_chain_config() -> (ChainInfo, String) {
    (
        networks::PION_1.clone(),
        String::from("neutron1r6rv879netg009eh6ty23v57qrq29afecuehlm"),
    )
}

// fn get_local_chain_config() -> (ChainInfo, String) {
//     let network = ChainInfo {
//         kind: ChainKind::Local,
//         chain_id: "neutron",
//         gas_denom: "untrn",
//         gas_price: 0.005,
//         grpc_urls: &["tcp://localhost:9101"],
//         network_info: NetworkInfo {
//             chain_name: "neutron",
//             pub_address_prefix: "neutron",
//             coin_type: 118u32,
//         },
//         lcd_url: None,
//         fcd_url: None,
//     };

//     (
//         network,
//         String::from("neutron1e35997edcs7rc28sttwd436u0e83jw6c02qnnj"),
//     )
// }
