# Withdrawal Cancellation Allows Share Inflation

Type: Implementation
Severity: 4 - Critical
Impact: 3 - High
Exploitability: 3 - High
Status: Acknowledged
Reports: Hydro Q1 2026 - Control center & Vault CW contracts (../../Hydro%20Q1%202026%20-%20Control%20center%20&%20Vault%20CW%20contract%202e8d212b18c380dca45cda5aec85b3a6.md)

## Involved artifacts

- [`contracts/inflow/vault`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/vault/src/contract.rs)

## Description

The vault uses a withdrawal queue mechanism where shares are burned immediately upon withdrawal request, and the `amount_to_receive` is locked at the current share price. When a user cancels a pending withdrawal, the `cancel_withdrawal` function recalculates shares to mint using the locked `amount_to_receive` against the current pool value via `calculate_number_of_shares_to_mint`. This recalculation uses the formula `shares = amount * total_shares / (total_pool_value - amount)`. If the pool value has decreased since the withdrawal was queued, the locked amount represents a larger fraction of the diminished pool, resulting in more shares being minted than were originally burned.

## Problem Scenarios

Let’s take the following scenario:

1. Attacker holds 500 shares (50%) when total shares are 1000 and pool value is 1000 tokens.
2. Vault funds are deployed to an adapter, leaving vault balance at 0.
3. Attacker queues withdrawal of 500 shares. The contract burns the shares and locks `amount_to_receive = 500` tokens. Pool value becomes 500 (= 0 + 1000 deployed - 500 queued), and remaining shares are 500.
4. A loss event occurs (e.g., slashing, bad debt in the adapter). Deployed amount drops from 1000 to 800, reducing pool value to 300 (= 0 + 800 - 500).
5. Attacker calls `CancelWithdrawal`. The contract:
    - Reads locked `amount_to_receive = 500` from the withdrawal entry
    - Adjusts pool value: 300 + 500 = 800
    - Calculates new shares: `500 * 500 / (800 - 500) = 833`
6. Attacker receives 833 shares instead of the original 500 shares burned.
7. Final state: Attacker holds 833/1333 shares worth ~500 tokens (avoided the loss). Victim holds 500/1333 shares worth ~300 tokens (absorbed the entire 200 token loss).

The attacker can also exploit this by monitoring for pending loss events (governance proposals to update deployed amounts, observable protocol issues) and timing withdrawals accordingly. The asymmetry also allows selective cancellation: users cancel after losses (gaining shares) but keep pending withdrawals after gains.

## Recommendation

When canceling a withdrawal, mint back the exact number of shares that were originally burned (`shares_burned` from the withdrawal entry) instead of recalculating based on the locked amount and current pool value.

```rust
// In cancel_withdrawal, replace:
let shares_to_mint = calculate_number_of_shares_to_mint(
    amount_to_withdraw_base_tokens,
    total_pool_value,
    total_shares_issued,
)?;

// With:
let shares_to_mint = shares_burned;

```

This ensures users cannot gain or lose shares through the cancel mechanism, maintaining fair loss/gain distribution among all shareholders.