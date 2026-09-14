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

## Environments and deployment

Deployments are driven by the scripts in `bin/`. There is one script per step and none
per environment: the target is chosen with `--env <target>`, which makes the script read
`.env.<target>` (or plain `.env` when `--env` is omitted) and nothing else. Staging and
production therefore run identical code, and a staging run cannot pick up a production
value.

Copy `.env.example` to `.env.<target>` and fill it in. Every `.env*` file except the
example is gitignored.

| Step | Script | Purpose |
|---|---|---|
| 1 | `bin/01_deploy_inflow_vault.sh` | Deploy the vault implementation and proxy |
| 2 | `bin/02_deploy_reserve_adapter.sh` | Deploy ReserveAdapter |
| 3 | `bin/03_safe_tx_link_adapter.sh` | Generate the Safe batch that links adapter and vault |
| 4 | `bin/04_safe_tx_approve_vault.sh` | Generate the Safe batch approving the vault to pull the asset |
| 5 | `bin/05_test_deposit_flow.sh` | Deposit and redeem against a live deployment, or seed it with `SKIP_REDEEM=true` |
| 6 | `bin/06_verify_contracts.sh` | Submit a deployed stack for explorer verification |

Steps 3 and 4 produce batches rather than transactions because the vault's privileged
roles are held by Safes, not by the deployer. See [Role split](#role-split) below.

Common flags: `--env <target>`, `--dry-run` (simulate without broadcasting), `--yes`
(skip the confirmation prompt). Shared helpers live in `bin/lib/common.sh`.

Two guards apply to every script. `EXPECTED_CHAIN_ID` must match the chain the RPC
reports, and any address that will sign the vault handshake must already be
vault-whitelisted. Both are checked before a transaction is broadcast.

Safe batches are written to `out/safe-tx/<target>-*.json`, which is gitignored: they are
per-deployment artifacts, reproducible from the script plus the config file. Import them
in the Safe UI under Apps then Transaction Builder.

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

Alternatively, the shell wrapper reads every variable above from the target's config file,
resolves the signer, checks the chain, and prints a summary before broadcasting:

```bash
./bin/01_deploy_inflow_vault.sh --env staging
```

### BasicInflowAdapter

A minimal `IAdapter` implementation that holds tokens directly without deploying them to an external protocol. Useful for testing vault ↔ adapter integration and as a reference for building real adapters.

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

There is no `bin/` wrapper for this adapter. It is a reference implementation and a test
fixture, not something we deploy: `script/DeployBasicAdapter.s.sol` wires the vault from
the deployer EOA in the same broadcast, which only works where that EOA is
vault-whitelisted. ReserveAdapter below is the adapter we actually run, and its two-step
Safe flow is the model to copy for any new one.

### ReserveAdapter

A shared backstop reserve that one or more vaults draw on to smooth reported APY. Unlike
BasicInflowAdapter it keeps a single shared pool with no per-depositor accounting, since
several vaults holding the same asset are meant to draw on one reserve.

> **Operational contract.** The adapter is registered with `tracked = true`, so the vault
> adjusts `deployedAmount` by the moved amount on every transfer to or from the reserve.
> Parked reserve funds must not count toward the share ratio in either direction, so each
> transfer **must** be followed by a `submitDeployedAmount` call that cancels that
> adjustment back out. Nothing enforces this on-chain. `depositorPosition()` is hardcoded
> to `0`, which keeps the adapter always disconnectable but also means
> `unregisterAdapter`'s position guard offers no protection here. Read the NatSpec in
> `contracts/ReserveAdapter.sol` before operating it.

#### Role split

The vault keeps two independent whitelists, and our deployments put a separate Safe behind
each. That separation is what shapes the deployment flow:

| Role | Held by | Gates |
|---|---|---|
| `whitelist` | `ADMIN_SAFE` | `registerAdapter`, `unregisterAdapter`, `depositToAdapter`, `withdrawFromAdapter`, `withdrawForDeployment`, `depositFromDeployment`, `updateDepositCap`, `updateFeeConfig`, upgrades, and membership of both whitelists |
| `deployedAmountWhitelist` | `SUBMIT_SAFE` | `submitDeployedAmount` only |

So the Safe that moves funds cannot also restate what the vault is worth. That matters
most for ReserveAdapter, whose compensating `submitDeployedAmount` is exactly the call on
the other side of the split.

Three consequences, which are the reason steps 3 and 4 exist:

- The deployer EOA is in neither whitelist. It only pays gas, so it cannot call
  `registerAdapter` and cannot wire the adapter it just deployed. `WIRE_MODE=safe` leaves
  the handshake to `bin/03`, which emits a batch for `ADMIN_SAFE` to execute.
- `depositFromDeployment` pulls the asset with `safeTransferFrom(asset, msg.sender, ...)`,
  and `msg.sender` is `ADMIN_SAFE`. The Safe must therefore approve the vault once, which
  is the batch `bin/04` emits.
- `SUBMIT_SAFE` needs no deployment step. It signs `submitDeployedAmount` from the Safe UI
  during operation, after every reserve transfer.

#### Deploying

Deploy, then execute the two batches from `ADMIN_SAFE`:

```bash
./bin/02_deploy_reserve_adapter.sh --env staging --dry-run   # simulate first
./bin/02_deploy_reserve_adapter.sh --env staging

# then generate the Safe batches and execute them from ADMIN_SAFE
./bin/03_safe_tx_link_adapter.sh --env staging <VAULT_ADDRESS> <ADAPTER_ADDRESS>
./bin/04_safe_tx_approve_vault.sh --env staging
```

Set `WIRE_MODE=deployer` in the config file to deploy and wire in a single broadcast
instead, which works only where the deployer EOA is itself vault-whitelisted.

**Required environment variables**

| Variable | Description |
|---|---|
| `VAULT_ADDRESS` | Proxy address of the vault to attach to |
| `EXPECTED_CHAIN_ID` | Chain the deploy is intended for; a mismatch aborts |
| `ADMIN_SAFE` | Adapter admin, and the vault-whitelisted signer of the batches from steps 3 and 4 |
| `PRIVATE_KEY`, `MNEMONIC` or `ACCOUNT_NAME` | Signing key, one of the three |

**Optional environment variables**

| Variable | Description |
|---|---|
| `WIRE_MODE` | `safe` (default) or `deployer` |
| `ADAPTER_NAME` | Vault-side adapter name; defaults to `reserve` |
| `ADAPTER_ADMIN` | Overrides `ADMIN_SAFE` as the initial adapter admin |

### End-to-end deposit test

After deploying both the vault and the adapter, verify the full deposit → adapter routing → redeem flow:

```bash
./bin/05_test_deposit_flow.sh --env staging
```

Reads `VAULT_ADDRESS`, the signing key, and optionally `ADAPTER_ADDRESS`, `DEPOSIT_AMOUNT`,
`SKIP_REDEEM` and `EXPECTED_DEPLOYER` from the target's config file. With `SKIP_REDEEM=true`
it stops after the deposit, which is how you seed a vault before rehearsing the withdraw
and deposit-for-deployment flow. See `.env.example` for all variables.

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

Unit tests live in `test/`, one file per area: vault initialisation, configuration, access
control, deposits, withdrawals, the withdrawal queue, cancellation, fees, adapters,
upgrades, and `test/ReserveAdapter.t.sol` for the reserve adapter.

Every test must pass against a plain checkout with no network access. Tests that need
live chain state belong behind a fork configuration, not in the default suite.

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
