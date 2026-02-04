# Threat Model

## Trust Assumptions

### Trusted

**Whitelisted addresses**

- **Can Do:** Manage adapters, Withdraw funds for deployment, Update config, Modify whitelist
- **Cannot Do:** Bypass access control, Directly drain user shares
- **Risk if Compromised:** Critical — Can drain all available funds via `withdraw_for_deployment`, can gain control of the whitelisted users set

**Control Center**

- **Can Do:** Provide pool value aggregates, Track deployed amounts, Set deposit caps
- **Cannot Do:** Access vault funds, Manipulate vault state directly
- **Risk if Compromised:** High — Incorrect values break share price calculations

### Semi-trusted

**Token Info Provider**

- **Can Do:** Provide token conversion rates
- **Cannot Do:** Access vault funds
- **Risk if Compromised:** Medium — Rate manipulation affects share price

**Adapters**

- **Can Do:** Hold vault funds, Report positions, Process deposits/withdrawals
- **Cannot Do:** Access other adapter funds, Access vault contract balance
- **Risk if Compromised:** Medium-High — Can misreport positions or fail; isolated by design

### Untrusted

**Regular Users**

- **Can Do:** Deposit funds, Withdraw own shares, Cancel own withdrawals
- **Cannot Do:** Access other users' withdrawals, Bypass queue FIFO, Manipulate share price directly
- **Risk if Compromised:** Low — Limited to own actions

## Share Accounting Properties

### VP-001: First Depositor Share Ratio

When the vault has zero shares issued or zero balance, the first deposit receives shares at a 1:1 ratio with deposited base tokens:

```
If (total_shares_issued == 0 OR deposit_token_current_balance == 0):
  shares_minted = deposit_amount_base_tokens
```

The first depositor sets the initial share price. If this ratio is not 1:1, it could enable share price manipulation. The 1:1 ratio ensures a fair starting point and prevents the first depositor from gaining an unfair advantage.

### Threats

**T-1: First Deposit Share Inflation Attack**

- Direct token transfer to contract address to inflate balance without minting shares. Attacker deposits 1uatom, receives 1 share. Then donates large amount directly to contract (bypassing deposit function). Next depositor receives very few shares due to inflated share price.
- **Impact**: Second and subsequent depositors lose value; first depositor can withdraw disproportionate amount
- **Conclusion**: This is a valid threat, finding is written.

**T-2: Rounding Errors in First Deposit**

- If first deposit amount is very small, rounding in subsequent calculations could cause share value divergence
- **Impact**: Small rounding errors compound over time, causing share value inaccuracy
- **Conclusion**: This is a valid threat, finding is written.

**T-3: Zero Deposit Accepted**

- If `deposit_amount` is zero, shares would be zero, creating invalid state
- **Impact**: Contract state corruption, potential for zero-division errors
- **Conclusion**: This threat does not stand, deposit works if only one denom and non-zero amount is sent and it errors otherwise.

---

### VP-002: Share Minting Calculation Correctness

For non-first deposits, shares minted must maintain proportional ownership:

```
shares_minted = (deposit_amount_base * total_shares_issued) / (total_pool_value - deposit_amount_base)
```

Where `total_pool_value` includes the just-deposited amount (already in contract balance via must_pay).

The formula ensures new depositors receive shares proportional to their contribution relative to existing pool value. The deposit amount is subtracted from total_pool_value in the denominator because it's already included in the contract balance when execute() is called.

This maintains the invariant that each share represents an equal fractional ownership of the pool.

### Threats

**T-1: Incorrect Pool Value Calculation**

- Manipulation of pool value calculation through adapter position misreporting. If `total_pool_value` doesn't include deposit amount, or includes it incorrectly, share calculation will be wrong
- **Impact**: New depositors receive incorrect number of shares, violating proportionality
- **Conclusion:** The threat is not valid, `total_pool_value` will include value sent to the contract because it is gotten by querying the balance of the contract.

**T-2: Integer Overflow in Multiplication**

- `deposit_amount_base * total_shares_issued` could overflow for large values
- **Impact**: Transaction reverts instead of minting shares, DoS for large deposits
- **Conclusion:** This is possible to happen, however since the `total_shares_issued` will be decreased when shares are burned, there will be no permanent DoS. Even if at one point  the product of those two numbers would have to be larger than `340282366920938463463374607431768211455`.

**T-3: Division by Zero**

- If `total_pool_value - deposit_amount_base == 0`, division fails
- **Impact**: Transaction reverts, but this should be caught by first deposit check
- Conclusion: The threat does not stand. Check prevents it.

**T-4: Rounding Direction Bias**

- Integer division always rounds down, potentially favoring vault or user depending on order
- **Impact**: Cumulative rounding errors could allow value extraction over many deposits
- **Conclusion**: The threat does not stand. Rounding is done in favor of protocol. Value will not be extracted.

---

### VP-003: Withdrawal Amount Calculation Correctness

The amount withdrawn must be proportional to shares burned:

```
amount_to_withdraw = (shares_burned * total_pool_value) / total_shares_supply
```

This formula must be inversely consistent with the deposit formula.

When users withdraw, they should receive tokens proportional to their share ownership. The calculation must mirror the deposit calculation to maintain fair value exchange.

The `total_pool_value` used must be calculated identically to deposits to ensure consistency.

### Threats

**T-1: Stale Pool Value**

- Withdraw immediately after adapter loss before pool value updates. If pool value is calculated before adapter positions are updated, withdrawal amount will be incorrect
- **Impact**: User receives more or less than fair share value
- **Conclusion:** This does not stand. Total pool info is gotten by Control Center polling every vault for their adapters, their balance and withdrawal queue values. Query that gets adapter is done by executing a query towards adapter, which returns value. If value has dropped, it will return updated value.

**T-2: Token Conversion Rate Manipulation**

- If Token Info Provider rate is manipulated between deposit and withdrawal, user receives incorrect amount
- **Impact**: User loses value if rate drops, gains if rate increases artificially
- **Conclusion:** This is valid. The finding is written.

**T-3: Withdrawal Queue Amount Not Subtracted**

- If withdrawal queue amount is not properly subtracted from pool value, withdrawers receive less than fair share
- **Impact**: Later withdrawers subsidize earlier ones, value leakage
- **Conclusion:** The threat does not stand, withdrawal queue is maintained well.

**T-4: Inconsistent Adapter Position Queries**

- If some adapters fail to respond during withdrawal but succeeded during deposit, pool value is understated
- **Impact**: Withdrawer receives less than deposited value
- **Conclusion:** Adapters are out of scope and are considered as trusted contract. We expect them to act correctly. However, if there is a bug in the adapter code, this can be an issue. We have written a finding about adapter assumptions.

---

### VP-004: Pool Value Calculation Consistency

Pool value must be calculated consistently across all operations using the same formula:

```
pool_value_base = (contract_balance + adapter_positions - withdrawal_queue_amount) * conversion_rate
```

Where:

- `adapter_positions` includes only NotTracked adapters (Tracked adapters counted in Control Center)
- `withdrawal_queue_amount` is the total committed to pending withdrawals
- `conversion_rate` is from Token Info Provider (or 1:1 if not configured)

All share minting and burning operations must use the same pool value calculation. Inconsistent calculations would allow attackers to deposit at one price and withdraw at another, extracting value.

The formula must account for all sources of value (contract balance, adapters) and all commitments (withdrawal queue) to accurately represent available pool value.

### Threats

**T-1: Double-Counting Tracked Adapters**

- Register adapter as Tracked but query includes it in positions. If Tracked adapters are included in `adapter_positions` query, they're counted twice (in Control Center deployed amount and local query)
- **Impact**: Pool value overstated, users receive fewer shares on deposit, more tokens on withdrawal
- **Conclusion:** This cannot happen, because if they are Tracked, the loop continues

**T-2: Withdrawal Queue Not Subtracted**

- If withdrawal queue amount is not subtracted, pool value is overstated by committed funds
- **Impact**: New depositors receive fewer shares, existing holders can withdraw more than fair share
- **Conclusion:** The threat does not stand, withdrawal queue is maintained well.

**T-3: Silent Adapter Query Failures**

- Malicious adapter returns error on position query to manipulate share price. Failed adapter position queries are silently ignored, understating pool value
- **Impact**: Pool value underreported, users receive more shares on deposit than they should
- **Conclusion:** Adapters are out of scope and are considered as trusted contract. We expect them to act correctly. However, if there is a bug in the adapter code, this can be an issue. We have written a finding about adapter assumptions.

**T-4: Inconsistent Token Conversion**

- If conversion rate is applied inconsistently (sometimes used, sometimes not), pool value varies
- **Impact**: Share price fluctuates incorrectly, enabling arbitrage
- **Conclusion:** The threat does not stand, conversion rate rem.

**T-5: Race Condition in Control Center Pool Info**

- If Control Center pool info includes stale deployed amount while vault updates it, pool value is temporarily inconsistent
- **Impact**: Brief window where share price is incorrect, enabling sandwich attacks
- **Conclusion:** This does not stand. Minting and burning will always use the newest value, because they will execute query towards Control Center to get updated value (which will then query other contracts).

## Withdrawal Queue FIFO

### VP-005: Withdrawal ID Monotonicity

Withdrawal IDs must strictly increase with each new withdrawal request. For any two withdrawals created at times t1 < t2 → withdrawal_id(t1) < withdrawal_id(t2)

FIFO ordering depends on monotonically increasing IDs. The auto-incrementing ID mechanism ensures that earlier withdrawals have smaller IDs, which are processed first during fulfillment.

### Threats

**T-1: ID Counter Not Incremented**

- Race condition in ID assignment. If `NEXT_WITHDRAWAL_ID` is not incremented after assigning an ID, duplicate IDs could be created
- **Impact**: Two withdrawals with same ID, causing storage collision and loss of one withdrawal
- **Conclusion:** The threat does not stand, `NEXT_WITHDRAWAL_ID` is assigned in withdrawal and since transactions are atommical, this can not happen.

**T-2: ID Counter Overflow**

- After 2^64 withdrawals, ID counter overflows and wraps to 0
- **Impact**: ID ordering breaks, old withdrawals could be re-created with same ID
- **Conclusion:** It is possible to happen, but not likely, since the value would need to be too larger than `18446744073709551616`. This would cause DoS and not wrap around.

---

### VP-006: Fulfill Processes In Order

The `fulfill_pending_withdrawals` function must process withdrawals in ascending ID order and stop when insufficient funds are encountered. `fulfill_pending_withdrawals` iterates from `LAST_FUNDED_WITHDRAWAL_ID + 1` in ascending order. If `withdrawal[i]` cannot be funded, all `withdrawal[j]` where `j > i` remain unfunded.

The fulfill function implements the FIFO guarantee by starting from the last funded ID and proceeding in order. Stopping at the first unfundable withdrawal ensures no later withdrawals are funded before earlier ones.

### Threats

**T-1: Starting From Wrong ID**

- If fulfill starts from wrong ID (not `LAST_FUNDED_WITHDRAWAL_ID + 1`), it could skip withdrawals
- **Impact**: Earlier withdrawals remain unfunded while later ones get funded, breaking FIFO
- **Conclusion:** The threat does not stand, `LAST_FUNDED_WITHDRAWAL_ID` is gotten from the state.

**T-2: Not Breaking Loop on Insufficient Funds**

- If loop continues after encountering unfundable withdrawal, later withdrawals could be funded
- **Impact**: Queue jumping, FIFO violation
- **Conclusion:** The threat does not stand, once unfundable withdrawal is found, operation breaks.

**T-3: Processing Already-Funded Withdrawals**

- If fulfill doesn't check `is_funded` flag, it could re-fund withdrawals
- **Impact**: Double-counting funded withdrawals, incorrect `LAST_FUNDED_WITHDRAWAL_ID`
- **Conclusion:** The threat does not stand, fulfill checks `is_funded` when getting withdrawals and sets `is_funded` when funding them.

**T-4: Iteration Order Not Ascending**

- If storage iteration order is not guaranteed ascending, FIFO breaks
- **Impact**: Random order fulfillment instead of FIFO
- **Conclusion:** The threat does not stand, withdrawals are gotten from the state in ascending order.

---

### VP-007: Cannot Skip Unfunded Withdrawals

A withdrawal with ID `i` cannot be funded if there exists any withdrawal with ID `j < i` that is not funded. If `withdrawal[i].is_funded == true`, then for all `j < i`: `withdrawal[j].is_funded == true` OR `withdrawal[j]` does not exist

This property ensures strict FIFO: you cannot fund a later withdrawal while earlier ones remain unfunded. Combined with monotonic IDs and ordered fulfillment, this guarantees queue fairness.

### Threats

**T-1: Manual Funding Bypass**

- If there's a way to manually mark a withdrawal as funded (outside fulfill function), FIFO can be bypassed
- **Impact**: Privileged users could fund their own withdrawals first
- **Conclusion:** The threat does not stand, withdraws are always in order.

**T-2: Fulfill Limit Allows Gap**

- If fulfill is called with limit=N, stops at unfundable withdrawal, then new funds arrive and fulfill is called again, could it skip?
- **Impact**: Depends on `LAST_FUNDED_WITHDRAWAL_ID` tracking
- **Conclusion:** The threat does not stand, fulfill checks `is_funded` when getting withdrawals and sets `is_funded` when funding them. Only after the loop is broken, or done, there will be an increase in `LAST_FUNDED_WITHDRAWAL_ID`.

**T-3: Cancellation Creates Gaps**

- If `withdrawal[i]` is canceled after `withdrawal[i+1]` is funded, creates discontinuity
- **Impact**: Depends on whether canceled IDs are cleaned up properly
- **Conclusion:** The threat does not stand, order is always maintained. If `withdrawal[i+1]` is funded, this means that `withdrawal[i]` is funded also, and funded withdraws cannot be canceled.

## Share Burning

### VP-008: Shares Burned Before Token Transfer

In the immediate withdrawal path, shares must be burned before tokens are sent to the user:

```
In withdraw() when can_fulfill_entirely == true:
  burn_shares_msg at line 541 executes BEFORE send_tokens_msg at line 523
```

- **Conclusion**:  CosmWasm uses an actor model where messages returned in a `Response` execute sequentially after the contract's execute function completes. All messages in a transaction are atomic — if any message fails, the entire transaction reverts including all state changes. The ordering of messages in the response matters for logical dependencies (e.g., cannot spend funds before receiving them), but both burn and send will either succeed together or fail together due to atomic execution.

---

### VP-009: Shares Burned Before Queue Entry

In the queued withdrawal path, shares must be burned before or simultaneously with queue entry creation. In `withdraw()` when `can_fulfill_entirely == false`:

- `burn_shares_msg` executes
- Queue entry is created
- Both must complete or both must fail (atomically)

Even though the user doesn't receive tokens immediately when queued, their shares must be burned to prevent double-spending. The user cannot use those shares elsewhere while waiting in the queue.

### Threats

**T-1: Queue Entry Created Without Burning**

- If `WITHDRAWAL_REQUESTS.save` succeeds but burn message fails, queue entry exists without shares burned
- **Impact**: User in queue with shares still in circulation, could transfer or withdraw with same shares
- **Conclusion:** The threat does not stand, once there is a withdraw executed - shares are burned.

**T-2: Shares Burned Without Queue Entry**

- If burn succeeds but queue entry save fails, user loses shares without withdrawal
- **Impact**: User funds permanently lost, can't recover shares or receive withdrawal
- **Conclusion:** The threat does not stand, once there is a withdraw executed - queue is altered and shares are burned. This cannot happen partially.

**T-3: Queue Info Updated Without Burning**

- If `WITHDRAWAL_QUEUE_INFO` is updated but burn fails, accounting mismatch
- **Impact**: Queue info overstates shares burned, breaking property VP-010
- **Conclusion**: This cannot happen, if burn fails, tx and state reverts.

## Queue Accounting

### VP-010: Queue Info Updated on Withdrawal

When a withdrawal is queued, `WITHDRAWAL_QUEUE_INFO` must be updated to reflect the new entry:

```
On queue path in withdraw():
  WITHDRAWAL_QUEUE_INFO.total_shares_burned += withdrawal.shares_burned
  WITHDRAWAL_QUEUE_INFO.total_withdrawal_amount += withdrawal.amount_to_receive
  WITHDRAWAL_QUEUE_INFO.non_funded_withdrawal_amount += withdrawal.amount_to_receive
```

The queue aggregates must be incremented atomically with the individual withdrawal entry creation. This maintains the invariant that aggregates equal the sum of individual entries.

### Threats

**T-1: Aggregate Update Calculation Error**

- If the delta applied to `WITHDRAWAL_QUEUE_INFO` doesn't match the withdrawal entry values
- **Impact**: Aggregates drift from reality, available balance calculated incorrectly.
- **Conclusion:** This does not stand. Changes are done in appropriate way.

**T-2: Partial Update Failure**

- If one field updates but another fails (e.g., `total_shares_burned` succeeds, `total_withdrawal_amount` fails)
- **Impact**: Inconsistent queue state, some fields accurate, others not
- **Conclusion:** This does not stand. Changes are atomic.

**T-3: Integer Overflow in Aggregate**

- If adding to aggregate causes overflow, transaction should fail
- **Impact**: DoS if queue grows too large, or silent wraparound if not checked
- **Conclusion**: This is possible to happen, however since the numbers are Uint128, this value would need to be abnormally large.

---

### VP-011: Queue Info Updated on Cancel

When withdrawals are canceled, `WITHDRAWAL_QUEUE_INFO` must be decremented to reflect removed entries:

```
On cancel_withdrawal():
  WITHDRAWAL_QUEUE_INFO.total_shares_burned -= sum(canceled_withdrawals.shares_burned)
  WITHDRAWAL_QUEUE_INFO.total_withdrawal_amount -= sum(canceled_withdrawals.amount_to_receive)
  WITHDRAWAL_QUEUE_INFO.non_funded_withdrawal_amount -= sum(canceled_withdrawals.amount_to_receive)
```

Cancellation removes withdrawals from the queue, so aggregates must be decremented. The decrements must exactly match the values being removed to maintain invariant consistency.

### Threats

**T-1: Incorrect Decrement Amount**

- If accumulated `shares_burned` or `amount_to_withdraw` doesn't match sum of canceled entries
- **Impact**: Aggregates become inaccurate, drift from reality over time
- **Conclusion:** The threat does not stand. `shares_burned` represent amount of shares sent. `amount_to_withdraw` represents the value of shares.

**T-2: Underflow in Aggregate**

- If trying to subtract more than current aggregate value, calculation fails
- **Impact**: Transaction reverts, cancellation impossible even when valid
- **Conclusion:** The threat does not stand. If revert happens, it is by design.

**T-3: Canceling Already-Funded Withdrawal**

- If funded withdrawal is somehow canceled, `non_funded_withdrawal_amount` incorrectly decremented
- **Impact**: Non-funded amount becomes negative or incorrect
- **Conclusion:** The threat does not stand. Funded withdraws cannot be canceled.

**T-4: Partial Cancellation Update**

- If some withdrawal entries are removed but aggregate not updated for all
- **Impact**: Aggregate overstates actual queue size
- **Conclusion:** This does not stand. Changes are atomic.

---

### VP-012: Queue Info Updated on Fulfill

When withdrawals are marked as funded, `WITHDRAWAL_QUEUE_INFO.non_funded_withdrawal_amount` must be decremented:

```
On fulfill_pending_withdrawals():
  WITHDRAWAL_QUEUE_INFO.non_funded_withdrawal_amount -= sum(funded_withdrawals.amount_to_receive)
  total_shares_burned and total_withdrawal_amount remain unchanged
```

Fulfilling doesn't remove withdrawals from the queue, it only changes their funded status. Only the `non_funded_withdrawal_amount` should decrease, while total amounts remain the same until claiming.

### Threats

**T-1: Decrementing Wrong Fields**

- If `total_shares_burned` or `total_withdrawal_amount` are incorrectly decremented during fulfill
- **Impact**: Aggregates become inaccurate, queue appears smaller than reality
- **Conclusion:** The threat does not stand. Decrements are done appropriately.

**T-2: Not Updating LAST_FUNDED_WITHDRAWAL_ID**

- If `non_funded_withdrawal_amount` is decremented but `LAST_FUNDED_WITHDRAWAL_ID` not updated
- **Impact**: Next fulfill call starts from wrong position, could re-fund or skip withdrawals
- **Conclusion:** The threat does not stand. Decrements are done appropriately.

**T-3: Funding Already-Funded Withdrawal**

- If `withdrawal.is_funded` is already true but still processed
- **Impact**: Non-funded amount decremented multiple times for same withdrawal
- **Conclusion:** The threat does not stand. Only non-funded withdraws will be fulfilled.

**T-4: Incorrect Funded Amount Calculation**

- If `total_amount_funded` doesn't equal sum of individual withdrawal amounts
- **Impact**: Non-funded decrement doesn't match actual funded withdrawals
- **Conclusion:** The threat does not stand. `total_amount_funded` is calculated based on newly funded withdraws in a loop.

---

### VP-013: Queue Info Updated on Claim

When funded withdrawals are claimed, `WITHDRAWAL_QUEUE_INFO` must be fully decremented:

```
On claim_unbonded_withdrawals():
  WITHDRAWAL_QUEUE_INFO.total_shares_burned -= sum(claimed_withdrawals.shares_burned)
  WITHDRAWAL_QUEUE_INFO.total_withdrawal_amount -= sum(claimed_withdrawals.amount_to_receive)
  non_funded_withdrawal_amount remains unchanged (already zero for funded withdrawals)
```

Claiming removes withdrawals completely from the queue after paying out. Both total fields must be decremented while non_funded stays the same (these withdrawals were already funded, so non_funded was decremented during fulfill).

### Threats

**T-1: Decrementing non_funded on Claim**

- If `non_funded_withdrawal_amount` is decremented during claim for already-funded withdrawals
- **Impact**: Non-funded amount goes negative or becomes incorrect
- **Conclusion:** The threat does not stand. It is not decremented during claim.

**T-2: Not Removing Withdrawal Entries**

- If `WITHDRAWAL_QUEUE_INFO` is updated but entries not removed from `WITHDRAWAL_REQUESTS`
- **Impact**: Aggregates decrease but individual entries remain, causing divergence
- **Conclusion:** The threat does not stand. Entry is removed.

**T-3: Claiming Non-Funded Withdrawal**

- If withdrawal with `is_funded == false` is somehow claimed
- **Impact**: Total amounts decremented incorrectly, non_funded amount is wrong
- **Conclusion:** The threat does not stand. Only funded withdraws will be claimed

**T-4: Partial Claim Processing**

- If some withdrawals are paid out but aggregates not updated for all
- **Impact**: Aggregate overstates actual queue size
- **Conclusion:** This does not stand. Changes are atomic.

## Access Control

### VP-014: Adapter Operations Require Whitelist

All adapter management operations must be restricted to whitelisted addresses:

```
register_adapter(), unregister_adapter(), set_adapter_allocation_mode(),
set_adapter_deployment_tracking() all require sender in WHITELIST
```

Only trusted administrators should be able to configure which adapters receive vault funds. Allowing unauthorized adapter registration would enable attackers to register malicious adapters and steal funds.

### Threats

**T-1: Missing Authorization Check**

- If any adapter operation doesn't call `validate_address_is_whitelisted()`
- **Impact**: Anyone can register malicious adapter, steal funds
- **Conclusion**: This does not stand. There is a check.

**T-2: Adapter Can Self-Register**

- If adapter contract can call `register_adapter()` to add itself
- **Impact**: Malicious adapter can add itself without admin approval
- **Conclusion:** This does not stand. Adapters can only be added by whitelisted addresses.

---

### VP-015: Manual Fund Movements Require Whitelist

All manual fund deployment operations must be restricted to whitelisted addresses:

```
deposit_to_adapter(), withdraw_from_adapter(), move_adapter_funds(),
withdraw_for_deployment(), deposit_from_deployment() all require sender in WHITELIST
```

Manual fund movements can directly affect vault liquidity and deployed amounts. Only authorized administrators should control these operations to prevent unauthorized fund extraction or manipulation.

### Threats

**T-1: Missing Authorization on Fund Movement**

- If any manual fund operation doesn't check whitelist
- **Impact**: Anyone can withdraw funds from vault, move funds between adapters
- **Conclusion:** This does not stand. Only whitelisted addresses can operate with vault funds. However this poses a certain risk itself, which is addressed in a finding.

**T-2: Move Adapter Funds Without Tracking Validation**

- If `move_adapter_funds` doesn't validate tracking modes match for non-deposit denoms
- **Impact**: Accounting mismatch between vault and Control Center
- **Conclusion:** This does not stand, there is a check which assures that tracking modes are the same.

---

### VP-016: Whitelist Modifications Require Whitelist

Adding and removing addresses from the whitelist must itself require whitelist authorization.

The whitelist is selT-referential: only whitelisted addresses can modify the whitelist. This prevents unauthorized privilege escalation while allowing legitimate administrators to manage access control.

### Threats

**T-1: Add Without Authorization**

- If `add_to_whitelist` doesn't check sender is whitelisted
- **Impact**: Anyone can grant themselves whitelist privileges, complete access control bypass
- **Conclusion**: This does not stand. There is a call to `validate_address_is_whitelisted.`

**T-2: Remove Without Authorization**

- If `remove_from_whitelist` doesn't check sender is whitelisted
- **Impact**: Anyone can remove admins from whitelist, DoS administrative operations
- **Conclusion**: This does not stand. There is a call to `validate_address_is_whitelisted.`

**T-3: Adding Already-Whitelisted Address Succeeds**

- If duplicate whitelist entry is allowed
- **Impact**: Whitelist count inflated, could affect last-address-removal check
- **Conclusion**: This does not stand. There is a check which prevents it.

---

### VP-017: Cannot Remove Last Whitelisted Address

The whitelist must always contain at least one address:

```
At all times: |WHITELIST| >= 1
remove_from_whitelist() must fail if it would make |WHITELIST| == 0
```

If the whitelist becomes empty, no one can perform administrative operations, and the vault becomes permanently locked. Critical operations like adapter management and emergency fund movements would be impossible.

### Threats

**T-1: Counting Whitelist Incorrectly**

- If whitelist size check counts removed address or doesn't count correctly
- **Impact**: Last address is removed, vault becomes locked
- **Conclusion**: This does not stand, there is a check preventing this.

**T-2: Check After Removal**

- If address is removed from storage before checking count
- **Impact**: Check sees empty whitelist and fails, but address already removed
- **Conclusion**: This is done in such a way that after removing from the whitelist, there has to be at least one address in the state in order for tx not to revert.

**T-3: Unregister Adapter Allows Empty Whitelist**

- If some other operation can remove the last whitelisted address indirectly
- **Impact**: Vault becomes locked through side channel
- **Conclusion**: This does not stand. There is no other operation which can remove whitelisted address.

## Value Conservation

### VP-018: Cancel Withdrawal Preserves Value

Canceling a withdrawal must not create or destroy value:

```
total_pool_value_before_cancel == total_pool_value_after_cancel + minted_shares_value
```

The shares minted back must equal the withdrawal amount at current pool value.

When a user cancels their withdrawal, they should receive shares worth the same value as their canceled withdrawal amount. The operation reorganizes ownership (from committed withdrawal back to shares) but doesn't change total pool value. 

### Threats

**T-1: Share Calculation Uses Wrong Pool Value**

- If shares minted are calculated using pool value that doesn't include the canceled amount
- **Impact**: User receives incorrect number of shares, value created or destroyed
- **Conclusion:** This does not stand. Mint number is calculated in the same way as if it was a deposit. However, since there is recalculation of shares, this poses another threat itself, and we have written a finding.

**T-2: Share Inflation Through Cancel**

- Cancel withdrawal after pool loses value to get "discount" shares. User withdraws when pool value is high, cancels when pool value is low, receives more shares than originally burned
- **Impact**: User gains extra value through timing, other holders diluted
- **Conclusion:** The threat is valid. Finding is written.

**T-3: Not Checking Deposit Cap**

- If cancel is allowed even when `deposit_cap` is reached
- **Impact**: Vault accepts more value than intended capacity
- **Conclusion**: This does not stand. Deposit cap relates to deposits, and does not affect cancellation.

**T-4: Withdrawal Queue Not Restored**

- If queue info is updated but withdrawal amount not added back to pool value calculation
- **Impact**: Pool value underreported, share price artificially low
- **Conclusion:** This does not stand. Changes are atomic.

---

### VP-019: Adapter Fund Movements Preserve Value

Moving funds between adapters or deploying/recalling funds manually must not change total pool value:

```
For deposit_to_adapter(), withdraw_from_adapter(), move_adapter_funds():
  total_pool_value_before == total_pool_value_after
```

These operations move funds between the vault's contract balance and adapter positions, or between different adapters. They reorganize where funds are held but don't add or remove value from the system.

### Threats

**T-1: Deployed Amount Double-Counted**

- If `withdraw_from_adapter` decrements deployed amount but adapter position still counted in pool value
- **Impact**: Total pool value overstated, share price inflated
- **Conclusion:** This does not stand. Once it has been withdrawn from adapter, means that adapter will not have assets and ControlCenter will not count those when querying.

**T-2: Moving Between Different Tracking Modes**

- Move funds to manipulate which adapters are counted in pool value. If funds moved from `Tracked` to `NotTracked` adapter or vice versa without proper accounting
- **Impact**: Value counted twice or not counted at all
- **Conclusion:** This does not stand. There is a check preventing this.

**T-3: Adapter Withdrawal Partial Failure**

- If `withdraw_from_adapter` calls adapter but adapter returns less than requested
- **Impact**: Deployed amount decremented by full amount but less received
- **Conclusion:** This does not stand. Changes are atomic.

**T-4: Move Funds Without Tracking Update**

- If `move_adapter_funds` doesn't update deployed amount when moving between Tracked adapters
- **Impact**: Deployed amount becomes incorrect, pool value wrong
- **Conclusion:** This does not stand. Changes are done appropriatelly.

---

### VP-020: Config Updates Don't Affect Value

Configuration changes must not alter total pool value:

```
For update_config(), set_token_info_provider_contract():
  total_pool_value_before == total_pool_value_after
```

Administrative configuration changes like adjusting `max_withdrawals_per_user` or changing the token info provider should not create or destroy value. They modify operational parameters, not the actual funds held.

### Threats

**T-1: Token Info Provider Change Affects Pool Value**

- Whitelisted user changes provider to manipulate rate, withdraws at better rate. If changing `token_info_provider` immediately changes conversion rate, pool value calculation changes
- **Impact**: Share price suddenly changes without actual value transfer
- **Conclusion:** This is a valid concern. We have written a finding.

**T-2: max_withdrawals_per_user Affects Queue**

- If reducing `max_withdrawals_per_user` invalidates existing queued withdrawals
- **Impact**: Users stuck with more withdrawals than new limit allows
- **Conclusion:** This does not stand, because max_withdrawals_per_user are not regarded when fulfilling or cancelling withdraws.

**T-3: Setting Token Provider to Malicious Contract**

- Whitelisted admin sets `token_info_provider` to contract that returns manipulated rates
- **Impact**: All subsequent pool value calculations wrong, share price manipulated
- **Conclusion:** This is a valid concern. We have written a finding.

**T-4: No Validation of New Config Values**

- If `update_config` doesn't validate new `max_withdrawals_per_user` is reasonable
- **Impact**: Could be set to 0 (DoS all withdrawals) or huge value (memory issues)
- **Conclusion:** This is a valid concern. We have written a finding.

## User Ownership

### VP-021: User Withdrawal List Consistency

The `USER_WITHDRAWAL_REQUESTS` mapping must be updated atomically with `WITHDRAWAL_REQUESTS` operations:

- When withdrawal entry is created: `USER_WITHDRAWAL_REQUESTS[user]` must include withdrawal ID
- When withdrawal entry is removed: `USER_WITHDRAWAL_REQUESTS[user]` must not include withdrawal ID

The user withdrawal list is a convenience index for querying user-specific withdrawals. It must stay perfectly synchronized with the main `WITHDRAWAL_REQUESTS` map. Any divergence would cause confusion, allow orphaned entries, or prevent users from canceling legitimate withdrawals.

### Threats

**T-1: Withdrawal Entry Created But Not Added to User List**

- If `WITHDRAWAL_REQUESTS.save` succeeds but `USER_WITHDRAWAL_REQUESTS.update` fails
- **Impact**: User has withdrawal in queue but can't query or cancel it
- **Conclusion:** This does not stand. Queues are maintained appropriatelly.

**T-2: Withdrawal Entry Removed But Remains in User List**

- If `WITHDRAWAL_REQUESTS.remove` succeeds but `USER_WITHDRAWAL_REQUESTS` not updated
- **Impact**: User's list contains IDs for non-existent withdrawals, queries fail
- **Conclusion:** This does not stand. There is a check preventing this.

**T-3: Duplicate IDs in User List**

- If withdrawal ID is added to user list multiple times
- **Impact**: User list bloated, could cause them to hit `max_withdrawals_per_user` incorrectly
- **Conclusion:** This does not stand. Withdrawal IDs are incremental.

**T-4: Wrong User Address in Withdrawal Entry**

- If withdrawal entry has `withdrawer = UserA but ID` is added to `UserB`'s list
- **Impact**: `UserB` can cancel `UserA`'s withdrawal, access control violation
- **Conclusion:** This does not stand. There is a check preventing this.

---

### VP-022: on_behalf_of Parameter Security

The `on_behalf_of` parameter in deposit and withdraw must not enable unauthorized operations:

When `withdraw(on_behalf_of = UserA)`:

- Shares must be paid by sender (not `UserA`)
- Withdrawal entry's withdrawer must be `UserA`
- `UserA`'s `USER_WITHDRAWAL_REQUESTS` list is updated (not sender's)
- Cancellation must be done by `UserA` only

The `on_behalf_of` feature allows one user to create deposits/withdrawals for another user. This is useful for contract integrations but must be carefully designed to prevent abuse. The sender provides the tokens/shares, but the resulting ownership belongs to the specified user.

### Threats

**T-1: Withdrawal Queue DoS via `on_behalf_of`**

- Attacker DoSes victim’s withdrawal queue. Attacker calls withdraw `on_behalf_of` victim repeatedly with their own shares to fill victim’s withdrawal queue
- **Impact**: Victim reaches `max_withdrawals_per_user limit`, cannot create their own withdrawals
- **Conclusion:** This is a valid concern. We have written a finding.

**T-2: Cancel on_behalf_of Allowed**

- If sender can cancel withdrawals created `on_behalf_of` another user
- **Impact**: Creator can cancel victim's withdrawals, preventing them from exiting
- **Conclusion:** This does not stand. There is a check preventing this.

**T-3: on_behalf_of Bypasses Withdrawal Limit Check**

- If `max_withdrawals_per_user` check uses sender instead of `on_behalf_of` user
- **Impact**: Sender could create unlimited withdrawals for victim
- **Conclusion:** This does not stand. If `on_behalf_of` is used, then its value will be entered in the queue.

## Adapter Isolation

### VP-023: Adapter Query Failures Are Isolated

When adapter queries fail during automated allocation or position queries, the vault must continue operating:

If `adapter.query(AvailableForDeposit)` fails:

- Vault skips this adapter silently
- Other adapters are still queried
- Deposit operation succeeds with remaining adapters

If `adapter.query(DepositorPosition)` fails:

- Pool value calculation continues without this adapter
- Other adapters' positions still counted

The fail-open design prioritizes vault availability over complete accuracy. If an adapter is temporarily unavailable or buggy, the vault continues operating with the remaining healthy adapters.

### Threats

**T-1: Silent Failures Hide Persistent Issues**

- If adapter always fails queries, vault never deploys to it but no error is raised
- **Impact**: Funds remain undeployed, users lose potential yield, no visibility into problem
- **Conclusion:** This is a valid concern. We have written a finding.Adapters are out of scope and are considered as trusted contract. We expect them to act correctly. However, if there is a bug in the adapter code, this can be an issue. We have written a finding about adapter assumptions.

**T-2: Pool Value Underreported Due to Failed Queries**

- Malicious adapter intentionally fails query when pool value would be high. If adapter holding large position fails `DepositorPosition` query, pool value is understated
- **Impact**: Share price artificially low, new depositors receive too many shares
- **Conclusion:** Adapters are out of scope and are considered as trusted contract. We expect them to act correctly. However, if there is a bug in the adapter code, this can be an issue. We have written a finding about adapter assumptions.

**T-3: Asymmetric Query Success**

- Adapter succeeds `AvailableForDeposit` during deposit but fails `DepositorPosition` later
- **Impact**: Funds deployed but not counted in pool value, share price wrong
- **Conclusion:** This does not stand. Messages are executed atomically.

**T-4: All Automated Adapters Fail**

- If all automated adapters fail availability queries, no funds are deployed
- **Impact**: All deposits remain in contract balance, no yield generation
- **Conclusion:** This is valid, but is intended. There is a function to deploy funds to the adapter manually.

---

### VP-024: Adapter Cannot Drain Contract Balance

Adapters can only control funds explicitly deployed to them; they cannot access the vault's contract balance. For any adapter A:

- A can only receive funds via Deposit message
- A can only be required to return funds via Withdraw message
- A cannot directly access vault's bank balance

Adapters are external contracts that is developed by the team and considered trusted. There cannot be a malicious adapter. However, adapters are constrained to only a set of actions.

### Threats

**T-1: Adapter Calls Withdraw on Vault**

- If adapter calls vault's withdraw function
- **Impact**: Depends on whether vault validates caller or could be tricked
- **Conclusion:** Adapters are out of scope and are considered as trusted contract. We expect them to act correctly. However, if there is a bug in the adapter code, this can be an issue. We have written a finding about adapter assumptions.

**T-2: Adapter Returns Wrong Amount**

- On `withdraw_from_adapter`, adapter returns less than requested amount
- **Impact**: Deployed amount decremented by full requested but less received
- **Conclusion:** Adapters are out of scope and are considered as trusted contract. We expect them to act correctly. However, if error is not returned but only caught in that contract, this will be an issue. We have written a finding about adapter assumptions.

---

### VP-025: Failed Adapter Withdrawals Revert Transaction

When an immediate withdrawal requires adapter withdrawals, the entire transaction must revert if any adapter withdrawal fails. `If withdraw()` takes immediate path and calls `adapter.withdraw()`:

- If `adapter.withdraw()` fails → entire transaction reverts
- User's shares are not burned (transaction rolled back)
- User can retry withdrawal

The "all-or-nothing" withdrawal strategy ensures users either receive their full withdrawal amount or nothing. If an adapter fails to provide the required liquidity, the user's shares are not burned and they can try again later or wait for the queue path.

This prevents partial withdrawals that would leave the user with burned shares but insufficient tokens received.

### Threats

**T-1: Adapter Withdrawal Fails But Transaction Continues**

- If adapter withdrawal error is caught and ignored instead of propagating
- **Impact**: User's shares burned but tokens not received, permanent loss
- **Conclusion:** Adapters are out of scope and are considered as trusted contract. We expect them to act correctly. However, if there is a bug in the adapter code, this can be an issue. We have written a finding about adapter assumptions.

**T-2: Adapter Returns Success But Doesn't Send Funds**

- Malicious adapter claims success but doesn't actually send tokens
- **Impact**: User receives less than expected, share calculation wrong
- **Conclusion:** Adapters are out of scope and are considered as trusted contract. We expect them to act correctly. However, if there is a bug in the adapter code, this can be an issue. We have written a finding about adapter assumptions.

**T-3: Queue Path Taken Despite Available Funds**

- If calculation of available funds is wrong, withdrawal queued when it could be immediate
- **Impact**: User waits in queue unnecessarily, worse UX but not critical security issue
- **Conclusion**: This does not stand. Withdrawals will be queued only when there is not enough assets to fullfil.