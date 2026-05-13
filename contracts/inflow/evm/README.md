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
