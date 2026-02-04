# Adapter Unregistration Without Position Check Causes Fund Loss

Type: Implementation
Severity: 2 - Medium
Impact: 3 - High
Exploitability: 1 - Low
Status: Acknowledged
Reports: Hydro Q1 2026 - Control center & Vault CW contracts (../../Hydro%20Q1%202026%20-%20Control%20center%20&%20Vault%20CW%20contract%202e8d212b18c380dca45cda5aec85b3a6.md)

## Involved artifacts

- [`contracts/inflow/vault`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/vault/src/contract.rs)

## Description

The `unregister_adapter` function removes an adapter from the vault's registry without verifying whether the adapter holds any of the vault's funds. For untracked adapters, this immediately excludes their position from pool value calculations since `query_total_adapter_positions` only iterates over registered adapters. This creates an accounting mismatch where deployed funds are no longer counted, causing share price to drop artificially.

## Problem Scenarios

Assume that adapter "mars_protocol" (untracked) holds 1,000,000 tokens of vault funds and that pool value is 2,000,000 tokens with 2,000 shares issued (share price = 1,000). Whitelisted  user calls `unregister_adapter("mars_protocol")` and adapter is removed from the `ADAPTERS` map with no position check. `query_total_adapter_positions` no longer includes the adapter's 1,000,000 tokens. This leads to the pool value dropping to 1,000,000 tokens (share price = 500). Set of following steps is crucial:

- New depositor deposits 1,000,000 tokens and receives 2,000 shares instead of fair 1,000 shares.
- Whitelisted user, figures out that there has been a mistake re-registers adapter, restoring pool value to 3,000,000 tokens.
- New depositor withdraws 2,000 shares for 1,500,000 tokens, profiting 500,000 tokens at existing holders' expense.

For tracked adapters, the funds remain counted in Control Center's `DEPLOYED_AMOUNT` but become operationally locked since the vault can no longer query or withdraw from the unregistered adapter, until it is registered again.

## Recommendation

Add a position check before allowing adapter unregistration. Query the adapter's position and require it to be zero.