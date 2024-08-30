use std::time::UNIX_EPOCH;

use cosmwasm_std::{Decimal, Timestamp, Uint128};

use cw_orch::{anyhow, prelude::*};

use hydro::{msg::TrancheInfo, query::QueryMsgFns as HydroQueryMsgFns};
use interface::{hydro::*, tribute::*};
use tribute::query::QueryMsgFns as TributeQueryMsgFns;

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
    let round_length = 30000000000;

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
            max_validator_shares_participating: 500,
            hub_transfer_channel_id: "channel-0".to_string(),
        },
        None,
        None,
    )?;

    let constants_response = hydro.constants()?;
    assert_eq!(constants_response.constants.round_length, round_length);

    let tribute = Tribute::new(chain.clone());
    tribute.upload()?;

    tribute.instantiate(
        &tribute::msg::InstantiateMsg {
            hydro_contract: hydro.addr_str()?,
            top_n_props_count: 10,
            community_pool_config: tribute::msg::CommunityPoolConfig {
                tax_percent: Decimal::percent(10),
                channel_id: "channel-1".to_string(),
                community_pool_address: "community-pool-address".to_string(), // TODO: fill this in
            },
        },
        None,
        None,
    )?;

    let config_response = tribute.config()?;
    assert_eq!(config_response.config.hydro_contract, hydro.address()?);

    Ok(())
}

fn get_neutron_testnet_chain_config() -> (ChainInfo, String) {
    (
        networks::PION_1.clone(),
        String::from("neutron1e68032v8dr8rfeg9wuhd3jjsun83vvla2fsrfs"),
    )
}

// fn get_local_chain_config() -> (ChainInfo, String) {
//     let network = ChainInfo {
//         kind: ChainKind::Local,
//         chain_id: "neutron",
//         gas_denom: "stake",
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
