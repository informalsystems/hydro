# Vortex Contract Usage Guide

This guide provides a step-by-step walkthrough for deploying and interacting with the Vortex CosmWasm contract on Osmosis mainnet.

---



### Store contract code

Compile contract to `.wasm`, then upload it to the chain:

```bash
osmosisd tx wasm store liquid_collateral.wasm \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 \
  --gas-prices 0.025uosmo
```
### Instantiate contract


The `InstantiateMsg` defines the configuration parameters when deploying the contract.

| Field                  | Type              | Description |
|------------------------|-------------------|-------------|
| `pool_id`              | `u64`             | Osmosis pool ID where the position will be created. |
| `principal_denom`      | `String`          | Denom of the principal token. |
| `counterparty_denom`   | `String`          | Denom of the counterparty token. |
| `round_duration`       | `u64`             | Duration (in seconds) of a round. |
| `position_admin`       | `Option<String>`  | Optional address with exclusive rights to execute `create_position`. If `None`, anyone may create a position. |
| `counterparty_owner`   | `Option<String>`  | Optional address that will receive the counterparty tokens. If `None`, defaults to: <br>1. `position_admin` if set, otherwise <br>2. the address that entered the position. |
| `principal_funds_owner`| `String`          | Address that will receive the principal tokens. |
| `auction_duration`     | `u64`             | Duration (in seconds) of the auction phase. |
| `principal_first`      | `bool`            | If `true`, the principal token is in the first position in pool. |


The contract assumes pool parameters are valid - there is no validation whether the pool actually exists, etc.

```bash
osmosisd tx wasm instantiate 12426 \
  '{
    "pool_id": 556,
    "principal_denom": "uosmo",
    "counterparty_denom": "ibc/DE6792CF9...",
    "round_duration": 86400,
    "position_admin": "osmo10czsd...",
    "principal_funds_owner": "osmo12at6...",
    "counterparty_owner": "osmo1k00...",
    "auction_duration": 3600,
    "principal_first": false
  }' \
  --label "vortex" \
  --no-admin \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.025uosmo
```

### Query contract state
```bash
osmosisd query wasm contract-state smart osmo1pv0... '{"state": {}}' \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/
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
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.025uosmo
```
### Query position (optionally check on-chain position details using the concentratedliquidity module)
Only used for testing purposes for checking whether contract state aligns with position details.
```bash
osmosisd query concentratedliquidity position-by-id 2806 \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/
```
### Liquidate position
Execution which makes contract partially or fully withdrawing from position if it goes out of range (Principal amount is zero).
```bash
osmosisd tx wasm execute osmo1pv0epte... '{"liquidate": {}}' \
  --amount 100000uosmo \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.025uosmo
```
### End round
Execution which makes contract fully withdrawing from position in case it wasn't fully liquidated beforehand but the round ended. In this case, it can be executed even if the position did not go out of range.
```bash
osmosisd tx wasm execute osmo1pv0eptex4... '{"end_round": {}}' \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.025uosmo
```
### Place bid
Execution which can be performed only in case auction is in progress. (In case end round resulted with needing some principal amount for replenishing).
```bash
osmosisd tx wasm execute osmo18w4389zu... \
  '{"place_bid": {"requested_amount": "1"}}' \
  --amount 5uosmo \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.025uosmo
```
### Swap (only for testing purposes on poolmanager module)
Used only for testing purposes for making position going out of range.
```bash
osmosisd tx poolmanager swap-exact-amount-in 10000uosmo 1 \
  --swap-route-pool-ids 471 \
  --swap-route-denoms ibc/9FF2B... \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.025uosmo
```
### Query bid
```bash
osmosisd query wasm contract-state smart osmo1dwdneu... \
  '{"bid": {"bid_id": 1}}' \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/
```
### Query sorted bids
```bash
osmosisd query wasm contract-state smart osmo1dwdneu... \
  '{"sorted_bids": {}}' \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/
```
### Resolve auction
Can be executed only if the auction is finished.
```bash
osmosisd tx wasm execute osmo1dwdneu... '{"resolve_auction": {}}' \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.025uosmo
```
### Query if liquidatable
Queries if the position is out of range.
```bash
osmosisd query wasm contract-state smart osmo1dwdneu... '{"is_liquidatable": {}}' \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/
```
### Query simulate liquidation
Calculates the counterparty amount which may be received if the position is liquidated. 
```bash
osmosisd query wasm contract-state smart osmo1dwdneu... \
  '{"simulate_liquidation": {"principal_amount": "2"}}' \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/
  ```


# Liquidation Guide for Users
This section explains how to check if a position is eligible for liquidation and how to execute the liquidation. This is useful for users operating a liquidation bot or manually managing positions.

## 1. Check if a Position is Liquidatable
```bash
osmosisd query wasm contract-state smart <contract_address> \
  '{"is_liquidatable": {}}' \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/
 ```

Output:

```json
{
  "data": {
    "liquidatable": true
  }
}
```
If the result is true, the position holds zero principal tokens, indicating that the price has fallen below the lower tick and the position is out of range.

## 2.Simulate Liquidation 
Users can estimate how much of the counterparty asset would be returned if a position is fully or partially liquidated. This operation is tied to the principal_to_replenish state variable.

For example, if principal_to_replenish is 100000, then:

Providing 100000 of principal tokens will simulate a 100% liquidation of the position.

Providing 50000 will simulate a 50% liquidation, and so on.

```bash
osmosisd query wasm contract-state smart <contract_address> \
  '{"simulate_liquidation": {"principal_amount": "100000"}}' \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/
```
Output:

```json
{
  "data": {
    "counterparty_to_receive": "8581"
  }
}
```
This allows users to preview how much counterparty liquidity would be received without committing to a transaction.

Note: This query will only return a value if the position is currently liquidatable. If it is not, the contract will return a message indicating that the position is not liquidatable at the moment.
## 3.Execute Liquidation 
This step performs the actual liquidation, causing the contract to partially or fully withdraw from a position if it has gone out of range (i.e., the position holds zero principal tokens).

To execute the liquidation:

```bash
osmosisd tx wasm execute <contract_address> '{"liquidate": {}}' \
  --amount 100000uosmo \
  --chain-id osmosis-1 \
  --node https://rpc.osmosis.zone/ \
  --from vortex1 \
  --gas auto --gas-adjustment 1.17 --gas-prices 0.025uosmo
```
In this example, 100000uosmo of the principal asset is sent to the contract to perform the liquidation. The amount determines the portion of the position that will be liquidated.

Note:
Steps 1. Check if a Position is Liquidatable and 2. Simulate Liquidation are optional helpers.
Users may skip them and go directly to execution if they:

Already know the position is liquidatable (e.g., querying osmosis chain without contract).

Can calculate expected returns independently (e.g., based on the principal_to_replenish value).

If the action is successfully executed, the contract's position will either be fully withdrawn - leaving no position in the pool — or partially reduced, depending on the amount of principal provided.
