# Unrestricted on_behalf_of Withdrawals Can Fill Victim’s Queue Slots

Type: Implementation
Severity: 2 - Medium
Impact: 1 - Low
Exploitability: 3 - High
Status: Acknowledged
Reports: Hydro Q1 2026 - Control center & Vault CW contracts (../../Hydro%20Q1%202026%20-%20Control%20center%20&%20Vault%20CW%20contract%202e8d212b18c380dca45cda5aec85b3a6.md)

## Involved artifacts

- [`contracts/inflow/vault`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/vault/src/contract.rs)

## Description

`Withdraw` accepts an optional `on_behalf_of` address and records the queued withdrawal under that address when it cannot be fulfilled immediately. There is no authorization linking `on_behalf_of` to the caller. Because the per-user withdrawal cap is enforced against the **withdrawer**, any caller can enqueue withdrawals under a victim’s address and exhaust their per-user queue slots.

This enables a targeted nuisance/DoS: a victim with a large intended withdrawal can be prevented from queuing it because their queue is already full of tiny withdrawals submitted by a third party. The attacker must burn their own shares to do this, so it is a griefing-style attack (value-costly), but it still blocks the victim’s workflow until they cancel the queued entries or wait for them to be funded.

## Problem scenario

Assume `max_withdrawals_per_user = 10` and vault liquidity is insufficient to fulfill withdrawals immediately.

1. Alice tries to withdraw a large amount.
2. Bob front‑runs with 10 `Withdraw` calls, each burning 1 share and setting `on_behalf_of = Alice`.
3. Each call queues a withdrawal entry under Alice and increments her pending count.
4. Alice’s withdrawal reverts due to hitting the per‑user queue limit.
5. Alice must cancel the queued entries or wait for funding before she can submit her own withdrawal.

## Recommendation

Consider adding minimum withdraw amount check. This would prevent cheap system griefing.