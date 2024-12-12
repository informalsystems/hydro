## The Purpose

This directory contains a set of shell scripts and JSON configuration files that allow us to easily store, instantiate and prepare our smart contracts for testing.
These scripts are used in two places:
1. In the GitHub [workflow](./../../.github/workflows/deploy-latest-contracts.yml) that will store, instantiate and populate the smart contracts on the Neutron testnet each time a PR is merged into the main branch.
2. To manually deploy contracts on the Neutron mainnet.

### Shell scripts
- `store_instantiate.sh` sends transactions to store the codes from the `artifacts` directory to the specified blockchain. Then it uses stored codes to instantiate Hydro and Tribute smart contracts. It relies on a JSON configuration file that is passed to it. It populates and exports two environment variables: `HYDRO_CONTRACT_ADDRESS` and `TRIBUTE_CONTRACT_ADDRESS`.
- `populate_contracts.sh` sends transactions to create proposals in the Hydro smart contract, and to add tributes for those proposals in the Tribute smart contract. It relies on `HYDRO_CONTRACT_ADDRESS` and `TRIBUTE_CONTRACT_ADDRESS` being previously set by the `store_instantiate.sh` script.
- `setup_on_mainnet.sh` executes previous two scripts by providing `config_mainnet.json` configuration that will result in smart contracts being set up on the Neutron mainnet. Prerequisite for running this script is to have the `neutrond` binary in your `PATH` and to import mnemonic that has enough NTRN tokens on the Neutron mainnet. If this mnemonic is for a different address than the one in `config_mainnet.json` file, then the configuration file needs to be adjusted as well.