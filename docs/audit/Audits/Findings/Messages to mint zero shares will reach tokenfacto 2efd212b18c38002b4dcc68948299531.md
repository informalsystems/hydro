# Messages to mint zero shares will reach tokenfactory instead of erroring in the contract

Type: Implementation
Severity: 0 - Informational
Impact: 1 - Low
Exploitability: 0 - None
Status: Reported
Reports: Hydro Q1 2026 - Control center & Vault CW contracts (../../Hydro%20Q1%202026%20-%20Control%20center%20&%20Vault%20CW%20contract%202e8d212b18c380dca45cda5aec85b3a6.md)

## Involved artifacts

- [`contracts/inflow/vault`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/vault/src/contract.rs)

## Description

The vault contract mints shares to users during deposits and when canceling withdrawals. The `deposit` and `cancel_withdrawal` functions calculate the number of shares to mint using `calculate_number_of_shares_to_mint` and then construct a mint message. However, the contract does not verify that the computed share amount is non-zero before submitting the mint message to the chain.

## Problem Scenarios

1. A user calls `deposit` with a very small amount or under specific pool value conditions where `calculate_number_of_shares_to_mint` returns zero.
2. The contract constructs `NeutronMsg::submit_mint_tokens` with zero amount.
3. The message is sent to the Neutron token factory module.
4. The chain rejects the zero-amount mint operation, causing the transaction to fail.
5. The user pays gas for the failed transaction, and the failure reason may be unclear since it originates from the chain module rather than the contract.

## Recommendation

Add an explicit check for zero shares before constructing the mint message. Return an early error with a descriptive message:

```rust
if vault_shares_to_mint.is_zero() {
    return Err(new_generic_error("cannot mint zero shares"));
}
```

Apply this check in both `deposit` and `cancel_withdrawal`.