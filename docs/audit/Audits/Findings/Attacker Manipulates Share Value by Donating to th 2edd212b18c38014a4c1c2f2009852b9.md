# Attacker Manipulates Share Value by Donating to the Vault Contract

Type: Implementation
Severity: 4 - Critical
Impact: 3 - High
Exploitability: 3 - High
Status: Acknowledged
Reports: Hydro Q1 2026 - Control center & Vault CW contracts (../../Hydro%20Q1%202026%20-%20Control%20center%20&%20Vault%20CW%20contract%202e8d212b18c380dca45cda5aec85b3a6.md)

## Involved artifacts

- [`contracts/inflow/vault`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/vault/src/contract.rs)

## Description

The vault calculates share prices based on the total pool value, which includes all tokens held by the contract. An attacker can inflate the share price by depositing a minimal amount and then sending tokens directly to the contract via bank transfer. This artificial price inflation causes subsequent depositors to receive fewer shares than their deposit warrants due to integer rounding. The attacker's share captures value from these rounding losses, enabling profit extraction at the expense of other users.

## Problem Scenario

**Preconditions:**

- The vault has zero existing deposits.
- The attacker can execute transactions before other depositors

**Attack Steps:**

1. Attacker deposits 1 uatom to the empty vault and receives 1 share.
2. Attacker sends 999,999 uatom directly to the vault contract via `BankMsg::Send` (not through the deposit function).
3. Share price inflates from 1 to 1,000,000 uatom per share. This means that now there is a ratio of 1 ATOM per 1 share.
4. Victim 1 deposits 1,500,000 uatom = 1.5 ATOM
    - Expected shares: 1,500,000 × 1 / 1,000,000 = 1.5
    - Actual shares received: 1 (rounded down)
    - Rounding loss: 500,000 uatom = 0.5 ATOM worth of value
5. Victim 2 deposits 1,500,000 uatom = 1.5 ATOM
    - Expected shares: 1,500,000 × 2 / 2,500,000 = 1.2
    - Actual shares received: 1 (rounded down)
    - Rounding loss: 250,000 uatom = 0.25 ATOM worth of value
6. Attacker withdraws by burning 1 share
    - Receives: 1 × 4,000,000 / 3 = 1,333,333 uatom. This is profit of 0.33 ATOM

**Outcome:**

| User | Invested (uatom) | Final Value (uatom) | Profit/Loss (uatom) |
| --- | --- | --- | --- |
| Attacker | 1,000,000 | 1,333,333 | +333,333 |
| Victim 1 | 1,500,000 | 1,333,333 | -166,667 |
| Victim 2 | 1,500,000 | 1,333,333 | -166,667 |

The attacker extracts 0.33 ATOM for 1 ATOM donated by exploiting accumulated rounding losses from multiple victims. 

## Recommendation

Consider thinking about virtual shares concept. Or consider minting some amount of shares to start to admin address in order to avoid low liquidity in a pool.