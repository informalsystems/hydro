# Update Config Lacks Validation For max_withdrawals_per_user

Type: Implementation
Severity: 0 - Informational
Impact: 1 - Low
Exploitability: 0 - None
Status: Reported
Reports: Hydro Q1 2026 - Control center & Vault CW contracts (../../Hydro%20Q1%202026%20-%20Control%20center%20&%20Vault%20CW%20contract%202e8d212b18c380dca45cda5aec85b3a6.md)

## Involved artifacts

- [`contracts/inflow/vault`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/vault/src/contract.rs)

## Description

The `update_config` entrypoint lets any whitelisted address change `max_withdrawals_per_user` without validating the new value. This value is enforced when a withdrawal is queued, and a user is rejected once their pending count exceeds the configured limit. If the limit is set to `0` (or an unreasonably low value), users cannot enqueue withdrawals when immediate fulfillment is impossible. This creates a simple configuration-based liveness failure for withdrawals.

## Problem scenario

1. A whitelisted operator calls `update_config` and sets `max_withdrawals_per_user = 0`.
2. Liquidity is insufficient to fulfill withdrawals immediately, so withdrawals are queued.
3. Any user attempting to withdraw hits the per-user limit check and their withdrawal reverts.
4. Withdrawals are effectively frozen until the configuration is changed again, creating a liveness failure for users.

## Recommendation

Validate `max_withdrawals_per_user` in `update_config`. A minimal fix is to reject values below `1`. If the protocol expects an upper bound, enforce a reasonable maximum to prevent accidental misconfiguration.