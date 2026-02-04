# Premature DEPLOYED_AMOUNT Update Creates Accounting Gap

Type: Implementation
Severity: 0 - Informational
Impact: 3 - High
Exploitability: 0 - None
Status: Reported
Reports: Hydro Q1 2026 - Control center & Vault CW contracts (../../Hydro%20Q1%202026%20-%20Control%20center%20&%20Vault%20CW%20contract%202e8d212b18c380dca45cda5aec85b3a6.md)

## Involved artifacts

- [`contracts/inflow/vault`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/vault/src/contract.rs)

## Description

The `withdraw_for_deployment` function sends tokens to a whitelisted address and immediately increases the Control Center's `DEPLOYED_AMOUNT`. However, the tokens are sent to the operator's personal wallet, not directly to an adapter. There is no on-chain enforcement that the tokens will ever be deployed. This creates a gap where `DEPLOYED_AMOUNT` reflects intended deployments rather than actual deployments.

## Problem scenario

1. Whitelisted address calls `withdraw_for_deployment` with 100 tokens.
2. Tokens are sent to the operator's wallet via `BankMsg::Send`.
3. `DEPLOYED_AMOUNT` is increased by 100 in the same transaction.
4. The operator now holds 100 tokens in their personal wallet with no on-chain obligation to deploy them.

During this intermediate state (which may last indefinitely):

- `DEPLOYED_AMOUNT` reports 100 tokens as deployed
- Actual deployed amount is 0
- Pool valuation is inflated by 100 tokens
- Share price is artificially high, diluting new depositors

The operator may deploy partially, deploy to the wrong destination, or never deploy at all. In each case, `DEPLOYED_AMOUNT` does not reflect reality.

## Recommendation

Update `DEPLOYED_AMOUNT` only when tokens are actually deposited into an adapter, not when they leave the vault. Remove the `update_deployed_amount_msg` from `withdraw_for_deployment` and rely on `deposit_to_adapter` to update tracking when tokens reach their destination.