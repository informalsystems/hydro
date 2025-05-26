# Liquid Collateral Contract Usage Guide

This guide provides a step-by-step walkthrough for deploying and interacting with the Liquid Collateral CosmWasm contract on Osmosis testnet.

---



## STORE CONTRACT CODE

Compile your contract to `.wasm`, then upload it to the chain:

```bash
osmosisd tx wasm store liquid_collateral.wasm \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 \
  --gas-prices 0.0025uosmo
```
## INSTANTIATE CONTRACT

```bash
osmosisd tx wasm instantiate 12426 \
  '{
    "pool_id": 556,
    "principal_denom": "uosmo",
    "counterparty_denom": "ibc/DE6792CF9...",
    "round_duration": 86400,
    "project_owner": null,
    "principal_funds_owner": "osmo12at6...",
    "auction_duration": 3600,
    "principal_first": false
  }' \
  --label "vortex" \
  --no-admin \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.0025uosmo
```

## QUERY CONTRACT STATE
```bash
osmosisd query wasm contract-state smart osmo1pv0... '{"state": {}}' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/
```
## ENTER POSITION:
```bash
osmosisd tx wasm execute osmo1pv0ep... \
  '{"create_position": {
    "lower_tick": 5547600,
    "upper_tick": 6536700,
    "principal_token_min_amount": "100000",
    "counterparty_token_min_amount": "11023"
  }}' \
  --amount 11023ibc/DE6792CF9...,100000uosmo \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.0025uosmo
```
QUERY POSITION (optionally check on-chain position details using the concentratedliquidity module)
```bash
osmosisd query concentratedliquidity position-by-id 2806 \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/
```
LIQUIDATE POSITION:
```bash
osmosisd tx wasm execute osmo1pv0epte... '"liquidate"' \
  --amount 100000uosmo \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.0025uosmo
```
## END ROUND:
```bash
osmosisd tx wasm execute osmo1pv0eptex4... '"end_round"' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.0025uosmo
```
## PLACE BID
```bash
osmosisd tx wasm execute osmo18w4389zu... \
  '{"place_bid": {"requested_amount": "1"}}' \
  --amount 5uosmo \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.0025uosmo
```
## SWAP (only for testing purposes on poolmanager module)
```bash
osmosisd tx poolmanager swap-exact-amount-in 10000uosmo 1 \
  --swap-route-pool-ids 471 \
  --swap-route-denoms ibc/9FF2B... \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.0025uosmo
```
## QUERY BID
```bash
osmosisd query wasm contract-state smart osmo1dwdneu... \
  '{"bid": {"bid_id": 1}}' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/
```
## QUERY SORTED BIDS
```bash
osmosisd query wasm contract-state smart osmo1dwdneu... \
  '{"sorted_bids": {}}' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/
```
## RESOLVE AUCTION
```bash
osmosisd tx wasm execute osmo1dwdneu... '"resolve_auction"' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.0025uosmo
```
## QUERY IF LIQUIDATABLE
```bash
osmosisd query wasm contract-state smart osmo1dwdneu... '"is_liquidatable"' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/
```
## QUERY SIMULATE LIQUIDATION
```bash
osmosisd query wasm contract-state smart osmo1dwdneu... \
  '{"simulate_liquidation": {"principal_amount": "2"}}' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/
  ```