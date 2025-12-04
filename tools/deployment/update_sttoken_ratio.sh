set -eux

if [ "$#" -ne 2 ]; then
    echo "Usage: $0 <config_file> <ST_TOKEN_CONTRACT_ADDRESS>"
    exit 1
fi

CONFIG_FILE="$1"
ST_TOKEN_CONTRACT_ADDRESS=$2

NEUTRON_CHAIN_ID=$(jq -r '.chain_id' $CONFIG_FILE)
NEUTRON_RPC_NODE=$(jq -r '.neutron_rpc_node' $CONFIG_FILE)
NEUTRON_API_NODE=$(jq -r '.neutron_api_node' $CONFIG_FILE)
TX_SENDER_WALLET=$(jq -r '.tx_sender_wallet' $CONFIG_FILE)

NEUTRON_BINARY="neutrond"
NEUTRON_CHAIN_ID_FLAG="--chain-id $NEUTRON_CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend test"
TX_FLAG="--gas auto --gas-adjustment 1.3"
NEUTRON_RPC_NODE_FLAG="--node $NEUTRON_RPC_NODE"
NEUTRON_TX_FLAGS="$TX_FLAG --gas-prices 0.0053untrn $NEUTRON_CHAIN_ID_FLAG $NEUTRON_RPC_NODE_FLAG $KEYRING_TEST_FLAG -y"

# Check if the Interchain Query is already created
QUERY_MSG='{"interchain_query_info":{}}'
RESPONSE=$($NEUTRON_BINARY query wasm contract-state smart "$ST_TOKEN_CONTRACT_ADDRESS" "$QUERY_MSG" $NEUTRON_RPC_NODE_FLAG --output json)

# If not, execute the transaction to create one
if [ "$(echo "$RESPONSE" | jq '.data.info == null')" = "true" ]; then
    echo "Creating Interchain Query for stTOKEN ratio update..."

    EXECUTE='{"register_host_zone_icq":{}}'
    $NEUTRON_BINARY tx wasm execute $ST_TOKEN_CONTRACT_ADDRESS "$EXECUTE" --from $TX_SENDER_WALLET --amount 1000000untrn $NEUTRON_TX_FLAGS
    sleep 10
fi

export RELAYER_REGISTRY_ADDRESSES=$ST_TOKEN_CONTRACT_ADDRESS
# populate the config for the ICQ relayer and the ICQ query creation
export NEUTRON_CHAIN_ID=$NEUTRON_CHAIN_ID
export RELAYER_NEUTRON_CHAIN_RPC_ADDR=$NEUTRON_RPC_NODE
export RELAYER_NEUTRON_CHAIN_REST_ADDR=$NEUTRON_API_NODE
export RELAYER_NEUTRON_CHAIN_HOME_DIR=$HOME/.neutrond
export RELAYER_NEUTRON_CHAIN_SIGN_KEY_NAME=$TX_SENDER_WALLET
export RELAYER_NEUTRON_CHAIN_KEYRING_BACKEND=test
export RELAYER_NEUTRON_CHAIN_DENOM=untrn
export RELAYER_NEUTRON_CHAIN_GAS_PRICES=0.0055untrn
export RELAYER_NEUTRON_CHAIN_GAS_PRICE_MULTIPLIER=2  
export RELAYER_NEUTRON_CHAIN_MAX_GAS_PRICE=0.011  
export RELAYER_NEUTRON_CHAIN_GAS_ADJUSTMENT=1.5  
export RELAYER_NEUTRON_CHAIN_CONNECTION_ID=connection-15
export RELAYER_NEUTRON_CHAIN_OUTPUT_FORMAT=json  
export RELAYER_TARGET_CHAIN_RPC_ADDR=https://stride-rpc.polkachu.com:443
export RELAYER_TARGET_CHAIN_API_ADDR=https://stride-api.polkachu.com

#####
# typically, no need to modify these  
export RELAYER_ALLOW_TX_QUERIES=false  
export RELAYER_ALLOW_KV_CALLBACKS=true  
export RELAYER_STORAGE_PATH=$HOME/.neutron_queries_relayer/leveldb
export LOGGER_LEVEL=debug
#####

# Run the relayer
neutron_query_relayer start
