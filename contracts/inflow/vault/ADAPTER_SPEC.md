# Adapter Interface Specification

This document defines the contract that adapters must follow when integrating with the Inflow Vault system.

## Overview

Adapters are external contracts that manage vault funds in various DeFi protocols. The vault delegates fund management to adapters and relies on specific behaviors being correctly implemented.

## Required Interface

Adapters must implement the following query and execute messages as defined in `interface/src/inflow_adapter.rs`.

### Execute Messages

#### Deposit
Accepts tokens from the vault for deployment.

**Request:**
```json
{
  "standard_action": {
    "deposit": {}
  }
}
```
Funds are sent via `info.funds`.

**Contract:**
- MUST accept the full amount sent in the message funds
- MUST NOT charge hidden fees that reduce the tracked position
- MUST be idempotent with respect to accounting
- Only callable by registered depositors

#### Withdraw
Returns tokens to the vault.

**Request:**
```json
{
  "standard_action": {
    "withdraw": {
      "coin": { "denom": "uatom", "amount": "1000000" }
    }
  }
}
```

**Contract:**
- MUST return exactly the requested amount, or revert
- Partial withdrawals are NOT permitted
- Any fees, slippage, or shortfalls MUST cause the transaction to fail
- MUST NOT return less than requested and succeed
- Only callable by registered depositors

### Queries

#### DepositorPosition
Returns the exact value of tokens held for a specific depositor.

**Request:**
```json
{
  "standard_query": {
    "depositor_position": {
      "depositor_address": "neutron1...",
      "denom": "uatom"
    }
  }
}
```

**Response:**
```json
{
  "amount": "1000000"
}
```

**Contract:**
- MUST return the exact current value of tokens held for the depositor
- MUST be denominated in the deposit token (not adapter-internal representations)
- Values MUST reflect current state (staleness tolerance: same block)
- MUST NOT return inflated or deflated values
- If unable to determine position, MUST return error (not zero)

#### AvailableForDeposit
Returns the maximum amount that can be deposited.

**Request:**
```json
{
  "standard_query": {
    "available_for_deposit": {
      "depositor_address": "neutron1...",
      "denom": "uatom"
    }
  }
}
```

**Response:**
```json
{
  "amount": "1000000"
}
```

**Contract:**
- MUST return conservative estimates (never over-report)
- MUST account for protocol deposit caps and other limitations
- If deposits are temporarily disabled, MUST return zero
- If unable to determine availability, MUST return error

#### AvailableForWithdraw
Returns the amount available for immediate withdrawal.

**Request:**
```json
{
  "standard_query": {
    "available_for_withdraw": {
      "depositor_address": "neutron1...",
      "denom": "uatom"
    }
  }
}
```

**Response:**
```json
{
  "amount": "1000000"
}
```

**Contract:**
- MUST return the actual withdrawable amount
- MUST account for any lockups, unbonding periods, or liquidity constraints
- If unable to determine availability, MUST return error

#### TimeToWithdraw
Returns estimated blocks/time required for withdrawal.

**Request:**
```json
{
  "standard_query": {
    "time_to_withdraw": {
      "depositor_address": "neutron1...",
      "coin": { "denom": "uatom", "amount": "1000000" }
    }
  }
}
```

**Response:**
```json
{
  "blocks": 0,
  "seconds": 0
}
```

**Contract:**
- Returns 0 for instant withdrawals (like Mars lending)
- SHOULD reflect actual unbonding periods if applicable

## Trust Assumptions

The vault makes the following trust assumptions about adapters:

1. **Position Accuracy**: The `DepositorPosition` query returns accurate values that correctly reflect the depositor's holdings.

2. **Withdrawal Completeness**: When `Withdraw` succeeds, exactly the requested amount is returned. The adapter never succeeds with a partial withdrawal.

3. **No Hidden Fees**: The adapter does not silently deduct fees that reduce the tracked position without updating the vault.

4. **Query Reliability**: Queries return errors rather than misleading values (like returning zero when unable to determine actual value).

## Error Handling

### Recommended Behavior
- Adapters SHOULD revert with descriptive errors rather than returning zero values
- Adapters SHOULD NOT silently fail or return partial results
- Error messages SHOULD indicate the root cause (insufficient liquidity, protocol paused, etc.)

### Vault Fallback Behavior
The vault silently skips adapters that fail queries. This is a fallback for resilience, not the expected path. Consistently failing adapters will:
- Have their positions excluded from pool value calculations
- Not receive new deposits
- Cause operational blindness for vault operators

Failed adapter queries are reported via response attributes for monitoring.

## Integration Testing

Before registering an adapter, verify:

1. **Position Accuracy**: Query position immediately after deposit/withdraw and verify values match
2. **Withdrawal Completeness**: Attempt withdrawal and verify exact amount returned
3. **Error Propagation**: Simulate failure conditions and verify errors are returned (not zeros)
4. **Availability Accuracy**: Verify reported availability matches actual capacity

### Example Test Scenarios

```rust
// Test 1: Deposit and verify position
adapter.deposit(1000)?;
let position = adapter.depositor_position(vault_addr)?;
assert_eq!(position.amount, 1000);

// Test 2: Withdraw and verify exact amount
let balance_before = bank.balance(vault_addr)?;
adapter.withdraw(Coin { denom: "uatom", amount: 500 })?;
let balance_after = bank.balance(vault_addr)?;
assert_eq!(balance_after - balance_before, 500);

// Test 3: Partial withdrawal fails
adapter.deposit(100)?;
let result = adapter.withdraw(Coin { denom: "uatom", amount: 1000 });
assert!(result.is_err()); // Must fail, not return partial

// Test 4: Query failure returns error
// (simulate protocol pause or network issues)
let result = adapter.available_for_deposit(vault_addr, "uatom");
assert!(result.is_err()); // Error, not zero
```

## Deployment Tracking

Adapters can be configured with two tracking modes:

- **Tracked**: Position is included in `DEPLOYED_AMOUNT` on the control center. Changes to position automatically update the deployed amount.
- **NotTracked**: Position is NOT included in `DEPLOYED_AMOUNT`. Useful for adapters where position is already counted elsewhere or for manual tracking.

When toggling tracking mode, the vault automatically queries the adapter's current position and updates `DEPLOYED_AMOUNT` accordingly to maintain accounting consistency.

## Admin Operations

### RegisterDepositor
Adds a new depositor address that can interact with the adapter.

### UnregisterDepositor
Removes a depositor address. Should only succeed if the depositor has no active position.

### SetDepositorEnabled
Enables or disables a depositor without removing registration.

## Version History

- v1.0 (2026-02): Initial specification based on audit findings
