# Inflow EVM smart contracts
This folder contains various smart contract intended to be deployed on EVM compatible blockchains.

## Prerequisites

The following tools must be available on the machine used to compile, test, or deploy:

| Tool | Purpose |
|---|---|
| [Foundry](https://book.getfoundry.sh/getting-started/installation) | Compilation, testing, and deployment scripting |

After cloning, install Foundry libraries:

```bash
forge install \
  foundry-rs/forge-std \
  OpenZeppelin/openzeppelin-contracts@v5.6.1 \
  OpenZeppelin/openzeppelin-contracts-upgradeable@v5.6.1 \
  --no-git
```

## CCTP USDC Forwarder
This smart contract will be used as a temporary holder of USDC tokens on EVM chains, until we bridge those tokens to Neutron chain for Inflow USDC vault deployment. There will be an off-chain component which will monitor balance changes of this contract and, once the contract has certain amount of USDC tokens, it will initiate the bridging request.
Constructor parameters:
- `cctpContract`- address of the CCTP protocol contract used to initiate the bridging request.
- `destinationDomain`- CCTP domain ID of Noble blockchain, since we will perform bridging to Neutron over Noble.
- `tokenToBridge`- address of the USDC ERC-20 smart contract.
- `recipient` - recipient address on Noble blockchain, encoded as a hexadecimal value into Solidity bytes32 type. This will be an address of a Noble Forwarding Account which will be created at the same time tokens are minted on Noble blockchain. By leveraging the Forwarding Accounts we will be able to bridge tokens from EVM chain to Neutron in a single transaction.
- `destinationCaller`- address of a Noble blockchain relayer encoded as a hexadecimal value into Solidity bytes32 type.
- `operator`- address controlled by our off-chain tool that will be allowed to execute permissioned actions against the contract.
- `admin`- address that can pause any execution on the contract in case of emergency.
- `operationalFeeBps` - determines how many tokens will be deducted from the bridging amount, expressed in basis points (i.e. 1% = 100 basis points).
- `minOperationalFee` - minimal operational fee that will be charged in case that computed value is below this value.

*Note: Setting both `operationalFeeBps` and `minOperationalFee` to zero means that no operational fees will be charged for bridging (i.e. the operator wallet will cover the expense of submitting transactions on EVM chain).

## InflowVault

An upgradeable ERC-4626 tokenised vault that holds a single ERC-20 asset. It supports adapter-based external deployment of idle funds, a two-phase FIFO withdrawal queue, and a high-water-mark performance fee system.

Upgradeability uses the UUPS proxy pattern (EIP-1822): the proxy is a thin forwarder and upgrade authorisation lives in the implementation, guarded by the vault's whitelist.

### Deployment

The deploy script deploys the implementation `InflowVault` contract and an `ERC1967Proxy` that wraps it, then calls `initialize()` through the proxy in a single broadcast.

**Required environment variables**

| Variable | Description |
|---|---|
| `ASSET` | ERC-20 token address accepted as deposit |
| `VAULT_NAME` | Share token name (e.g. `"inflow_usdc_share"`) |
| `VAULT_SYMBOL` | Share token symbol (e.g. `"inflow_usdc_share"`) |
| `DEPOSIT_CAP` | Maximum total assets the vault will hold, in token base units |
| `MAX_WITHDRAWALS_PER_USER` | Maximum concurrent queued withdrawals per address |
| `INITIAL_ADMIN` | Address added to the whitelist at initialisation |
| `PRIVATE_KEY` | Private key used to sign transactions |
| `RPC_URL` | RPC endpoint of a node used to broadcast transactions |

**Optional environment variables**

| Variable | Description |
|---|---|
| `INITIAL_DEPLOYED_AMOUNT_ADMIN` | Address added to the deployed-amount whitelist at initialisation; defaults to `INITIAL_ADMIN` |
| `FEE_RATE` | Performance fee rate in WAD — `0` disables fees, `1e18` = 100% |
| `FEE_RECIPIENT` | Recipient of accrued fee shares; required when `FEE_RATE > 0` |

```bash
export ASSET=0x3600000000000000000000000000000000000000   # USDC on Arc
export VAULT_NAME="inflow_usdc_share"
export VAULT_SYMBOL="inflow_usdc_share"
export DEPOSIT_CAP=1000000000000                         # 1 000 000 USDC (6 decimals)
export MAX_WITHDRAWALS_PER_USER=10
export INITIAL_ADMIN=0xYourAdminAddress
export PRIVATE_KEY=0xYourAdminPrivateKey
export RPC_URL=https://rpc.testnet.arc.network

forge script script/DeployInflowVault.s.sol \
  --rpc-url $RPC_URL --broadcast --private-key $PRIVATE_KEY -vvvv
```

The script prints the proxy and implementation addresses on completion.

Alternatively, a shell script is provided that loads `.env` automatically, accepts `PRIVATE_KEY` or `MNEMONIC`, and sets sensible Arc testnet defaults for all optional variables:

```bash
bash bin/deploy_inflow_vault.sh
```

### Adapters

Two adapter implementations are provided. `BasicInflowAdapter` is a minimal reference/example for testing vault ↔ adapter integration and for building real adapters. `ReserveAdapter` is the first adapter built for genuine production use: a shared backstop reserve used for APY smoothing. See the comparison table under `ReserveAdapter` below for how they differ.

### BasicInflowAdapter

A minimal `IAdapter` implementation that holds tokens directly without deploying them to an external protocol. Useful for testing vault ↔ adapter integration and as a reference for building real adapters.

Enforces **one depositor per asset**: each token is exclusively owned by a single registered depositor, so `depositorPosition()` is always exact rather than a shared-pool total. `registerDepositor`'s `metadata` argument must be `abi.encode(tokenAddress)`, the asset that depositor will exclusively hold; a second depositor may register for a *different* token on the same deployment, but never for a token another depositor already holds. Always registered as **untracked**; combined with per-asset exclusivity, this is what prevents two vaults with the same underlying asset from double-counting the same balance in `totalAssets()`.

**Required environment variables**

| Variable | Description |
|---|---|
| `VAULT_ADDRESS` | Proxy address of the vault to register the adapter on |
| `PRIVATE_KEY` or `MNEMONIC` | Signing key: provide one or the other |
| `RPC_URL` | RPC endpoint of a node used to broadcast transactions |

**Optional environment variables**

| Variable | Description |
|---|---|
| `ADAPTER_ADMIN` | Address granted admin rights on the adapter; defaults to the deployer |

Or use the shell script (reads `.env` automatically):

```bash
bash bin/02_deploy_basic_adapter.sh
```

### ReserveAdapter

A shared backstop reserve that one or more vaults draw on for APY smoothing: funds move into the reserve when a vault's raw yield for a period is above target, and move back out when it's below target, so the vault's reported growth stays steadier over time. Unlike `BasicInflowAdapter`, the shared-pool model (no per-depositor accounting) is intentional here: multiple vaults are meant to draw on the same pool for the same asset.

| | BasicInflowAdapter | ReserveAdapter |
|---|---|---|
| Tracking mode | Untracked only | Tracked only |
| Per-asset exclusivity | One depositor per token | None (shared pool, multiple depositors per token) |
| `depositorPosition()` | Real balance for the depositor's own token, else 0 | Always hardcoded `0` |
| Disconnect (`vault.unregisterAdapter`) | Blocked while real balance > 0 | Always succeeds instantly, regardless of balance |
| Intended use | Reference/example, testing | Production APY-smoothing reserve |

**Required environment variables**

| Variable | Description |
|---|---|
| `VAULT_ADDRESS` | Proxy address of the vault to register the adapter on |
| `PRIVATE_KEY` or `MNEMONIC` | Signing key — provide one or the other |
| `RPC_URL` | RPC endpoint of a node used to broadcast transactions |

**Optional environment variables**

| Variable | Description |
|---|---|
| `ADAPTER_ADMIN` | Address granted admin rights on the adapter; defaults to the deployer |

Or use the shell script (reads `.env` automatically):

```bash
bash bin/06_deploy_reserve_adapter.sh
```

#### Reserve fund-movement runbook

`ReserveAdapter` is registered tracked, but its funds must never be allowed to persist in the vault's reported `deployedAmount`. Parked reserve funds should not affect the share ratio while sitting idle, in either direction. This isn't enforced on-chain; it requires the following sequence for every reserve transfer:

1. Once the period's raw tracked-venue value is known (e.g. the verifier-approved `submitDeployedAmount` value for that period), compute the target smoothing delta.
2. Call `depositToAdapter("reserve", amount)` (raw growth above target) or `withdrawFromAdapter("reserve", amount)` (raw growth below target). Because the adapter is tracked, this automatically bumps or reduces `deployedAmount` by `amount`. That adjustment is unconditional, built into the vault's adapter library, not something `ReserveAdapter` itself controls.
3. Immediately follow with a `submitDeployedAmount` call computed to cancel that automatic adjustment back out, so the submitted value reflects only the true, unsmoothed value of the real tracked venues, never including the reserve.

Skipping step 3, or computing it wrong, will leave the reserve's balance incorrectly inflating or deflating the vault's reported `totalAssets()` until corrected.

#### Unregistering an adapter: runbook

Vault-side (`vault.unregisterAdapter`) and adapter-side (`adapter.unregisterDepositor`) registrations are independent; neither call affects the other. Fully disconnecting a vault from an adapter requires both, **in this order**:

1. Drain funds: `vault.withdrawFromAdapter(name, fullAmount)`.
2. `vault.unregisterAdapter(name)`, checked while the vault is still a registered depositor on the adapter.
3. `adapter.unregisterDepositor(vaultAddress)` for adapter-side cleanup.

Doing step 3 before step 1 locks the vault out of its own funds (the adapter's depositor gating blocks it once unregistered). Doing step 3 before step 2 can also silently defeat the vault-side check on `BasicInflowAdapter`, since `depositorPosition()` returns 0 for a depositor no longer registered for that token.

For `ReserveAdapter`, the vault-side guard offers **no protection at all**: `vault.unregisterAdapter` always succeeds instantly regardless of real balance. Reconciling real funds and `deployedAmount` (per the runbook above) before disconnecting is purely an operational responsibility.

### End-to-end deposit test

After deploying both the vault and the adapter, verify the full deposit → adapter routing → redeem flow:

```bash
bash bin/test_deposit_flow.sh
```

Reads `VAULT_ADDRESS`, `ADAPTER_ADDRESS`, `PRIVATE_KEY` (or `MNEMONIC`), and optionally `DEPOSIT_AMOUNT` / `SKIP_REDEEM` from `.env`. See `.env.example` for all variables.

### Upgrade

The upgrade script deploys a new implementation contract and calls `upgradeToAndCall` on the existing proxy. The signing wallet must be whitelisted on the vault (`_authorizeUpgrade()` in InflowVault enforces this). Upgrades support execution of functions in the new version of `InflowVault` contract that are marked with `reinitializer` modifier. These `reinitializer` functions allow setting the new smart contract version, as well as initialization of newly introduced smart contract fields. An example of such function would be:

```bash
function reinitializeV2(string calldata initialValue) external reinitializer(2) {
  _newStringField = initialValue;
}
```

**Required environment variables**

| Variable | Description |
|---|---|
| `PROXY` | Address of the existing `ERC1967Proxy` |
| `PRIVATE_KEY` | Private key used to sign transactions |
| `RPC_URL` | RPC endpoint of a node used to broadcast transactions |

**Optional environment variables**

| Variable | Description |
|---|---|
| `MIGRATION_DATA` | ABI-encoded calldata forwarded to `upgradeToAndCall` — use this to invoke a `reinitializer` function on the new implementation. Defaults to empty (no migration call). |

```bash
# Upgrade with no migration call
export PROXY=0xYourProxyAddress
export PRIVATE_KEY=0xYourAdminPrivateKey
export RPC_URL=https://rpc.testnet.arc.network

forge script script/UpgradeInflowVault.s.sol \
  --rpc-url $RPC_URL --broadcast --private-key $PRIVATE_KEY -vvvv
```

```bash
# Upgrade and call a reinitializer on the new implementation
export PROXY=0xYourProxyAddress
export MIGRATION_DATA=$(cast calldata "reinitializeV2(string)" "initial field value")
export PRIVATE_KEY=0xYourAdminPrivateKey
export RPC_URL=https://rpc.testnet.arc.network

forge script script/UpgradeInflowVault.s.sol \
  --rpc-url $RPC_URL --broadcast --private-key $PRIVATE_KEY -vvvv
```

> **Note:** Storage layout compatibility between implementation versions must be verified manually before upgrading.

### Testing

```bash
forge test
```

Unit tests are split across two files:
- `test/InflowVaultCancelWithdrawal.t.sol` — withdrawal queue and cancellation scenarios
- `test/InflowVaultAdapters.t.sol` — adapter pull-pattern and multi-token fund movement

## Before committing

Run all three commands from the `contracts/inflow/evm/` directory:

```bash
forge fmt && forge lint && forge test
```

`forge lint` will always emit one unfixable note:

```
note[mixed-case-function]: contracts/CCTPUSDCForwarder.sol — requestCCTPTransferWithCaller
```

This is expected. The function is declared in `ICCTPBridge`, an interface that mirrors an
external contract's ABI. The name cannot be changed without breaking the call. Forge lint
does not support per-line suppression, so the note cannot be silenced without disabling the
rule globally. Ignore it.
