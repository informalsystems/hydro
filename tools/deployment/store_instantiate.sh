#!/bin/bash
set -eux

CONFIG_FILE="$1"
IS_GITHUB_WORKFLOW=$2

NEUTRON_CHAIN_ID=$(jq -r '.chain_id' $CONFIG_FILE)
NEUTRON_NODE=$(jq -r '.neutron_rpc_node' $CONFIG_FILE)
TX_SENDER_WALLET=$(jq -r '.tx_sender_wallet' $CONFIG_FILE)
TX_SENDER_ADDRESS=$(neutrond keys show $TX_SENDER_WALLET --keyring-backend test | grep "address:" | sed 's/.*address: //')
HYDRO_TEST_ADDRESS="neutron1r6rv879netg009eh6ty23v57qrq29afecuehlm"
HUB_CONNECTION_ID=$(jq -r '.hub_connection_id' $CONFIG_FILE)
HUB_CHANNEL_ID=$(jq -r '.hub_channel_id' $CONFIG_FILE)

NEUTRON_BINARY="neutrond"
NEUTRON_CHAIN_ID_FLAG="--chain-id $NEUTRON_CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend test"
TX_FLAG="--gas auto --gas-adjustment 1.3"
NEUTRON_NODE_FLAG="--node $NEUTRON_NODE"
NEUTRON_TX_FLAGS="$TX_FLAG --gas-prices 0.0053untrn --chain-id $NEUTRON_CHAIN_ID $NEUTRON_NODE_FLAG $KEYRING_TEST_FLAG -y"

MAINNET_ROUND_LENGTH="2628000000000000" # 365 / 12
ROUND_END_TEST_ROUND_LENGTH="86400000000000" # 1 day

if [[ "$OSTYPE" == "darwin"* ]]; then
    # macOS
    CURRENT_TIME_NO_MINS_AND_SECS=$(date -j -f "%Y-%m-%d %H:%M:%S" "$(date +%Y-%m-%d\ %H:00:00)" +"%s000000000")
else
    # Linux
    CURRENT_TIME_NO_MINS_AND_SECS=$(date -d "$(date +"%Y-%m-%d %H:00:00")" +"%s000000000")
fi
SPECIFIC_TIMESTAMP=""

# these ones are used in the InstantiateMsg
ROUND_LENGTH=$ROUND_END_TEST_ROUND_LENGTH
FIRST_ROUND_START_TIME=$CURRENT_TIME_NO_MINS_AND_SECS
HYDRO_COMMITTEE_DAODAO="neutron1xd6z4nwmfeamv089fr9s4hlp3vq00l0tn9j9ysauc2j5pcmlm6vsk7nf7q"

MAX_DEPLOYMENT_DURATION=3
HYDRO_WASM_PATH="./artifacts/hydro.wasm"
TRIBUTE_WASM_PATH="./artifacts/tribute.wasm"
DAO_VOTING_ADAPTER_WASM_PATH="./artifacts/dao_voting_adapter.wasm"

HYDRO_CODE_ID=""
TRIBUTE_CODE_ID=""
DAO_VOTING_ADAPTER_CODE_ID=""

HYDRO_SC_LABEL="Hydro"
TRIBUTE_SC_LABEL="Tribute"

store_hydro() {
    error_handler() {
        echo "Content of store_hydro_res.json:"
        cat ./store_hydro_res.json
    }
    trap error_handler ERR

    echo 'Storing Hydro wasm...'

    $NEUTRON_BINARY tx wasm store $HYDRO_WASM_PATH --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS --output json &> ./store_hydro_res.json
    sleep 10

    STORE_HYDRO_TX_HASH=$(grep -o '{.*}' ./store_hydro_res.json | jq -r '.txhash')
    $NEUTRON_BINARY q tx $STORE_HYDRO_TX_HASH $NEUTRON_NODE_FLAG --output json &> ./store_hydro_tx.json
    HYDRO_CODE_ID=$(jq -r '.events[] | select(.type == "store_code") | .attributes[] | select(.key == "code_id") | .value' ./store_hydro_tx.json)
}

store_tribute() {
    error_handler() {
        echo "Content of store_tribute_res.json:"
        cat ./store_tribute_res.json
    }
    trap error_handler ERR

    echo 'Storing Tribute wasm...'

    $NEUTRON_BINARY tx wasm store $TRIBUTE_WASM_PATH --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS --output json &> ./store_tribute_res.json
    sleep 10

    STORE_TRIBUTE_TX_HASH=$(grep -o '{.*}' ./store_tribute_res.json | jq -r '.txhash')
    $NEUTRON_BINARY q tx $STORE_TRIBUTE_TX_HASH $NEUTRON_NODE_FLAG --output json &> ./store_tribute_tx.json
    TRIBUTE_CODE_ID=$(jq -r '.events[] | select(.type == "store_code") | .attributes[] | select(.key == "code_id") | .value' ./store_tribute_tx.json)
}

store_dao_voting_adapter() {
    error_handler() {
        echo "Content of store_dao_voting_adapter_res.json:"
        cat ./store_dao_voting_adapter_res.json
    }
    trap error_handler ERR

    echo 'Storing dao_voting_adapter wasm...'

    $NEUTRON_BINARY tx wasm store $DAO_VOTING_ADAPTER_WASM_PATH --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS --output json &> ./store_dao_voting_adapter_res.json
    sleep 10

    STORE_DAO_VOTING_ADAPTER_TX_HASH=$(grep -o '{.*}' ./store_dao_voting_adapter_res.json | jq -r '.txhash')
    $NEUTRON_BINARY q tx $STORE_DAO_VOTING_ADAPTER_TX_HASH $NEUTRON_NODE_FLAG --output json &> ./store_dao_voting_adapter_tx.json
    DAO_VOTING_ADAPTER_CODE_ID=$(jq -r '.events[] | select(.type == "store_code") | .attributes[] | select(.key == "code_id") | .value' ./store_dao_voting_adapter_tx.json)
}

instantiate_hydro() {
    error_handler() {
        echo "Content of instantiate_hydro_res.json:"
        cat ./instantiate_hydro_res.json
    }
    trap error_handler ERR

    echo 'Instantiating Hydro contract...'

    INIT_HYDRO='{"round_length":'$ROUND_LENGTH',"lock_epoch_length":'$ROUND_LENGTH', "tranches":[{"name": "ATOM Bucket", "metadata": "A bucket of ATOM to deploy as PoL"}, {"name": "USDC Bucket", "metadata": "This is a bucket for USDC from the Cosmos Hub community pool."}],"first_round_start":"'$FIRST_ROUND_START_TIME'","max_locked_tokens":"20000000000","whitelist_admins":["'$HYDRO_COMMITTEE_DAODAO'","'$TX_SENDER_ADDRESS'"],"initial_whitelist":["'$TX_SENDER_ADDRESS'"],"icq_managers":["'$TX_SENDER_ADDRESS'"],"round_lock_power_schedule": [[1, "1"], [2, "1.25"], [3, "1.5"], [6, "2"], [12, "4"]],"max_deployment_duration":'$MAX_DEPLOYMENT_DURATION',"token_info_providers":[{"lsm":{"max_validator_shares_participating":500,"hub_connection_id":"'$HUB_CONNECTION_ID'","hub_transfer_channel_id":"'$HUB_CHANNEL_ID'","icq_update_period":109000}}]}'

    $NEUTRON_BINARY tx wasm instantiate $HYDRO_CODE_ID "$INIT_HYDRO" --admin $TX_SENDER_ADDRESS --label "'$HYDRO_SC_LABEL'" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS --output json &> ./instantiate_hydro_res.json
    sleep 10

    INSTANTIATE_HYDRO_TX_HASH=$(grep -o '{.*}' ./instantiate_hydro_res.json | jq -r '.txhash')
    $NEUTRON_BINARY q tx $INSTANTIATE_HYDRO_TX_HASH $NEUTRON_NODE_FLAG --output json &> ./instantiate_hydro_tx.json
    export HYDRO_CONTRACT_ADDRESS=$(jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value' ./instantiate_hydro_tx.json)

    if $IS_GITHUB_WORKFLOW; then
        echo "HYDRO_CONTRACT_ADDRESS=$HYDRO_CONTRACT_ADDRESS" >> $GITHUB_ENV
    fi
}

instantiate_tribute() {
    error_handler() {
        echo "Content of instantiate_tribute_res.json:"
        cat ./instantiate_tribute_res.json
    }
    trap error_handler ERR
    echo 'Instantiating Tribute contract...'

    INIT_TRIBUTE='{"hydro_contract":"'$HYDRO_CONTRACT_ADDRESS'"}'

    $NEUTRON_BINARY tx wasm instantiate $TRIBUTE_CODE_ID "$INIT_TRIBUTE" --admin $TX_SENDER_ADDRESS --label "'$TRIBUTE_SC_LABEL'" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS --output json &> ./instantiate_tribute_res.json
    sleep 10

    INSTANTIATE_TRIBUTE_TX_HASH=$(grep -o '{.*}' ./instantiate_tribute_res.json | jq -r '.txhash')
    $NEUTRON_BINARY q tx $INSTANTIATE_TRIBUTE_TX_HASH $NEUTRON_NODE_FLAG --output json &> ./instantiate_tribute_tx.json
    export TRIBUTE_CONTRACT_ADDRESS=$(jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value' ./instantiate_tribute_tx.json)

    if $IS_GITHUB_WORKFLOW; then
        echo "TRIBUTE_CONTRACT_ADDRESS=$TRIBUTE_CONTRACT_ADDRESS" >> $GITHUB_ENV
    fi
}

store_hydro
store_tribute
store_dao_voting_adapter

echo 'Hydro code ID:' $HYDRO_CODE_ID
echo 'Tribute code ID:' $TRIBUTE_CODE_ID
echo 'DAO Voting Adapter code ID:' $DAO_VOTING_ADAPTER_CODE_ID

instantiate_hydro
instantiate_tribute

echo 'Hydro contract address:' $HYDRO_CONTRACT_ADDRESS
echo 'Tribute contract address:' $TRIBUTE_CONTRACT_ADDRESS

echo 'Contracts instantiated successfully!'
