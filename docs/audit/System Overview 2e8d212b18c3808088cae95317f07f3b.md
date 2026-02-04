# System Overview

**Inflow Vault** is a CosmWasm/Neutron vault that takes deposits in one configured token (for example a liquid-staked ATOM derivative like stATOM) and issues **vault shares** that represent proportional ownership of the vault’s total value. The vault can deploy funds into external protocols using **adapter contracts**, so strategy logic stays modular and the vault contract remains the central accounting and coordination point.

The vault integrates with a **Control Center** contract for governance-style parameters and reporting. It queries the Control Center for things like **deposit caps** and pool information, and (depending on adapter settings) reports how much has been deployed. If configured, a **Token Info Provider** supplies exchange-rate data so the vault can accept deposits in one denomination while valuing the pool in a base denomination (e.g., deposit stATOM but track value in ATOM).

User actions are permissionless: anyone can **deposit** and **withdraw** (including “on behalf of” another address if enabled), and anyone can help process the withdrawal queue. Admin operations are restricted by a **whitelist** and cover adapter registration/configuration, manual fund movements, and config updates.

## Deposits

On deposit, the vault checks the deposit cap, optionally converts the deposit into base units using the Token Info Provider rate, then mints shares so ownership stays proportional. For the first depositor, shares are effectively 1:1; after that, share issuance is based on current pool value and total shares outstanding.

If there are automated adapters configured, the vault allocates deposit funds across them based on each adapter’s reported capacity, in a deterministic order. Adapter failures don’t block deposits; the vault skips adapters that fail to respond.

## Withdrawals and the queue

Withdrawals follow an **all-or-nothing** approach: the vault either pays immediately or queues the entire request. A key security decision is that **shares are burned immediately** when a withdrawal is requested, before external interactions.

If liquidity is sufficient (vault balance plus what can be pulled from adapters, accounting for queued obligations), the vault withdraws from adapters if needed and pays the user right away. If not, it creates a FIFO withdrawal entry in a queue.

Queued withdrawals are handled in two permissionless steps:

1. **Fulfill:** mark withdrawals as funded in FIFO order when the vault has enough balance.
2. **Claim:** users (or anyone) claim funded withdrawals, and the vault sends payouts and removes the entries.

Users can **cancel** withdrawals only if they are not funded yet, and receive shares minted back based on current pool value (meaning the share amount may differ from what was burned).

## Adapters and accounting

Adapters are modular integrations with external protocols. Each adapter is configured as:

- **Automated vs Manual:** automated participates in vault allocation logic; manual is admin-driven only.
- **Tracked vs NotTracked:** tracked positions are accounted via the Control Center; not-tracked positions are queried directly from adapters when computing pool value (to avoid double counting).

Pool value is essentially: **vault balance + adapter positions − queued withdrawal obligations**, with optional conversion into a base denomination via the Token Info Provider.

## Access control

The vault splits actions into **public user flows** and **privileged operations**. Admin access is enforced with a **whitelist** stored in contract state.

- **Whitelisted-only** operations include: registering/unregistering adapters, switching adapter modes (automated/manual, tracked/not-tracked), manual deposits/withdrawals/moves between adapters, updating config (including Token Info Provider settings), and managing the whitelist itself.
- **Permissionless (public)** operations include: deposit, withdraw, cancel eligible withdrawals, and the queue maintenance calls (fulfill funded withdrawals and claim payouts).

The whitelist logic also prevents removing the **last remaining whitelisted address**, so the contract can’t accidentally strand itself without any admin.

## Pool value calculation

Pool value is computed so share pricing reflects the vault’s real backing while excluding funds already committed to withdrawals.

Conceptually, the vault’s pool value is:

- **On-chain balance** of the deposit token held by the vault
- **Plus** positions held in adapters that are **NotTracked** (queried directly from each adapter via a position query)
- **Minus** the total amount reserved/committed in the **withdrawal queue** (because those funds are already owed)
- **Then optionally converted** into a **base denomination** using the Token Info Provider exchange rate (if configured)

Tracked adapters are not summed directly by the vault (to avoid double-counting), because their deployed amounts are expected to be represented via the Control Center’s aggregated accounting.

This pool value is what the vault uses to mint/burn shares proportionally and to compute withdrawal amounts from share quantities.

## Trust assumptions

The vault relies on a few external actors and one privileged role. User actions (deposit/withdraw/queue processing) are permissionless, but correctness and safety depend on these dependencies behaving. More about that can be found in [Threat model](Threat%20Model%202e8d212b18c3805ebb2ff64cdbe341f3.md).

### Whitelisted operators

Whitelisted addresses can **configure adapters**, perform **manual fund movements**, and update limited config (e.g., max withdrawals per user, Token Info Provider). They **cannot directly transfer arbitrary vault funds to themselves**, cannot edit other users’ withdrawal requests, and must follow the same share rules as everyone else. The real risk is indirect: a malicious or careless operator can register bad adapters or misconfigure tracking/accounting. The contract also prevents removing the last whitelisted address.

### Control Center

The vault trusts the Control Center for **deposit caps** and **pool/aggregation info**, and (for tracked adapters) for deployed-amount accounting. If the Control Center lies, breaks, or blocks updates, deposits and accounting can be disrupted.

### Token Info Provider (optional)

If enabled, it supplies the **exchange rate** between deposit token and base token. Bad or manipulable rates distort **share price and withdrawals**. If disabled, the vault assumes a 1:1 ratio (deposit denom is the base denom).

### Adapters

Adapters **custody deployed funds** and report capacity/positions. Automated allocation depends on adapters honestly reporting `AvailableForDeposit/Withdraw` and positions. A malicious adapter can **steal or lock deployed funds**, but it can’t directly touch undeployed vault balance. Query failures during allocation are skipped; withdrawal failures during the immediate-withdraw path revert the transaction.

### Neutron token factory

The vault assumes the token factory correctly enforces **authorized mint/burn** and correct supply reporting for vault shares.

### Governance protections

No built-in **timelock/multisig** in the contract. If you want that, put a multisig or governance-controlled account on the whitelist and enforce process off-chain.

### Main trust boundaries

- between Vault and Adapters: biggest custody risk (deployed funds).
- between Vault and Token Info Provider: biggest pricing risk (share price).
- between Vault and Control Center: parameter/accounting dependency (caps, aggregation).
- between Whitelisted users and Users: operators can’t directly drain, but can harm via integrations/config.