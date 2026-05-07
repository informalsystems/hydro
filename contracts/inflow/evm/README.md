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

### Testing

```bash
forge test
```
