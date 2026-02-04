# Deployed Amount Accounting Mismatch for Untracked Adapters

Type: Implementation
Severity: 0 - Informational
Impact: 3 - High
Exploitability: 0 - None
Status: Reported
Reports: Hydro Q1 2026 - Control center & Vault CW contracts (../../Hydro%20Q1%202026%20-%20Control%20center%20&%20Vault%20CW%20contract%202e8d212b18c380dca45cda5aec85b3a6.md)

## Involved artifacts

- [`contracts/inflow/vault`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/vault/src/contract.rs)

## Description

The vault tracks deployed funds differently based on adapter type. For **tracked** adapters, balances are stored in the Control Center's `DEPLOYED_AMOUNT` state variable. For **untracked** adapters, balances are queried dynamically via `query_total_adapter_positions`. When calculating total pool value, both sources are summed. The `withdraw_for_deployment` function always increases `DEPLOYED_AMOUNT`, regardless of where the funds will actually be deployed.

## Problem Scenarios

Whitelisted address calls `withdraw_for_deployment` with 100 tokens. Control Center's `DEPLOYED_AMOUNT` increases by 100. Address deposits the tokens into an **untracked** adapter via `deposit_to_adapter` which results in untracked adapter balance now being 100, queryable via `query_total_adapter_positions`.

When total pool value is calculated:

- `DEPLOYED_AMOUNT` = 100 (from step 2)
- `query_total_adapter_positions()` = 100 (from step 4)
- **Total deployed = 200** (but only 100 tokens exist)

The same tokens are counted twice: once in `DEPLOYED_AMOUNT` and once in the untracked adapter query. This inflates the pool valuation, causing the share price to be artificially high. New depositors receive fewer shares than they should, while existing shareholders benefit unfairly.

## Recommendation

Not so trivial.