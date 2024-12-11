#!/bin/bash
set -eux

source tools/deployment/store_instantiate.sh "tools/deployment/config_mainnet.json" false
source tools/deployment/populate_contracts.sh "tools/deployment/config_mainnet.json" $HYDRO_CONTRACT_ADDRESS $TRIBUTE_CONTRACT_ADDRESS

echo "Hydro contract address: $HYDRO_CONTRACT_ADDRESS"
echo "Tribute contract address: $TRIBUTE_CONTRACT_ADDRESS"