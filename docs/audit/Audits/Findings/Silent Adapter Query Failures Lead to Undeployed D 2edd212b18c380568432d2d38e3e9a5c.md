# Silent Adapter Query Failures Lead to Undeployed Deposits

Type: Implementation
Severity: 1 - Low
Impact: 1 - Low
Exploitability: 1 - Low
Status: Acknowledged
Reports: Hydro Q1 2026 - Control center & Vault CW contracts (../../Hydro%20Q1%202026%20-%20Control%20center%20&%20Vault%20CW%20contract%202e8d212b18c380dca45cda5aec85b3a6.md)

## Involved artifacts

- [`contracts/inflow/vault`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/vault/src/contract.rs)

## Description

The `calculate_venues_allocation` function determines how deposit funds should be distributed across registered adapters. Before allocating funds, it queries each adapter's available capacity via `AvailableForDeposit`. The function silently ignores adapter query errors, treating them identically to adapters that return zero available capacity. This design choice obscures adapter failures and can cause deposits to remain undeployed in the vault contract when adapters are malfunctioning rather than genuinely at capacity.

## Problem Scenario

Consider the following scenario with three automated adapters:

1. User submits a deposit of 1000 tokens.
2. `calculate_venues_allocation` iterates through the adapters:
    - Adapter A: `AvailableForDeposit` query returns 0 → skipped.
    - Adapter B: Query fails with an error (e.g., contract panic, invalid response) → silently skipped.
    - Adapter C: `AvailableForDeposit` query returns 0 → skipped.
3. The function returns an empty allocation vector `Ok(vec![])`.
4. The deposit succeeds: vault shares are minted, but all 1000 tokens remain idle in the vault contract balance.

The depositor and vault operators have no indication that Adapter B failed. The outcome is indistinguishable from a scenario where all adapters are legitimately at capacity. If Adapter B was the only adapter with available capacity and failed due to a transient or fixable issue, funds that should have been deployed remain idle. This results in reduced yield generation for depositors and operational blindness to adapter health issues.

For withdrawals, the same silent error handling in `calculate_venues_allocation` can cause the vault to underestimate available liquidity, potentially queueing withdrawals unnecessarily when funds actually exist in a failing adapter.

## Recommendation

Modify `calculate_venues_allocation` to track and report adapter query failures. At minimum, emit attributes in the response indicating which adapters failed:

```rust
// Track failed adapters
let mut failed_adapters: Vec<String> = Vec::new();

for (adapter_name, adapter_info) in automated_adapters {
    // ... existing query logic ...

    match available_result {
        Ok(response) if response.amount > Uint128::zero() => {
            // existing allocation logic
        }
        Ok(_) => {
            // Zero capacity - legitimate, no action needed
        }
        Err(_) => {
            failed_adapters.push(adapter_name);
        }
    }
}

// Return failed adapters info alongside allocations

```

Then propagate this information to the caller so it can be included in response attributes:

```rust
.add_attribute("failed_adapter_queries", failed_adapters.join(","))

```

Optionally, consider failing the transaction if all adapters with `Automated` allocation mode fail their queries, as this indicates a systemic issue rather than normal capacity exhaustion.