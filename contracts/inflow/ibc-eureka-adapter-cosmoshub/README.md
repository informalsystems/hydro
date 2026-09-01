### IBC Eureka Inflow Adapter ###
A smart contract that allows us to bridge tokens from Cosmos Hub to EVM* chains by using IBC Eureka. It relies on Skip:Go infrastructure to take care of relaying for a certain fee. Fee is paid in the same denom as the token that is being bridged.

*Note that currently only Ethereum is supported.

An example of request against the Skip:Go API to obtain a messages that need to be send in order to bridge wBTC from Cosmos Hub to Ethereum:

```
curl --request POST \
  --url https://api.skip.build/v2/fungible/msgs \
  --header 'Content-Type: application/json' \
  --data '
{
    "source_asset_denom": "ibc/D742E8566B0B8CC8F569D950051C09CF57988A88F0E45574BFB3079D41DE6462",
    "source_asset_chain_id": "cosmoshub-4",
    "dest_asset_denom": "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599",
    "dest_asset_chain_id": "1",
    "amount_in": "10000000",
    "amount_out": "9999787",
    "address_list":[
      "cosmos1k00qymyetxt36uga8kp88nxyj3hvxfn2v0hgnn",
      "0xc2a7e8FEe8fF0B35e752345772C46ba1Db36a6eF"
    ],
    "operations": [
        {
            "eureka_transfer": {
                "destination_port": "transfer",
                "source_client": "08-wasm-1369",
                "from_chain_id": "cosmoshub-4",
                "to_chain_id": "1",
                "pfm_enabled": true,
                "supports_memo": true,
                "denom_in": "ibc/D742E8566B0B8CC8F569D950051C09CF57988A88F0E45574BFB3079D41DE6462",
                "denom_out": "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599",
                "entry_contract_address": "cosmos1clswlqlfm8gpn7n5wu0ypu0ugaj36urlhj7yz30hn7v7mkcm2tuqy9f8s5",
                "callback_adapter_contract_address": "cosmos1lqu9662kd4my6dww4gzp3730vew0gkwe0nl9ztjh0n5da0a8zc4swsvd22",
                "bridge_id": "EUREKA",
                "smart_relay": true,
                "smart_relay_fee_quote": {
                    "fee_amount": "213",
                    "relayer_address": "",
                    "expiration": "2026-08-31T09:00:00Z",
                    "fee_denom": "ibc/D742E8566B0B8CC8F569D950051C09CF57988A88F0E45574BFB3079D41DE6462",
                    "fee_payment_address": "cosmos1066ea436np9m6gf4q95q0nte2ctq84wuzahttk"
                }
            },
            "tx_index": 0,
            "amount_in": "10000000",
            "amount_out": "9999787"
        }
    ]
}
'
```

Use the data from the request above and the response received in order to instantiate this smart contract. Example instantiation message:

```
{
    "admins": ["cosmos1k00qymyetxt36uga8kp88nxyj3hvxfn2v0hgnn"],
    "skip_swap_entry_point_contract": "cosmos1clswlqlfm8gpn7n5wu0ypu0ugaj36urlhj7yz30hn7v7mkcm2tuqy9f8s5",
    "source_channel": "08-wasm-1369",
    "eureka_fee_receiver": "cosmos1066ea436np9m6gf4q95q0nte2ctq84wuzahttk",
    // Skip:Go uses 12 hours; transfers may fail if this value is too small
    "ibc_transfer_timeout_seconds": 43200,
    "initial_depositors": [{"address": "cosmos1k00qymyetxt36uga8kp88nxyj3hvxfn2v0hgnn"}],
    "initial_denoms": ["ibc/D742E8566B0B8CC8F569D950051C09CF57988A88F0E45574BFB3079D41DE6462"],
    "initial_allowed_destination_addresses": ["0xc2a7e8FEe8fF0B35e752345772C46ba1Db36a6eF"],
    "initial_executors": [{"address": "cosmos1k00qymyetxt36uga8kp88nxyj3hvxfn2v0hgnn"}]
}
```
