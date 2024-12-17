set -eux

if [ "$#" -ne 2 ]; then
    echo "Usage: $0 HYDRO_CONTRACT_ADDRESS NUM_OF_VALIDATORS"
    exit 1
fi

HYDRO_CONTRACT_ADDRESS=$1
NUM_OF_VALIDATORS=$2

export RELAYER_REGISTRY_ADDRESSES=$HYDRO_CONTRACT_ADDRESS
export NUM_VALIDATORS_TO_ADD=$NUM_OF_VALIDATORS

# populate the config for the ICQ relayer and the ICQ query creation
export NEUTRON_CHAIN_ID="neutron-1"
export RELAYER_NEUTRON_CHAIN_RPC_ADDR=https://neutron-rpc.publicnode.com:443
export RELAYER_NEUTRON_CHAIN_REST_ADDR=https://neutron-rest.publicnode.com
export RELAYER_NEUTRON_CHAIN_HOME_DIR=$HOME/.neutrond
export RELAYER_NEUTRON_CHAIN_SIGN_KEY_NAME=submitter
export RELAYER_NEUTRON_CHAIN_KEYRING_BACKEND=test
export RELAYER_NEUTRON_CHAIN_DENOM=untrn
export RELAYER_NEUTRON_CHAIN_GAS_PRICES=0.0055untrn
export RELAYER_NEUTRON_CHAIN_GAS_PRICE_MULTIPLIER=2  
export RELAYER_NEUTRON_CHAIN_MAX_GAS_PRICE=0.011  
export RELAYER_NEUTRON_CHAIN_GAS_ADJUSTMENT=1.5  
export RELAYER_NEUTRON_CHAIN_CONNECTION_ID=connection-0
export RELAYER_NEUTRON_CHAIN_OUTPUT_FORMAT=json  
export RELAYER_TARGET_CHAIN_RPC_ADDR=https://cosmos-rpc.publicnode.com:443
export RELAYER_TARGET_CHAIN_API_ADDR=https://cosmos-api.polkachu.com/

# maximum number of validator queries to submit in a single block.
# lower this if you get errors about exceeding the max block size
export BATCH_SIZE=30

#####
# typically, no need to modify these  
export RELAYER_ALLOW_TX_QUERIES=false  
export RELAYER_ALLOW_KV_CALLBACKS=true  
export RELAYER_STORAGE_PATH=$HOME/.neutron_queries_relayer/leveldb
export LOGGER_LEVEL=debug
#####

# Create the ICQ queries by running the go script in this folder
icq-tool

# Run the relayer
neutron_query_relayer start