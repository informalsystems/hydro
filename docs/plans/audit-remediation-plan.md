# Audit Remediation Implementation Plan

This document outlines the implementation plan for addressing findings from the Q1 2026 Control Center & Vault audit.

## Summary of Changes

| # | Task | Finding | Severity | Files Affected |
|---|------|---------|----------|----------------|
| 1 | Pre-mint shares on instantiate | Attacker Manipulates Share Value | Critical | `vault/src/contract.rs`, `vault/src/msg.rs` |
| 2 | Sync DEPLOYED_AMOUNT on tracking toggle | Unconstrained Whitelisted Privileges | Medium | `vault/src/contract.rs` |
| 3 | Block adapter unregistration if position > 0 | Adapter Unregistration Without Position Check | Medium | `vault/src/contract.rs` |
| 4 | Add failed adapter query attributes | Silent Adapter Query Failures | Low | `vault/src/contract.rs` |
| 5 | Add zero shares check | Messages to mint zero shares | Informational | `vault/src/contract.rs` |
| 6 | Add config validation | Update Config Lacks Validation | Informational | `vault/src/contract.rs` |
| 7 | Create adapter interface documentation | Adapter trust assumptions | Informational | `vault/ADAPTER_SPEC.md` |

---

## Task 1: Pre-mint Shares on Instantiate

**Finding:** Attacker Manipulates Share Value by Donating to the Vault Contract (Critical)

**Problem:** An attacker can deposit 1 token, donate tokens directly to the contract, inflate share price, and extract value from subsequent depositors through rounding losses.

**Solution:** Pre-mint 1,000,000 shares during instantiation. The instantiator must send the appropriate amount of collateral (deposit tokens) that, when converted using the token info provider ratio, equals at least 1,000,000 base tokens.

### Collateral Calculation

The shares minted are determined dynamically based on how many tokens the instantiator sends, with a minimum threshold:

1. **Dynamic Minting**: Shares minted = deposit tokens converted to base tokens (1:1 ratio at init)
2. **Minimum Threshold**: Must mint at least 1,000,000 shares (fail otherwise)
3. **Token Info Provider**: Provides the conversion ratio from deposit tokens to base tokens
   - Example: If ratio is 1.2 (1 deposit token = 1.2 base tokens)
   - Sending 1,000,000 deposit tokens → 1,200,000 base tokens → 1,200,000 shares
   - Minimum required: 833,334 deposit tokens → 1,000,000 base tokens → 1,000,000 shares
4. **No Token Info Provider**: If `token_info_provider_contract` is `None`, the deposit token IS the base token (ratio = 1:1)
5. **Validation**: `deposit_amount * ratio >= 1,000,000` (fail if would mint fewer shares)

### Implementation Details

#### 1.1 Modify `InstantiateMsg` in `vault/src/msg.rs`

Add a new field to specify the admin address for pre-minted shares:

```rust
#[cw_serde]
pub struct InstantiateMsg {
    // ... existing fields ...
    /// Address to receive the pre-minted shares. This address will receive
    /// shares to prevent share price manipulation attacks.
    /// The instantiator must send deposit tokens that convert to at least
    /// 1,000,000 base tokens using the token_info_provider ratio.
    pub initial_shares_recipient: String,
}
```

#### 1.2 Modify `instantiate` in `vault/src/contract.rs`

1. Validate that the instantiator sends funds with the message matching `deposit_denom`
2. Query the token info provider to get the conversion ratio (if configured)
3. Convert deposit tokens to base tokens
4. Validate the base token amount >= 1,000,000 (minimum shares threshold)
5. Store the base token amount as the number of shares to mint

```rust
// In instantiate():
const MINIMUM_INITIAL_SHARES: u128 = 1_000_000;

// 1. Require funds to be sent
let initial_deposit = cw_utils::must_pay(&info, &msg.deposit_denom)?;

// 2. Validate initial_shares_recipient
let initial_shares_recipient = deps.api.addr_validate(&msg.initial_shares_recipient)?;

// 3. Convert deposit tokens to base tokens using token info provider
let initial_shares_to_mint = if let Some(ref provider) = token_info_provider_contract {
    // Query the token info provider for the ratio
    let ratio_response: RatioResponse = deps.querier.query_wasm_smart(
        provider.to_string(),
        &TokenInfoProviderQueryMsg::Ratio {
            denom: msg.deposit_denom.clone(),
        },
    )?;
    // deposit_tokens * ratio = base_tokens = shares_to_mint
    initial_deposit.checked_multiply_ratio(
        ratio_response.ratio.numerator(),
        ratio_response.ratio.denominator(),
    )?
} else {
    // No token info provider means deposit token IS the base token (1:1)
    // shares_to_mint = deposit_amount
    initial_deposit
};

// 4. Validate minimum shares threshold
if initial_shares_to_mint < Uint128::new(MINIMUM_INITIAL_SHARES) {
    return Err(new_generic_error(format!(
        "insufficient collateral: sent {} deposit tokens ({} base tokens), \
         need at least {} base tokens to mint minimum {} shares",
        initial_deposit, initial_shares_to_mint, MINIMUM_INITIAL_SHARES, MINIMUM_INITIAL_SHARES
    )));
}

// 5. Store for reply handler
// initial_shares_to_mint = base token value of deposit = number of shares to mint
```

#### 1.3 Modify `ReplyPayload` enum in `vault/src/msg.rs`

```rust
#[derive(Serialize, Deserialize)]
pub enum ReplyPayload {
    CreateDenom {
        subdenom: String,
        metadata: DenomMetadata,
        initial_shares_recipient: String,
        /// The amount of shares to mint (equals base token value of initial deposit)
        initial_shares_amount: Uint128,
    },
}
```

#### 1.4 Modify reply handler

In the `reply` function, after creating the denom:
- Mint shares equal to `initial_shares_amount` (the base token value) to `initial_shares_recipient`
- The deposited funds remain in the contract as backing

```rust
// In reply() after setting denom metadata:
let mint_msg = NeutronMsg::submit_mint_tokens(
    full_denom.denom.clone(),
    initial_shares_amount,  // This is >= 1,000,000 and equals the base token value
    initial_shares_recipient.clone(),
);

Ok(Response::new()
    .add_message(metadata_msg)
    .add_message(mint_msg)
    .add_attribute("action", "reply_create_denom")
    .add_attribute("full_denom", full_denom.denom)
    .add_attribute("initial_shares_minted", initial_shares_amount)
    .add_attribute("initial_shares_recipient", initial_shares_recipient)
)
```

#### 1.5 Share/Collateral Invariant

After instantiation, the following invariant holds:
- `total_shares_issued = initial_shares_to_mint` (dynamic, but guaranteed >= 1,000,000)
- `total_pool_value = initial_shares_to_mint` (same value in base tokens)
- `share_price = total_pool_value / total_shares_issued = 1.0`

Examples:
- Send 1,000,000 deposit tokens (no ratio) → mint 1,000,000 shares
- Send 2,000,000 deposit tokens (no ratio) → mint 2,000,000 shares
- Send 833,334 deposit tokens (ratio 1.2) → mint 1,000,000 shares (minimum)
- Send 1,666,667 deposit tokens (ratio 1.2) → mint 2,000,000 shares

This means:
- Each share is worth exactly 1 base token at initialization
- Subsequent depositors receive shares at fair value
- Rounding attacks are mitigated because the pool has at least 1,000,000 shares

#### 1.6 Update tests

- Update all tests that call `instantiate` to include funds and `initial_shares_recipient`
- Add test: instantiation with exactly 1,000,000 deposit tokens (no ratio) → mints 1,000,000 shares
- Add test: instantiation with 2,000,000 deposit tokens (no ratio) → mints 2,000,000 shares
- Add test: instantiation with token info provider ratio 1.2, send 833,334 tokens → mints 1,000,000 shares
- Add test: instantiation with token info provider ratio 1.2, send 1,666,667 tokens → mints 2,000,000 shares
- Add test: instantiation fails without funds
- Add test: instantiation fails with 999,999 deposit tokens (no ratio) - below minimum
- Add test: instantiation fails with token info provider ratio 1.2, send 833,333 tokens - below minimum after conversion
- Add test: instantiation fails with wrong denom

---

## Task 2: Sync DEPLOYED_AMOUNT on Tracking Toggle

**Finding:** Unconstrained Whitelisted Privileges - Deployment tracking toggle (Medium)

**Problem:** Toggling between `Tracked` and `NotTracked` without synchronizing `DEPLOYED_AMOUNT` creates accounting gaps (double-counting or zero-counting).

**Solution:** When changing tracking mode, query the adapter's current position and update `DEPLOYED_AMOUNT` accordingly.

### Implementation Details

#### 2.1 Modify `set_adapter_deployment_tracking` in `vault/src/contract.rs`

Location: Lines 1225-1250

```rust
fn set_adapter_deployment_tracking(
    deps: DepsMut<NeutronQuery>,
    env: Env,  // Add env parameter
    info: MessageInfo,
    config: &Config,  // Add config parameter
    name: String,
    deployment_tracking: DeploymentTracking,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    let mut adapter_info = ADAPTERS
        .may_load(deps.storage, name.clone())?
        .ok_or_else(|| ContractError::AdapterNotFound { name: name.clone() })?;

    let old_tracking = adapter_info.deployment_tracking.clone();

    // Only proceed if tracking mode is actually changing
    if old_tracking == deployment_tracking {
        return Ok(Response::new()
            .add_attribute("action", "set_adapter_deployment_tracking")
            .add_attribute("sender", info.sender)
            .add_attribute("adapter_name", name)
            .add_attribute("result", "no_change"));
    }

    // Query current adapter position
    let adapter_position = query_single_adapter_position(
        &deps.as_ref(),
        &env,
        &adapter_info,
        &config.deposit_denom,
    )?;

    let mut messages = vec![];

    match (&old_tracking, &deployment_tracking) {
        // Tracked -> NotTracked: Subtract from DEPLOYED_AMOUNT
        (DeploymentTracking::Tracked, DeploymentTracking::NotTracked) => {
            if !adapter_position.is_zero() {
                let update_msg = build_update_deployed_amount_msg(
                    config,
                    adapter_position,
                    DeploymentDirection::Subtract,
                )?;
                messages.push(update_msg);
            }
        }
        // NotTracked -> Tracked: Add to DEPLOYED_AMOUNT
        (DeploymentTracking::NotTracked, DeploymentTracking::Tracked) => {
            if !adapter_position.is_zero() {
                let update_msg = build_update_deployed_amount_msg(
                    config,
                    adapter_position,
                    DeploymentDirection::Add,
                )?;
                messages.push(update_msg);
            }
        }
        _ => {} // Same tracking mode, shouldn't reach here
    }

    // Update deployment tracking
    adapter_info.deployment_tracking = deployment_tracking.clone();
    ADAPTERS.save(deps.storage, name.clone(), &adapter_info)?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("action", "set_adapter_deployment_tracking")
        .add_attribute("sender", info.sender)
        .add_attribute("adapter_name", name)
        .add_attribute("deployment_tracking", format!("{:?}", deployment_tracking))
        .add_attribute("synced_amount", adapter_position))
}
```

#### 2.2 Add helper function to query single adapter position

```rust
/// Query position for a single adapter
fn query_single_adapter_position(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    adapter_info: &AdapterInfo,
    deposit_denom: &str,
) -> Result<Uint128, ContractError> {
    let query_msg = AdapterInterfaceQueryMsg::DepositorPosition {
        depositor_address: env.contract.address.to_string(),
        denom: deposit_denom.to_string(),
    };

    let result: Result<DepositorPositionResponse, _> = deps.querier.query_wasm_smart(
        adapter_info.address.to_string(),
        &AdapterInterfaceQuery {
            standard_query: &query_msg,
        },
    );

    match result {
        Ok(response) => Ok(response.amount),
        Err(e) => Err(new_generic_error(format!(
            "failed to query adapter position: {}",
            e
        ))),
    }
}
```

#### 2.3 Update execute match arm

The execute match for `SetAdapterDeploymentTracking` needs to pass `env` and `config`:

```rust
ExecuteMsg::SetAdapterDeploymentTracking {
    name,
    deployment_tracking,
} => set_adapter_deployment_tracking(deps, env, info, &config, name, deployment_tracking),
```

#### 2.4 Update tests

- Add test: toggle Tracked -> NotTracked subtracts from DEPLOYED_AMOUNT
- Add test: toggle NotTracked -> Tracked adds to DEPLOYED_AMOUNT
- Add test: toggle with zero position doesn't send message
- Add test: toggle to same mode is no-op

---

## Task 3: Block Adapter Unregistration if Position > 0

**Finding:** Adapter Unregistration Without Position Check Causes Fund Loss (Medium)

**Problem:** Unregistering an adapter with funds causes those funds to vanish from pool value calculations.

**Solution:** Query the adapter's position before unregistration and reject if position > 0.

### Implementation Details

#### 3.1 Modify `unregister_adapter` in `vault/src/contract.rs`

Location: Lines 1167-1188

```rust
fn unregister_adapter(
    deps: DepsMut<NeutronQuery>,
    env: Env,  // Add env parameter
    info: MessageInfo,
    config: &Config,  // Add config parameter
    name: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    let adapter_info = ADAPTERS
        .may_load(deps.storage, name.clone())?
        .ok_or_else(|| ContractError::AdapterNotFound { name: name.clone() })?;

    // Query adapter position - must be zero to unregister
    let adapter_position = query_single_adapter_position(
        &deps.as_ref(),
        &env,
        &adapter_info,
        &config.deposit_denom,
    )?;

    if !adapter_position.is_zero() {
        return Err(new_generic_error(format!(
            "cannot unregister adapter '{}' with non-zero position: {}",
            name, adapter_position
        )));
    }

    // Remove adapter from ADAPTERS map
    ADAPTERS.remove(deps.storage, name.clone());

    Ok(Response::new()
        .add_attribute("action", "unregister_adapter")
        .add_attribute("sender", info.sender)
        .add_attribute("adapter_name", name)
        .add_attribute("adapter_address", adapter_info.address))
}
```

#### 3.2 Update execute match arm

```rust
ExecuteMsg::UnregisterAdapter { name } => {
    unregister_adapter(deps, env, info, &config, name)
}
```

#### 3.3 Add new error variant (optional, or use generic error)

In `vault/src/error.rs`:

```rust
#[error("Cannot unregister adapter '{name}' with non-zero position: {position}")]
AdapterHasPosition { name: String, position: Uint128 },
```

#### 3.4 Update tests

- Update existing `unregister_adapter_success` test to ensure position is zero
- Add test: unregister fails when adapter has funds
- Add test: unregister succeeds after withdrawing all funds from adapter

---

## Task 4: Add Failed Adapter Query Attributes

**Finding:** Silent Adapter Query Failures Lead to Undeployed Deposits (Low)

**Problem:** When adapter queries fail silently, operators have no visibility into which adapters are malfunctioning.

**Solution:** Track failed adapter queries and include them as response attributes. Do NOT fail the transaction.

### Implementation Details

#### 4.1 Modify `calculate_venues_allocation` return type

Location: Lines 1526-1590

Change the function to return both allocations and failed adapters:

```rust
struct AllocationResult {
    allocations: Vec<(String, Uint128)>,
    failed_adapters: Vec<String>,
}

fn calculate_venues_allocation(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    amount: Uint128,
    denom: String,
    is_deposit: bool,
) -> Result<AllocationResult, ContractError> {
    let inflow_address = env.contract.address.to_string();
    let mut allocations: Vec<(String, Uint128)> = Vec::new();
    let mut failed_adapters: Vec<String> = Vec::new();
    let mut remaining = amount;

    // ... existing adapter iteration logic ...

    for (adapter_name, adapter_info) in automated_adapters {
        if remaining.is_zero() {
            break;
        }

        let query_msg = if is_deposit {
            AdapterInterfaceQueryMsg::AvailableForDeposit { ... }
        } else {
            AdapterInterfaceQueryMsg::AvailableForWithdraw { ... }
        };

        let available_result: Result<AvailableAmountResponse, _> = deps.querier.query_wasm_smart(
            adapter_info.address.to_string(),
            &AdapterInterfaceQuery { standard_query: &query_msg },
        );

        match available_result {
            Ok(available_response) if available_response.amount > Uint128::zero() => {
                let to_allocate = available_response.amount.min(remaining);
                allocations.push((adapter_name, to_allocate));
                remaining = remaining.checked_sub(to_allocate)?;
            }
            Ok(_) => {
                // Zero capacity - legitimate, no action needed
            }
            Err(_) => {
                // Query failed - track for visibility
                failed_adapters.push(adapter_name);
            }
        }
    }

    Ok(AllocationResult {
        allocations,
        failed_adapters,
    })
}
```

#### 4.2 Update callers to add attributes

In `deposit()` and `withdraw()` functions:

```rust
let allocation_result = calculate_venues_allocation(...)?;

let mut response = Response::new()
    // ... existing attributes ...
    ;

if !allocation_result.failed_adapters.is_empty() {
    response = response.add_attribute(
        "failed_adapter_queries",
        allocation_result.failed_adapters.join(","),
    );
}
```

#### 4.3 Similarly update `query_total_adapter_positions`

For the pool info query, track and report failed adapters:

```rust
fn query_total_adapter_positions(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    deposit_denom: String,
) -> StdResult<(Uint128, Vec<String>)> {  // Return tuple with failed adapters
    // ... existing logic ...
    let mut failed_adapters: Vec<String> = Vec::new();

    for (name, adapter_info) in adapters {
        // ...
        if let Err(_) = result {
            failed_adapters.push(name);
        }
    }

    Ok((total_positions, failed_adapters))
}
```

#### 4.4 Update tests

- Add test: deposit with one failing adapter includes attribute
- Add test: deposit with all adapters failing includes all names
- Add test: successful queries don't add attribute

---

## Task 5: Add Zero Shares Check

**Finding:** Messages to mint zero shares will reach tokenfactory instead of erroring in the contract (Informational)

**Problem:** If share calculation results in zero, the mint message reaches the chain and fails with an unclear error.

**Solution:** Add explicit check before constructing mint messages.

### Implementation Details

#### 5.1 Add check in `deposit()` function

Location: After `calculate_number_of_shares_to_mint` call (~line 257)

```rust
let vault_shares_to_mint = calculate_number_of_shares_to_mint(
    deposit_amount_base_tokens,
    total_pool_value,
    total_shares_issued,
)?;

// Prevent minting zero shares
if vault_shares_to_mint.is_zero() {
    return Err(new_generic_error(
        "deposit amount too small: would mint zero shares"
    ));
}
```

#### 5.2 Add check in `cancel_withdrawal()` function

Location: After shares calculation in the cancellation loop (~line 640)

```rust
let shares_to_mint = calculate_number_of_shares_to_mint(
    amount_to_withdraw_base_tokens,
    total_pool_value,
    total_shares_issued,
)?;

if shares_to_mint.is_zero() {
    return Err(new_generic_error(
        "cannot cancel withdrawal: would mint zero shares"
    ));
}
```

#### 5.3 Update tests

- Add test: deposit with tiny amount fails with clear error
- Add test: cancel_withdrawal with conditions that would mint zero shares fails

---

## Task 6: Add Config Validation

**Finding:** Update Config Lacks Validation For max_withdrawals_per_user (Informational)

**Problem:** Setting `max_withdrawals_per_user = 0` effectively freezes withdrawals.

**Solution:** Validate that `max_withdrawals_per_user >= 1`.

### Implementation Details

#### 6.1 Modify `update_config()` function

Location: Lines 1594-1619

```rust
fn update_config(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    mut current_config: Config,
    config_update: UpdateConfigData,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    let mut response = Response::new()
        .add_attribute("action", "update_config")
        .add_attribute("sender", info.sender);

    if let Some(max_withdrawals_per_user) = config_update.max_withdrawals_per_user {
        // Validate: must be at least 1
        if max_withdrawals_per_user < 1 {
            return Err(new_generic_error(
                "max_withdrawals_per_user must be at least 1"
            ));
        }

        current_config.max_withdrawals_per_user = max_withdrawals_per_user;
        response = response.add_attribute(
            "max_withdrawals_per_user",
            max_withdrawals_per_user.to_string(),
        );
    }

    CONFIG.save(deps.storage, &current_config)?;

    Ok(response)
}
```

#### 6.2 Also validate in `instantiate()`

```rust
if msg.max_withdrawals_per_user < 1 {
    return Err(new_generic_error(
        "max_withdrawals_per_user must be at least 1"
    ));
}
```

#### 6.3 Update tests

- Add test: update_config with max_withdrawals_per_user = 0 fails
- Add test: instantiate with max_withdrawals_per_user = 0 fails

---

## Task 7: Create Adapter Interface Documentation

**Finding:** Adapter trust assumptions (Informational)

**Problem:** Trust assumptions about adapter behavior are implicit rather than documented.

**Solution:** Create explicit Adapter Interface Specification document.

### Implementation Details

Create file: `contracts/inflow/vault/ADAPTER_SPEC.md`

```markdown
# Adapter Interface Specification

This document defines the contract that adapters must follow when integrating with the Inflow Vault system.

## Overview

Adapters are external contracts that manage vault funds in various DeFi protocols. The vault delegates fund management to adapters and relies on specific behaviors being correctly implemented.

## Required Interface

Adapters must implement the following query and execute messages as defined in `interface/src/adapter_interface.rs`.

### Queries

#### DepositorPosition
Returns the exact value of tokens held for a specific depositor.

**Request:**
```json
{
  "depositor_position": {
    "depositor_address": "neutron1...",
    "denom": "uatom"
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
Returns the amount available for new deposits.

**Contract:**
- MUST return conservative estimates (never over-report)
- If deposits are temporarily disabled, MUST return zero
- If unable to determine availability, SHOULD return error

#### AvailableForWithdraw
Returns the amount available for withdrawal.

**Contract:**
- MUST return the actual withdrawable amount
- MUST account for any lockups, unbonding periods, or liquidity constraints
- If unable to determine availability, SHOULD return error

### Execute Messages

#### Deposit
Accepts tokens from the vault for deployment.

**Contract:**
- MUST accept the full amount sent in the message funds
- MUST NOT charge hidden fees that reduce the tracked position
- MUST be idempotent with respect to accounting

#### Withdraw
Returns tokens to the vault.

**Contract:**
- MUST return exactly the requested amount, or revert
- Partial withdrawals are NOT permitted
- Any fees, slippage, or shortfalls MUST cause the transaction to fail
- MUST NOT return less than requested and succeed

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

## Integration Testing

Before registering an adapter, verify:

1. **Position Accuracy**: Query position immediately after deposit/withdraw and verify values match
2. **Withdrawal Completeness**: Attempt withdrawal and verify exact amount returned
3. **Error Propagation**: Simulate failure conditions and verify errors are returned (not zeros)
4. **Availability Accuracy**: Verify reported availability matches actual capacity

## Version History

- v1.0 (2026-02): Initial specification based on audit findings
```

---

## Implementation Order

Recommended order of implementation:

1. **Task 6: Config validation** - Simple, low risk
2. **Task 5: Zero shares check** - Simple, low risk
3. **Task 7: Documentation** - No code changes to contract
4. **Task 4: Failed adapter attributes** - Moderate complexity
5. **Task 3: Adapter unregistration check** - Moderate complexity
6. **Task 2: DEPLOYED_AMOUNT sync** - Higher complexity, affects accounting
7. **Task 1: Pre-mint shares** - Highest complexity, affects instantiation

---

## Testing Strategy

For each task:
1. Write unit tests first (TDD approach)
2. Run `make test-unit` after each change
3. Run `make clippy` to catch issues early
4. Update schemas if message types change (`make schema`)

After all tasks:
1. Run full test suite: `make test-unit`
2. Run linter: `make clippy`
3. Regenerate schemas: `make schema`
4. Compile contracts: `make compile`

---

## Migration Considerations

### Task 1 (Pre-mint shares)
- **New deployments only**: This change affects `instantiate`, not `migrate`
- Existing vaults are not affected
- New vaults will require funds at instantiation

### Tasks 2-6 (Contract logic changes)
- These can be deployed via migration
- No state migration required
- Behavior changes are backward compatible

### Task 7 (Documentation)
- No migration needed
