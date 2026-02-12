# Inflow Vault Integration Guide

Inflow vaults are CosmWasm smart contracts on the Neutron blockchain that accept token deposits, issue vault shares in return, and deploy deposited funds into yield strategies. This guide covers everything you need to integrate with the vaults: depositing, withdrawing, handling the withdrawal queue, and querying vault state.

## Contract Entrypoints for Depositing/Withdrawing

### Depositing

To deposit tokens into a vault, send an `ExecuteMsg::Deposit` message **with the deposit tokens attached as funds**.

**Message format (JSON):**
```json
{
  "deposit": {
    "on_behalf_of": null
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `on_behalf_of` | `string \| null` | Optional. Recipient address for the vault shares. Defaults to the sender. |

**Attached funds:** You must include the deposit denomination tokens (e.g. ATOM, USDC, wBTC) as the transaction funds. The amount you attach is the amount that gets deposited.

**What happens:** The vault mints vault shares proportional to your deposit relative to the total pool value. The share price is determined by `total_pool_value / total_shares_issued` across all sub-vaults managed by the Control Center.

### Withdrawing

To withdraw from a vault, send an `ExecuteMsg::Withdraw` message **with your vault shares attached as funds**.

**Message format (JSON):**
```json
{
  "withdraw": {
    "on_behalf_of": null
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `on_behalf_of` | `string \| null` | Optional. Recipient address for the withdrawn tokens. Defaults to the sender. |

**Attached funds:** You must include vault share tokens as the transaction funds. The vault share denom is a tokenfactory denom like `factory/<vault_contract_address>/inflow_atom_ushare`.

**What happens:** The vault calculates the equivalent deposit-token value of your shares. If sufficient funds are available in the vault, the withdrawal is fulfilled immediately. Otherwise, the withdrawal enters the **withdrawal queue** (see below).

### Cancelling a Withdrawal

If your withdrawal is in the queue and has **not yet been funded**, you can cancel it:

```json
{
  "cancel_withdrawal": {
    "withdrawal_ids": [1, 2, 3]
  }
}
```

This returns your vault shares back to you. You cannot cancel withdrawals that have already been marked as funded.

### Claiming Funded Withdrawals

Once queued withdrawals are funded, claim them with:

```json
{
  "claim_unbonded_withdrawals": {
    "withdrawal_ids": [1, 2, 3]
  }
}
```

This transfers the deposit tokens to the withdrawer.

## Handling the Withdrawal Queue

The vault uses an **all-or-nothing** withdrawal queue system. When a user requests a withdrawal, one of two things happens:

1. **Instant fulfillment** -- If the vault holds enough liquid funds (contract balance + reclaimable adapter funds) to cover the full withdrawal amount, the tokens are sent immediately.

2. **Queued** -- If insufficient funds are available, the entire withdrawal request enters a FIFO queue. The user's shares are burned, and a `WithdrawalEntry` is created.

### Withdrawal lifecycle

```
User sends Withdraw
       │
       ▼
  Enough funds? ──Yes──► Instant payout
       │
       No
       │
       ▼
  Added to queue (is_funded: false)
       │
       ▼
  FulfillPendingWithdrawals called
  (processes queue in FIFO order)
       │
       ▼
  Marked funded (is_funded: true)
       │
       ▼
  User calls ClaimUnbondedWithdrawals
       │
       ▼
  Tokens sent to user
```

### Key queries

#### Share price / ratio (on-chain)

Query the vault contract's `pool_info` to get the current share ratio:
```json
{ "pool_info": {} }
```
Returns:
- `shares_issued` -- Total vault shares outstanding
- `balance_base_tokens` -- Vault's liquid balance in deposit tokens (held in the contract)
- `adapter_deposits_base_tokens` -- Tokens currently deployed to yield adapters/strategies
- `withdrawal_queue_base_tokens` -- Tokens reserved for queued withdrawals

**Note:** The vault `pool_info` only reflects this single vault's local state. For the true share price, query the **Control Center's** `pool_info` instead, which aggregates across all sub-vaults: `total_pool_value / total_shares_issued` (see [Control Center Queries](#control-center-queries)).

You can also query a specific share amount's value directly:
```json
{ "shares_equivalent_value": { "shares": "1000000" } }
```

Note that this only works for an amount of shares up to the total minted by the vault.

#### TVL and share ratio history (metrics API)

The metrics backend provides historical TVL and share ratio data:
```
GET https://inflow-vault-metrics-brnuh.ondigitalocean.app/average?vault={vault}&days={days}&points_per_day={points_per_day}
```

| Parameter | Required | Description |
|-----------|----------|-------------|
| `vault` | Yes | `atom`, `usdc`, or `btc` |
| `days` | No (default: 1) | Number of days of history |
| `points_per_day` | No (default: 24, or 4 if days > 7) | Data points per day (1-24) |

Returns an array of snapshots:
```json
[
  {
    "time": 1704067200,
    "share_ratio": 1.05,
    "total_pool_value": 1050000,
    "price": 10.0
  }
]
```

#### APY (metrics API)

```
GET https://inflow-vault-metrics-brnuh.ondigitalocean.app/apy?vault={vault}
```

| Parameter | Required | Description |
|-----------|----------|-------------|
| `vault` | Yes | `atom`, `usdc`, or `btc` |

Returns current APY with multiple time windows:
```json
{
  "timestamp": 1704585600,
  "daily_apy": 0.045,
  "avg_7_days": 0.040,
  "avg_14_days": 0.038,
  "overall_average": 0.035
}
```

APY is calculated from share ratio changes: `(ratioEnd / ratioStart)^(365/days) - 1`.

For daily APY history (useful for charts), use:
```
GET https://inflow-vault-metrics-brnuh.ondigitalocean.app/apy_daily?vault={vault}&days={days}
```

#### Average withdrawal time (metrics API)

```
GET https://inflow-vault-metrics-brnuh.ondigitalocean.app/user/avg-withdrawal-time?vault={vault}
```

Returns the average time for queued withdrawals to be fulfilled over the last 7 days:
```json
{
  "avg_withdrawal_time_seconds": 3600
}
```

#### Withdrawal queue (on-chain)

**Queue overview:**
```json
{ "withdrawal_queue_info": {} }
```
Returns an object with an `info` field containing `WithdrawalQueueInfo`:
```json
{
  "info": {
    "total_shares_burned": "396605",
    "total_withdrawal_amount": "393459",
    "non_funded_withdrawal_amount": "0"
  }
}
```
- `info.total_shares_burned` -- Total shares burned across all queued withdrawals
- `info.total_withdrawal_amount` -- Total deposit-token value of all queued withdrawals
- `info.non_funded_withdrawal_amount` -- Amount still awaiting funding

**A user's pending withdrawals:**
```json
{
  "user_withdrawal_requests": {
    "address": "neutron1...",
    "start_from": 0,
    "limit": 50
  }
}
```
Returns a list of `WithdrawalEntry` objects:
```json
{
  "id": 1,
  "initiated_at": "1700000000000000000",
  "withdrawer": "neutron1...",
  "shares_burned": "1000000",
  "amount_to_receive": "1050000",
  "is_funded": false
}
```

**Funded (claimable) withdrawals:**
```json
{
  "funded_withdrawal_requests": { "limit": 50 }
}
```

**Trigger funding of queued withdrawals (permissionless):**
```json
{
  "fulfill_pending_withdrawals": { "limit": 10 }
}
```
Anyone can call this. It iterates through unfunded withdrawals in FIFO order and marks them as funded if the contract holds sufficient balance.

## List of Vault Contracts and Control Centers

### Mainnet (Neutron)

| Asset | Vault Contract | Control Center |
|-------|---------------|----------------|
| ATOM  | `neutron1jymuhmex63kq8r20vss30jq2n7jmz9kqpegka8qwqccxx4nveufsmkv2fa` | `neutron1vk3cy35cudlpk8w9kuu9prcanc49n3ajcnu86a43ue9ln6v4v6zsaucnw9` |
| USDC  | `neutron19n7699nh4388v6wcxjke88j5933phrc3azsw783wnkt4255nc9rqzr79c0` | `neutron1d054u05vx29k20gqrj5h2h2lz7pl7x9fch4ypl5jmaj6q5yw4vgsgk4lx0` |
| wBTC  | `neutron197k8x8rr6860jt40jsaa779yyh53ug5y7aewp9uffd38f5npp9tqaw48d3` | `neutron1c3djqnwur4aryxe7knr4kvcm3hj2wvnl5887lc5dwsh7z40pf2cq9flznr` |

### Deposit Denoms (IBC on Neutron)

| Asset | Denom |
|-------|-------|
| ATOM  | `ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9` |
| USDC  | `ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81` |
| wBTC  | `ibc/0E293A7622DC9A6439DB60E6D234B5AF446962E27CA3AB44D0590603DFF6968E` |

### Vault Share Denoms (Mainnet)

| Asset | Vault Share Denom |
|-------|-------------------|
| ATOM  | `factory/neutron1jymuhmex63kq8r20vss30jq2n7jmz9kqpegka8qwqccxx4nveufsmkv2fa/inflow_atom_ushare` |
| USDC  | `factory/neutron19n7699nh4388v6wcxjke88j5933phrc3azsw783wnkt4255nc9rqzr79c0/inflow_usdc_ushare` |
| wBTC  | `factory/neutron197k8x8rr6860jt40jsaa779yyh53ug5y7aewp9uffd38f5npp9tqaw48d3/inflow_btc_ushare` |

### Token Decimals

All on-chain amounts are stored in micro-units. Use these decimal places to convert between micro-units and human-readable values:

| Asset | Decimals | Example |
|-------|----------|---------|
| ATOM  | 6 | `1000000` = 1 ATOM |
| USDC  | 6 | `1000000` = 1 USDC |
| wBTC  | 8 | `100000000` = 1 wBTC |

Vault shares use the same number of decimals as their underlying deposit token.

### Querying via LCD REST API

As an alternative to using CosmJS, you can query contract state directly via the Neutron LCD REST API. This is useful for browser-based integrations or any HTTP client.

**Endpoint format:**
```
GET https://neutron-rest.publicnode.com/cosmwasm/wasm/v1/contract/{contract_address}/smart/{base64_query}
```

Where `{base64_query}` is the base64-encoded JSON query message. For example, to query `pool_info` on the ATOM Control Center:

```
# Query: {"pool_info":{}}
# Base64: eyJwb29sX2luZm8iOnt9fQ==

GET https://neutron-rest.publicnode.com/cosmwasm/wasm/v1/contract/neutron1vk3cy35cudlpk8w9kuu9prcanc49n3ajcnu86a43ue9ln6v4v6zsaucnw9/smart/eyJwb29sX2luZm8iOnt9fQ==
```

The response wraps the contract's return value in a `data` field:
```json
{
  "data": {
    "total_pool_value": "478616650118",
    "total_shares_issued": "454921048556"
  }
}
```

To query a user's token balance:
```
GET https://neutron-rest.publicnode.com/cosmos/bank/v1beta1/balances/{address}/by_denom?denom={denom}
```

### Control Center Queries

The Control Center manages global state across all sub-vaults for a given asset class. Useful queries:

```json
{ "pool_info": {} }
```
Returns `total_pool_value` and `total_shares_issued` across all sub-vaults, which determines the share price:
```json
{
  "total_pool_value": "478616650118",
  "total_shares_issued": "454921048556"
}
```
Share price = `total_pool_value / total_shares_issued` (in micro-units, so the ratio is the human-readable price).

```json
{ "subvaults": {} }
```
Returns the list of registered sub-vault addresses.

## Full Example of Integration

Below is a complete TypeScript example using `@cosmjs/cosmwasm-stargate` to deposit into and withdraw from the ATOM vault.

### Setup

```typescript
import { SigningCosmWasmClient } from "@cosmjs/cosmwasm-stargate";
import { GasPrice } from "@cosmjs/stargate";
import { DirectSecp256k1HdWallet } from "@cosmjs/proto-signing";

const RPC_ENDPOINT = "https://neutron-rpc.publicnode.com";

// ATOM vault on mainnet
const VAULT_CONTRACT = "neutron1jymuhmex63kq8r20vss30jq2n7jmz9kqpegka8qwqccxx4nveufsmkv2fa";
const ATOM_DENOM = "ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9";
const VAULT_SHARE_DENOM = "factory/neutron1jymuhmex63kq8r20vss30jq2n7jmz9kqpegka8qwqccxx4nveufsmkv2fa/inflow_atom_ushare";

async function getClient(mnemonic: string) {
  const wallet = await DirectSecp256k1HdWallet.fromMnemonic(mnemonic, {
    prefix: "neutron",
  });
  const [{ address }] = await wallet.getAccounts();
  const client = await SigningCosmWasmClient.connectWithSigner(
    RPC_ENDPOINT,
    wallet,
    { gasPrice: GasPrice.fromString("0.025untrn") }
  );
  return { client, address };
}
```

### Deposit

```typescript
async function deposit(mnemonic: string, amountAtom: number) {
  const { client, address } = await getClient(mnemonic);

  // Convert to micro-units (6 decimals for ATOM)
  const microAmount = Math.round(amountAtom * 1_000_000).toString();

  // Deposit + fulfill pending withdrawals in one transaction
  const result = await client.signAndBroadcast(
    address,
    [
      {
        typeUrl: "/cosmwasm.wasm.v1.MsgExecuteContract",
        value: {
          sender: address,
          contract: VAULT_CONTRACT,
          msg: new TextEncoder().encode(
            JSON.stringify({ deposit: { on_behalf_of: null } })
          ),
          funds: [{ denom: ATOM_DENOM, amount: microAmount }],
        },
      },
      {
        typeUrl: "/cosmwasm.wasm.v1.MsgExecuteContract",
        value: {
          sender: address,
          contract: VAULT_CONTRACT,
          msg: new TextEncoder().encode(
            JSON.stringify({ fulfill_pending_withdrawals: { limit: 10 } })
          ),
          funds: [],
        },
      },
    ],
    "auto"
  );

  console.log("Deposit tx:", result.transactionHash);
}
```

### Withdraw

```typescript
async function withdraw(mnemonic: string, sharesAmount: string) {
  const { client, address } = await getClient(mnemonic);

  const result = await client.execute(
    address,
    VAULT_CONTRACT,
    { withdraw: { on_behalf_of: null } },
    "auto",
    undefined,
    [{ denom: VAULT_SHARE_DENOM, amount: sharesAmount }]
  );

  console.log("Withdraw tx:", result.transactionHash);
  // Check if withdrawal was instant or queued by querying user_withdrawal_requests
}
```

### Check and Claim Queued Withdrawals

```typescript
async function checkAndClaimWithdrawals(mnemonic: string) {
  const { client, address } = await getClient(mnemonic);

  // 1. Query user's pending withdrawal requests
  const requests = await client.queryContractSmart(VAULT_CONTRACT, {
    user_withdrawal_requests: {
      address: address,
      start_from: 0,
      limit: 50,
    },
  });

  console.log("Pending withdrawals:", requests.withdrawals);

  // 2. Filter for funded withdrawals that can be claimed
  const fundedIds = requests.withdrawals
    .filter((w: any) => w.is_funded)
    .map((w: any) => w.id);

  if (fundedIds.length === 0) {
    // Try to trigger funding first
    await client.execute(
      address,
      VAULT_CONTRACT,
      { fulfill_pending_withdrawals: { limit: 50 } },
      "auto"
    );

    // Re-check
    const updated = await client.queryContractSmart(VAULT_CONTRACT, {
      user_withdrawal_requests: { address, start_from: 0, limit: 50 },
    });
    const newFundedIds = updated.withdrawals
      .filter((w: any) => w.is_funded)
      .map((w: any) => w.id);

    if (newFundedIds.length > 0) {
      const result = await client.execute(
        address,
        VAULT_CONTRACT,
        { claim_unbonded_withdrawals: { withdrawal_ids: newFundedIds } },
        "auto"
      );
      console.log("Claimed:", result.transactionHash);
    } else {
      console.log("No funded withdrawals to claim yet.");
    }
  } else {
    // 3. Claim funded withdrawals
    const result = await client.execute(
      address,
      VAULT_CONTRACT,
      { claim_unbonded_withdrawals: { withdrawal_ids: fundedIds } },
      "auto"
    );
    console.log("Claimed:", result.transactionHash);
  }
}
```

### Query Vault State

```typescript
async function queryVaultState() {
  const { CosmWasmClient } = await import("@cosmjs/cosmwasm-stargate");
  const client = await CosmWasmClient.connect(RPC_ENDPOINT);

  // Pool info (shares issued, balances)
  const poolInfo = await client.queryContractSmart(VAULT_CONTRACT, {
    pool_info: {},
  });
  console.log("Pool info:", poolInfo);

  // Withdrawal queue status (note: fields are nested under .info)
  const queueResp = await client.queryContractSmart(VAULT_CONTRACT, {
    withdrawal_queue_info: {},
  });
  const queueInfo = queueResp.info;
  console.log("Queue info:", queueInfo);

  // How much a given number of shares is worth
  const value = await client.queryContractSmart(VAULT_CONTRACT, {
    shares_equivalent_value: { shares: "1000000" },
  });
  console.log("1 share worth:", value, "micro-tokens");

  // Control Center pool info (global across all sub-vaults)
  const CONTROL_CENTER = "neutron1vk3cy35cudlpk8w9kuu9prcanc49n3ajcnu86a43ue9ln6v4v6zsaucnw9";
  const ccPoolInfo = await client.queryContractSmart(CONTROL_CENTER, {
    pool_info: {},
  });
  console.log("Global pool value:", ccPoolInfo.total_pool_value);
  console.log("Global shares issued:", ccPoolInfo.total_shares_issued);
}
```
