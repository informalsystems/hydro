# Unconstrained Whitelisted Privileges

Type: Design
Severity: 2 - Medium
Impact: 3 - High
Exploitability: 1 - Low
Status: Reported
Reports: Hydro Q1 2026 - Control center & Vault CW contracts (../../Hydro%20Q1%202026%20-%20Control%20center%20&%20Vault%20CW%20contract%202e8d212b18c380dca45cda5aec85b3a6.md)

## Involved artifacts

- [`contracts/inflow/vault`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/vault/src/contract.rs)

## Description

The vault contract implements a whitelist-based access control model where whitelisted addresses have extensive privileged capabilities including: extracting funds for deployment, setting external oracle contracts, registering arbitrary adapter contracts, toggling adapter accounting modes, and managing whitelist membership. The contract assumes whitelisted users act honestly without enforcing these assumptions through code. A single compromised or malicious whitelisted address can exploit multiple attack vectors to extract funds from the vault, manipulate share prices, or permanently lock depositor funds.

## Problem Scenarios

### Token Info Provider Manipulation for Share Price Arbitrage

The `set_token_info_provider_contract` function allows any whitelisted user to point the vault to an arbitrary contract for exchange rate queries. This rate affects all share calculations.

A whitelisted user can deploy a malicious token info provider returning an inflated ratio—say 2.0 instead of 1.0. A collaborator depositing 1,000,000 tokens will receive shares as if they deposited 2,000,000. Once the collaborator has their inflated shares, the whitelisted user resets the provider to the legitimate contract. The collaborator withdraws at fair share price, extracting value from other shareholders.

This manipulation also affects `cancel_withdrawal`, where shares minted back use the current ratio rather than the ratio at withdrawal time. A user can withdraw when the ratio is low, locking in a high token amount, then cancel after the ratio increases to receive more shares than originally burned.

### Malicious Adapter Registration for Direct Fund Theft

The `register_adapter` function accepts any contract address as an adapter without validation. A whitelisted user can deploy a malicious contract that accepts deposits but ignores withdrawal requests.

By registering this contract with `AllocationMode::Automated`, user deposits automatically flow to the malicious adapter. The vault sends funds, and the malicious adapter keeps them. When the vault queries the adapter's position, it returns zero or fails, causing pool value to collapse. For targeted theft, registering with `AllocationMode::Manual` allows using `deposit_to_adapter` to selectively move vault funds to the malicious contract.

### Deployment Tracking Toggle for Double-Counting or Zero-Counting

The `set_adapter_deployment_tracking` function allows toggling between `Tracked` and `NotTracked` without synchronizing the Control Center's `DEPLOYED_AMOUNT`, creating accounting gaps.

For inflation: an adapter registered as `Tracked` has its position in `DEPLOYED_AMOUNT`. Toggling to `NotTracked` causes the vault to also query the adapter directly. The same funds are now counted twice—once via query, once in deployed amount—inflating pool value and allowing withdrawals at artificially high prices.

For deflation: an adapter registered as `NotTracked` is queried directly. Toggling to `Tracked` skips the query, but if `DEPLOYED_AMOUNT` was never updated, the funds vanish from accounting. Pool value drops, allowing a collaborator to deposit at depressed prices and receive excess shares.

### withdraw_for_deployment as Unrestricted Fund Extraction

The `withdraw_for_deployment` function sends vault funds directly to the whitelisted caller's wallet while incrementing `DEPLOYED_AMOUNT`. There is no mechanism requiring the user to return these funds.

A whitelisted user can extract the entire available balance, receive tokens in their personal wallet, and never call `deposit_from_deployment`. The pool value continues to include these phantom "deployed" funds, so share price remains stable despite the vault being empty. New depositors buy shares backed by nothing. When withdrawals exceed available balance, users discover they cannot withdraw.

### Whitelist Takeover via Unconstrained Member Management

The `add_to_whitelist` and `remove_from_whitelist` functions allow any single whitelisted user to add or remove other addresses. The only protection is that the last address cannot be removed.

If any whitelisted address is compromised, the attacker immediately adds multiple attacker-controlled addresses, then removes all legitimate administrators. With their addresses secured, the check preventing removal of the last address never triggers. Legitimate administrators are locked out, and the attacker has complete vault control to execute any other attack. There is no timelock, multi-signature requirement, or governance delay.

## Recommendation

Consider implementing defense-in-depth controls for whitelisted operations:

**Token Info Provider**: Require governance or timelock for changes. Add sanity bounds on ratio values. Cache the ratio at withdrawal time to prevent cancel arbitrage.

**Adapter Registration**: Implement an adapter allowlist or require governance approval. Require adapters implement a verification interface proving legitimacy.

**Deployment Tracking Toggle**: When changing tracking mode, require synchronizing `DEPLOYED_AMOUNT` in the same transaction to prevent accounting gaps.

**withdraw_for_deployment**: Implement limits, timelocks, or multi-signature authorization. Consider escrow rather than direct transfer. Add deadlines for returning funds.

**Whitelist Management**: Require multiple signatures for changes. Implement timelock for removals. Consider governance integration.

**Adapter Query Failures**: Change to fail-closed—revert if any adapter fails rather than silently excluding positions.

**General**: Consider role-based access control with different privilege levels rather than a single all-powerful whitelist.