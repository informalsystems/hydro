# Adapter trust assumptions

Type: Design
Severity: 0 - Informational
Impact: 3 - High
Exploitability: 0 - None
Status: Reported
Reports: Hydro Q1 2026 - Control center & Vault CW contracts (../../Hydro%20Q1%202026%20-%20Control%20center%20&%20Vault%20CW%20contract%202e8d212b18c380dca45cda5aec85b3a6.md)

## Involved artifacts

- [`contracts/inflow/vault`](https://github.com/informalsystems/hydro/blob/62068707e3705258b2bded634406fa6894148ae9/contracts/inflow/vault/src/contract.rs)

## Description

The vault delegates fund management to external adapter contracts under the assumption that adapters are trusted. This trust model is appropriate when adapters are developed and controlled by the team, and the whitelist mechanism prevents untrusted parties from registering arbitrary adapters. However, trust does not preclude accidental misbehavior, implementation bugs, improper error handling, or incorrect calculations within adapter code. The vault's correctness depends on specific adapter behaviors that are currently implicit rather than explicitly documented and enforced.

## Problem scenario

The vault makes several undocumented assumptions about adapter behavior. When these assumptions are violated — even unintentionally — critical vault properties break:

**Assumption 1: Position queries return accurate values**

The vault queries adapters via `DepositorPosition` to calculate total pool value. If an adapter returns an incorrect value (due to a bug, stale cache, or rounding error), the pool value calculation becomes wrong. This affects share pricing for all deposits and withdrawals. Users may receive more or fewer shares than they deserve, enabling value extraction.

**Assumption 2: Withdrawals return the exact requested amount**

When the vault calls `Withdraw` on an adapter, it expects the adapter to return exactly the requested token amount. The vault decrements its internal `deployed_amount` tracking by the requested value without verifying the actual amount received. If an adapter returns less than requested (due to slippage, fees, or bugs), the vault's accounting diverges from reality.

**Assumption 3: Query failures are transient**

The vault silently skips adapters that fail position queries, continuing with partial data. This fail-open design assumes failures are temporary. If an adapter holding significant funds consistently fails queries, the pool value is permanently understated, artificially lowering share prices.

**Assumption 4: Adapters cannot manipulate availability**

Automated allocation queries adapters for `AvailableForDeposit`. A buggy or malicious adapter could report incorrect availability, causing deposits to fail or funds to be deployed to suboptimal strategies.

## Recommendation

Create an explicit Adapter Interface Specification document that defines:

1. **Position Query Contract**: Adapters MUST return the exact value of tokens held for the vault, denominated in the deposit token. Staleness tolerance should be specified (e.g., values must reflect state within N blocks).
2. **Withdrawal Contract**: Adapters MUST return exactly the requested amount or revert. Partial withdrawals are not permitted. Any fees or slippage must be handled internally by the adapter.
3. **Error Handling Contract**: Adapters SHOULD revert with descriptive errors rather than returning zero values. The vault's silent failure handling is a fallback, not the expected path.
4. **Availability Query Contract**: Adapters MUST return conservative availability estimates. Over-reporting available capacity that cannot be fulfilled is a violation.

This specification should be:

- Documented in the repository alongside adapter interface definitions
- Referenced in adapter development guidelines
- Verified during adapter audits before registration
- Enforced through integration tests that simulate misbehaving adapters