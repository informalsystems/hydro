# Canceling Withdrawals Repriced by Token Info Provider Updates

Type: Implementation
Severity: 1 - Low
Impact: 2 - Medium
Exploitability: 1 - Low
Status: Reported
Reports: Hydro Q1 2026 - Control center & Vault CW contracts (../../Hydro%20Q1%202026%20-%20Control%20center%20&%20Vault%20CW%20contract%202e8d212b18c380dca45cda5aec85b3a6.md)

## Involved artifacts

- [`contracts/inflow/vault`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/vault/src/contract.rs)

## Description

The vault converts deposit tokens into base tokens using a ratio provided by `token_info_provider_contract`. This ratio is used in pool value calculations and in the share minting that happens when users cancel withdrawals. The config function `set_token_info_provider_contract` allows a whitelisted caller to change the ratio source. Changing this source alters valuation without moving any funds. This creates a path where administrative updates can change value accounting during withdrawal cancellation.

## Problem scenario

1. A whitelisted address calls `set_token_info_provider_contract` and points it to a contract that returns a different ratio for the same deposit denom.
2. The vault immediately uses this new ratio for pool value and share calculations without any asset transfer.
3. A user cancels a queued withdrawal; the vault converts the queued deposit amount using the new ratio.
4. The user receives more or fewer shares than they originally burned, even though no funds moved and the queued amount did not change.
5. The outcome is incorrect pool valuation and unfair share accounting, with cancellation outcomes driven purely by a config update.

## Recommendation

Restrict ratio changes from affecting live accounting without explicit value reconciliation. At minimum, require the token info provider to be immutable after initialization or enforce a timelocked update with a mandatory pause window. If updates must remain possible, snapshot the conversion ratio used for each queued withdrawal and for share mint/burn calculations at the time of action, then use the snapshot until the action is finalized.