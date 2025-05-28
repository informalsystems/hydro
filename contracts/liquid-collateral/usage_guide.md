# Liquid Collateral Contract Usage Guide

This guide provides a step-by-step walkthrough for deploying and interacting with the Liquid Collateral CosmWasm contract on Osmosis testnet.

---



### Store contract code

Compile your contract to `.wasm`, then upload it to the chain:

```bash
osmosisd tx wasm store liquid_collateral.wasm \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 \
  --gas-prices 0.0025uosmo
```
### Instantiate contract

The contract assumes pool parameters are valid - there is no validation whether the pool actually exists, etc.

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

### Query contract state
```bash
osmosisd query wasm contract-state smart osmo1pv0... '{"state": {}}' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/
```
### Enter position
Execution which makes contract enter the liquidity position in Osmosis concentrated liquidity pool.
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
### Query position (optionally check on-chain position details using the concentratedliquidity module)
Only used for testing purposes for checking whether contract state aligns with position details.
```bash
osmosisd query concentratedliquidity position-by-id 2806 \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/
```
### Liquidate position
Execution which makes contract partially or fully withdrawing from position if it goes out of range (Principal amount is zero).
```bash
osmosisd tx wasm execute osmo1pv0epte... '"liquidate"' \
  --amount 100000uosmo \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.0025uosmo
```
### End round
Execution which makes contract fully withdrawing from position in case it wasn't fully liquidated beforehand but the Hydro round ended. In this case, it can be executed even if the position did not go out of range.
```bash
osmosisd tx wasm execute osmo1pv0eptex4... '"end_round"' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.0025uosmo
```
### Place bid
Execution which can be performed only in case auction is in progress. (In case end round resulted with needing some principal amount for replenishing).
```bash
osmosisd tx wasm execute osmo18w4389zu... \
  '{"place_bid": {"requested_amount": "1"}}' \
  --amount 5uosmo \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.0025uosmo
```
### Swap (only for testing purposes on poolmanager module)
Used only for testing purposes for making position going out of range.
```bash
osmosisd tx poolmanager swap-exact-amount-in 10000uosmo 1 \
  --swap-route-pool-ids 471 \
  --swap-route-denoms ibc/9FF2B... \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.0025uosmo
```
### Query bid
```bash
osmosisd query wasm contract-state smart osmo1dwdneu... \
  '{"bid": {"bid_id": 1}}' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/
```
### Query sorted bids
```bash
osmosisd query wasm contract-state smart osmo1dwdneu... \
  '{"sorted_bids": {}}' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/
```
### Resolve auction
Can be executed only if the auction is finished.
```bash
osmosisd tx wasm execute osmo1dwdneu... '"resolve_auction"' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.0025uosmo
```
### Query if liquidatable
Queries if the position is out of range. This will mainly be used by liquidation bot. 
```bash
osmosisd query wasm contract-state smart osmo1dwdneu... '"is_liquidatable"' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/
```
### Query simulate liquidation
Calculates the counterparty amount which may be received if the position is liquidated. 
```bash
osmosisd query wasm contract-state smart osmo1dwdneu... \
  '{"simulate_liquidation": {"principal_amount": "2"}}' \
  --chain-id osmo-test-5 \
  --node https://rpc.testnet.osmosis.zone/
  ```
