use cosmwasm_std::{Env, StdResult, Uint128};
use serde_json::json;

use crate::state::{ChainConfig, Config, TokenConfig};

/// Construct the IBC Eureka memo for bridging tokens to an EVM chain via Cosmos Hub.
///
/// The outer IBC transfer (Neutron→Hub) uses this as its memo, triggering a wasm call
/// on Cosmos Hub that forwards tokens to the EVM chain via IBC Eureka.
pub fn construct_eureka_memo(
    config: &Config,
    chain_config: &ChainConfig,
    token_config: &TokenConfig,
    ethereum_recipient: &str,
    recover_address: &str,
    eureka_fee_amount: Uint128,
    env: &Env,
) -> StdResult<String> {
    let timeout_nanos = env.block.time.nanos() + config.ibc_default_timeout_seconds * 1_000_000_000;
    let timeout_seconds = env.block.time.seconds() + config.ibc_default_timeout_seconds;

    let memo = json!({
        "dest_callback": {
            "address": config.skip_ibc_adapter
        },
        "wasm": {
            "contract": config.skip_entry_point,
            "msg": {
                "action": {
                    "action": {
                        "ibc_transfer": {
                            "ibc_info": {
                                "encoding": "application/x-solidity-abi",
                                "eureka_fee": {
                                    "coin": {
                                        "amount": eureka_fee_amount.to_string(),
                                        "denom": token_config.hub_denom
                                    },
                                    "receiver": chain_config.eureka_fee_receiver,
                                    "timeout_timestamp": timeout_nanos
                                },
                                "memo": "",
                                "receiver": ethereum_recipient,
                                "recover_address": recover_address,
                                "source_channel": chain_config.eureka_source_channel
                            }
                        }
                    },
                    "exact_out": false,
                    "timeout_timestamp": timeout_seconds
                }
            }
        }
    });

    serde_json::to_string(&memo).map_err(|e| {
        cosmwasm_std::StdError::generic_err(format!("Failed to serialize Eureka memo: {}", e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::mock_env;
    use cosmwasm_std::Uint128;

    fn test_config() -> Config {
        Config {
            skip_entry_point: "cosmos1clswlqlfm8gpn7n5wu0ypu0ugaj36urlhj7yz30hn7v7mkcm2tuqy9f8s5"
                .to_string(),
            skip_ibc_adapter: "cosmos1lqu9662kd4my6dww4gzp3730vew0gkwe0nl9ztjh0n5da0a8zc4swsvd22"
                .to_string(),
            neutron_to_hub_channel: "channel-1".to_string(),
            ibc_default_timeout_seconds: 600,
        }
    }

    fn test_chain_config() -> ChainConfig {
        ChainConfig {
            chain_id: "ethereum-1".to_string(),
            eureka_source_channel: "08-wasm-1369".to_string(),
            eureka_fee_receiver: "cosmos1066ea436np9m6gf4q95q0nte2ctq84wuzahttk".to_string(),
            min_eureka_fee: Uint128::new(100),
            max_eureka_fee: Uint128::new(10000),
        }
    }

    fn test_token_config() -> TokenConfig {
        TokenConfig {
            denom: "ibc/0E293A7622DC9A6439DB60E6D234B5AF446962E27CA3AB44D0590603DFF6968E"
                .to_string(),
            hub_denom: "ibc/D742E8566B0B8CC8F569D950051C09CF57988A88F0E45574BFB3079D41DE6462"
                .to_string(),
        }
    }

    #[test]
    fn test_memo_structure() {
        let config = test_config();
        let chain_config = test_chain_config();
        let token_config = test_token_config();
        let env = mock_env();

        let memo = construct_eureka_memo(
            &config,
            &chain_config,
            &token_config,
            "fa82c937fc0f6fd3bc6c66f612cf5b539d489d21",
            "cosmos1k64ssp5pnkmwtndfzvgtnjmhx06w8mdvhpatyg",
            Uint128::new(306),
            &env,
        )
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&memo).unwrap();

        // dest_callback
        assert_eq!(parsed["dest_callback"]["address"], config.skip_ibc_adapter);

        // wasm contract
        assert_eq!(parsed["wasm"]["contract"], config.skip_entry_point);

        // ibc_info fields
        let ibc_info = &parsed["wasm"]["msg"]["action"]["action"]["ibc_transfer"]["ibc_info"];
        assert_eq!(ibc_info["encoding"], "application/x-solidity-abi");
        assert_eq!(
            ibc_info["source_channel"],
            chain_config.eureka_source_channel
        );
        assert_eq!(
            ibc_info["receiver"],
            "fa82c937fc0f6fd3bc6c66f612cf5b539d489d21"
        );
        assert_eq!(
            ibc_info["recover_address"],
            "cosmos1k64ssp5pnkmwtndfzvgtnjmhx06w8mdvhpatyg"
        );
        assert_eq!(ibc_info["memo"], "");

        // eureka_fee
        let fee = &ibc_info["eureka_fee"];
        assert_eq!(fee["coin"]["amount"], "306");
        assert_eq!(fee["coin"]["denom"], token_config.hub_denom);
        assert_eq!(fee["receiver"], chain_config.eureka_fee_receiver);

        // action-level fields
        let action = &parsed["wasm"]["msg"]["action"];
        assert_eq!(action["exact_out"], false);
        assert!(action["timeout_timestamp"].is_number());

        // timeout_timestamp in action is in seconds; eureka_fee timeout_timestamp is in nanos
        let action_timeout = action["timeout_timestamp"].as_u64().unwrap();
        let fee_timeout = fee["timeout_timestamp"].as_u64().unwrap();
        assert!(fee_timeout > action_timeout); // nanos >> seconds
    }
}
