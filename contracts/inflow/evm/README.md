# Inflow EVM smart contracts
This folder contains various smart contract intended to be deployed on EVM compatible blockchains and used to bridge the gap to our Inflow smart contracts deployed on Neutron blockchain.

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

### Compiling
To compile the contracts located in `contracts` folder, you will need to install `nodejs`, `npm` and `hardhat`. Then run the following command: `npx hardhat compile`. The output will be stored in `artifacts/contracts` folder.

## InflowVault

An upgradeable ERC-4626 tokenised vault that holds a single ERC-20 asset. It supports adapter-based external deployment of idle funds, a two-phase FIFO withdrawal queue, and a high-water-mark performance fee system.

Upgradeability uses the UUPS proxy pattern (EIP-1822): the proxy is a thin forwarder and upgrade authorisation lives in the implementation, guarded by the vault's whitelist.

### Prerequisites

The following tools must be available on the machine used to compile, test, or deploy:

| Tool | Purpose |
|---|---|
| [Node.js](https://nodejs.org/) ≥ 18 | Required by the OZ Upgrades plugin to run storage-layout safety checks at deploy time |
| [Foundry](https://book.getfoundry.sh/getting-started/installation) | Compilation, testing, and deployment scripting |

Install Node dependencies (OpenZeppelin contracts):

```bash
npm install
```

The `lib/openzeppelin-foundry-upgrades/` directory is committed to the repo and contains the [OpenZeppelin Foundry Upgrades](https://docs.openzeppelin.com/upgrades-plugins/foundry/foundry-upgrades) plugin. It does **not** need to be reinstalled unless you delete the `lib/` directory, in which case run:

```bash
forge install OpenZeppelin/openzeppelin-foundry-upgrades --no-git
```

### Testing

```bash
forge test
```

### Deploying

The deployment script at `script/DeployInflowVault.s.sol` uses the OZ Upgrades plugin. Before broadcasting, the plugin validates storage layout safety and confirms the contract is correctly configured for UUPS.

Set the required environment variables, then run:

```bash
export ASSET=<ERC-20 token address>
export VAULT_NAME="Hydro Inflow Vault"
export VAULT_SYMBOL="hvUSDC"
export DEPOSIT_CAP=<uint256, e.g. 1000000000000>
export MAX_WITHDRAWALS_PER_USER=10
export INITIAL_ADMIN=<whitelisted admin address>
# Optional:
export FEE_RATE=0          # WAD, 0 = disabled
export FEE_RECIPIENT=0x0   # required when FEE_RATE > 0

forge script script/DeployInflowVault.s.sol \
  --rpc-url <RPC_URL> \
  --sender <DEPLOYER_ADDRESS> \
  --broadcast
```

The script prints both the proxy address (the address callers interact with) and the implementation address.

### Upgrading

To upgrade to a new implementation, call `upgradeToAndCall` on the proxy from a whitelisted address (including the governing DAO). Before broadcasting, the OZ Upgrades plugin compares the new implementation's storage layout against the previous one and rejects any changes that would corrupt existing state.

`PROXY` is the address printed by the deploy script (the address callers interact with, not the implementation).

```bash
export PROXY=<proxy address from the deploy step>

forge script script/UpgradeInflowVault.s.sol \
  --rpc-url <RPC_URL> \
  --sender <WHITELISTED_ADDRESS> \
  --broadcast
```

The script prints the proxy address and the new implementation address once the upgrade is confirmed.
