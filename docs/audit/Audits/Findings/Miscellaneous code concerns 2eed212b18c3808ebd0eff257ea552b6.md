# Miscellaneous code concerns

Type: Implementation
Severity: 0 - Informational
Impact: 0 - None
Exploitability: 0 - None
Status: Reported
Reports: Hydro Q1 2026 - Control center & Vault CW contracts (../../Hydro%20Q1%202026%20-%20Control%20center%20&%20Vault%20CW%20contract%202e8d212b18c380dca45cda5aec85b3a6.md)

## Involved artifacts

- [`contracts/inflow/vault`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/vault/src/contract.rs)
- [`contracts/inflow/control-center`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/control-center/src/contract.rs)

## Problem list

- The `query_shares_issued` function loads the entire config from storage to retrieve `vault_shares_denom`. This function is called from `get_pool_info`, which already has the config available as a parameter. This results in an unnecessary storage read.
- The `update_config` function allows whitelisted addresses to modify the vault configuration. Currently, the only configurable parameter is `max_withdrawals_per_user`. The function unconditionally emits an `update_config` event and writes to storage, regardless of whether any configuration value actually changed.
- The `query_total_adapter_positions` function calculates the total value held across all untracked adapters. It first collects all adapters into a vector, then iterates through them and skips adapters with `DeploymentTracking::Tracked` status. This results in unnecessary collection and iteration over adapters that will be discarded.