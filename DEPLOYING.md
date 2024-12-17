# Manually deploying & interacting with contracts

This directory contains a set of shell scripts and JSON configuration files that allow us to easily store, instantiate and prepare our smart contracts for testing.

## Setting up the environment

To get started, you will need to have the following installed:
* [Docker](https://docs.docker.com/get-docker/)

You will need to create and prepare the `.seed` file in the repo root with the seed phrase of the test account; see the `.seed.example` file for an example.

This account will deploy the contracts, and send the transactions to populate the contracts.

Then, you need to build the dockerfile. Run the following command:

```bash
docker build -t hydro-docker .
```

## Setting up contracts

To deploy a new set of contracts to mainnet, run the following command:

```bash
docker run hydro-docker ./tools/deployment/setup_on_mainnet.sh
```

Take note of the contract addresses outputted by the script. You will need them to interact with the contracts.

## Populating contracts

If you already have contracts deployed, you can populate them with a new series of bids and tributes by running

```bash
docker run hydro-docker ./tools/deployment/populate_contracts.sh "tools/deployment/config_mainnet.json" $HYDRO_CONTRACT_ADDRESS $TRIBUTE_CONTRACT_ADDRESS
```
where you will need to replace the contract addresses with the addresses of the contracts you wish to populate.

This will create 3 bids with tributes in the current round of the contract.

Notice that the `.seed` file will need to contain the passphrase of the account that created the contracts.
> **TIP:** When you switch out the phrase in the `.seed` file, you will need to rebuild the docker image.

## Adding liquidity deployments

To make rewards claimable, liquidity deployments need to be added.
For a certain proposal in a certain round and certain tranche, here is how you can add a liquidity deployment for it, to make tributes claimable or refundable:

```bash
docker run hydro-docker ./tools/deployment/add_liquidity_deployments.sh "./tools/deployment/config_mainnet.json" $HYDRO_CONTRACT_ADDRESS $TRIBUTE_CONTRACT_ADDRESS $ROUND_ID $TRANCHE_ID $PROPOSAL_ID $FUNDS
```
FUNDS should be 0 if the tribute for the bid should become refundable; and non-zero if it should become claimable.
Don't worry about the non-zero number - this script isn't actually sending funds over. It only matters whether the number is zero or not.

## Issuing interchain queries and relaying

### Setting the Neutron Interchain Queries (ICQ) Relayer locally (optional)

If you want to run the relayer locally instead of through Docker, clone the [ICQ Relayer](https://github.com/neutron-org/neutron-query-relayer) repository and switch to the latest tag (v0.3.0 at the time of writing), then build the relayer.
Make sure that `neutron_query_relayer` is in the path and executable. 

### Setting up the ICQ population tool

To set up the ICQ population tool, inside `./tools/`, run this command to build the binary:

```bash
go build -o build/icq-tool ./*.go
```

### Setting up variables (optional)

You can adjust parameters like the RPC nodes to connect to for relaying by modifying the exports in `./tools/relaying.sh`.

## Run the script

Simply run the script by running `docker run hydro-docker ./tools/relaying.sh HYDRO_CONTRACT_ADDRESS NUM_OF_VALIDATORS_TO_ADD`.

It will not stop on its own, but the script will eventually print out the relayer logs, and once there are no more regular changes in those, you can stop the container.

