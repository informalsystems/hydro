## Issuing interchain queries and relaying

### Neutron Interchain Queries (ICQ) Relayer Setup

Clone the [ICQ Relayer](https://github.com/neutron-org/neutron-query-relayer) repository and switch to the latest tag (v0.3.0 at the time of writing). Adjust the path in the `relaying.sh` file to point to the cloned repository.

### Setting up variables

Set up the variables by modifying the exports in `relaying.sh`.

## Run the script

Simply run the script by running `./relaying.sh HYDRO_CONTRACT_ADDRESS NUM_OF_VALIDATORS_TO_ADD`.

It will not stop on its own, but the script will eventually print out the relayer logs, and once there is no more regular change in those, you can stop the script.

