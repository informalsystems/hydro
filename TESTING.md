# End-to-end testing


## CW Orchestrator

We are using the cw-orchestrator (see [here](https://github.com/AbstractSDK/cw-orchestrator) and [here](https://orchestrator.abstract.money/intro.html)). The cw-orchestrator requires an interface to be created for each smart contract that we want to test. Interfaces for Hydro smart contracts are located in [packages/interface](./packages/interface/) directory. For more details on how to write a cw-orchestrator smart contract interface see [here](https://orchestrator.abstract.money/contracts/interfaces.html).

The cw-orchestrator also requires our smart contracts compiled as .wasm files to be present in the artifacts directory. To build the smart contracts for wasm platform use `make compile-rust-optimizer` command.

Once we have our contracts interfaces ready and .wasm files compiled, we can use them to write end-to-end tests. Tests are placed in [test/e2e](./test/e2e/) directory.

## Interchain Test

We also write end-to-end tests using [Interchain Test](https://github.com/strangelove-ventures/interchaintest).
These are placed in the [test/interchain](./test/interchain) directory, see [hydro_test.go](./test/interchain/hydro_test.go).
