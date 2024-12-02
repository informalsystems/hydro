#!/bin/bash
set -eux

CONFIG_FILE="$1"
IS_GITHUB_WORKFLOW=$2

NEUTRON_CHAIN_ID=$(jq -r '.chain_id' $CONFIG_FILE)
NEUTRON_BINARY=$(jq -r '.binary_name' $CONFIG_FILE)
NEUTRON_NODE=$(jq -r '.neutron_rpc_node' $CONFIG_FILE)
TX_SENDER_WALLET=$(jq -r '.tx_sender_wallet' $CONFIG_FILE)
TX_SENDER_ADDRESS=$(jq -r '.tx_sender_address' $CONFIG_FILE)
HUB_CONNECTION_ID=$(jq -r '.hub_connection_id' $CONFIG_FILE)
HUB_CHANNEL_ID=$(jq -r '.hub_channel_id' $CONFIG_FILE)

NEUTRON_CHAIN_ID_FLAG="--chain-id $NEUTRON_CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend test"
TX_FLAG="--gas auto --gas-adjustment 1.3"
NEUTRON_NODE_FLAG="--node $NEUTRON_NODE"
NEUTRON_TX_FLAGS="$TX_FLAG --gas-prices 0.0053untrn --chain-id $NEUTRON_CHAIN_ID $NEUTRON_NODE_FLAG $KEYRING_TEST_FLAG -y"

MAINET_ROUND_LENGTH="2628000000000000" # 365 / 12
ROUND_END_TEST_ROUND_LENGTH="172800000000000" # 2 days

CURRENT_TIME_NO_MINS_AND_SECS=$(date -d "$(date +"%Y-%m-%d %H:00:00")" +"%s000000000")
SPECIFIC_TIMESTAMP=""

# these ones are used in the InstantiateMsg
ROUND_LENGTH=$ROUND_END_TEST_ROUND_LENGTH
FIRST_ROUND_START_TIME=$CURRENT_TIME_NO_MINS_AND_SECS
HYDRO_COMMITTEE_DAODAO="neutron1w7f40hgfc505a2wnjsl5pg35yl8qpawv48w5yekax4xj2m43j09s5fa44f"

IS_IN_PILOT_MODE=true
MAX_DEPLOYMENT_DURATION=3
HYDRO_WASM_PATH="./artifacts/hydro.wasm"
TRIBUTE_WASM_PATH="./artifacts/tribute.wasm"

HYDRO_CODE_ID=""
TRIBUTE_CODE_ID=""

HYDRO_SC_LABEL="Hydro V2.0.2- Round End Testing"
TRIBUTE_SC_LABEL="Tribute V2.0.2- Round End Testing"

store_hydro() {
    echo 'Storing Hydro wasm...'

    $NEUTRON_BINARY tx wasm store $HYDRO_WASM_PATH --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS --output json &> ./store_hydro_res.json
    sleep 10

    STORE_HYDRO_TX_HASH=$(grep -o '{.*}' ./store_hydro_res.json | jq -r '.txhash')
    $NEUTRON_BINARY q tx $STORE_HYDRO_TX_HASH $NEUTRON_NODE_FLAG --output json &> ./store_hydro_tx.json
    HYDRO_CODE_ID=$(jq -r '.events[] | select(.type == "store_code") | .attributes[] | select(.key == "code_id") | .value' ./store_hydro_tx.json)
}

store_tribute() {
    echo 'Storing Tribute wasm...'

    $NEUTRON_BINARY tx wasm store $TRIBUTE_WASM_PATH --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS --output json &> ./store_tribute_res.json
    sleep 10

    STORE_TRIBUTE_TX_HASH=$(grep -o '{.*}' ./store_tribute_res.json | jq -r '.txhash')
    $NEUTRON_BINARY q tx $STORE_TRIBUTE_TX_HASH $NEUTRON_NODE_FLAG --output json &> ./store_tribute_tx.json
    TRIBUTE_CODE_ID=$(jq -r '.events[] | select(.type == "store_code") | .attributes[] | select(.key == "code_id") | .value' ./store_tribute_tx.json)
}

instantiate_hydro() {
    echo 'Instantiating Hydro contract...'

    INIT_HYDRO='{"round_length":'$ROUND_LENGTH',"lock_epoch_length":'$ROUND_LENGTH', "tranches":[{"name": "ATOM Bucket", "metadata": "A bucket of ATOM to deploy as PoL"}],"first_round_start":"'$FIRST_ROUND_START_TIME'","max_locked_tokens":"20000000000","whitelist_admins":["'$HYDRO_COMMITTEE_DAODAO'","'$TX_SENDER_ADDRESS'"],"initial_whitelist":["'$TX_SENDER_ADDRESS'"],"max_validator_shares_participating":500,"hub_connection_id":"'$HUB_CONNECTION_ID'","hub_transfer_channel_id":"'$HUB_CHANNEL_ID'","icq_update_period":109000,"icq_managers":["'$TX_SENDER_ADDRESS'"],"is_in_pilot_mode":'$IS_IN_PILOT_MODE',"max_deployment_duration":'$MAX_DEPLOYMENT_DURATION'}'

    $NEUTRON_BINARY tx wasm instantiate $HYDRO_CODE_ID "$INIT_HYDRO" --admin $TX_SENDER_ADDRESS --label "'$HYDRO_SC_LABEL'" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS --output json &> ./instantiate_hydro_res.json
    sleep 10

    INSTANTIATE_HYDRO_TX_HASH=$(grep -o '{.*}' ./instantiate_hydro_res.json | jq -r '.txhash')
    $NEUTRON_BINARY q tx $INSTANTIATE_HYDRO_TX_HASH $NEUTRON_NODE_FLAG --output json &> ./instantiate_hydro_tx.json
    HYDRO_CONTRACT_ADDRESS=$(jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value' ./instantiate_hydro_tx.json)

    if $IS_GITHUB_WORKFLOW; then
        echo "HYDRO_CONTRACT_ADDRESS=$HYDRO_CONTRACT_ADDRESS" >> $GITHUB_ENV
    fi
}

instantiate_tribute() {
    echo 'Instantiating Tribute contract...'

    INIT_TRIBUTE='{"hydro_contract":"'$HYDRO_CONTRACT_ADDRESS'"}'

    $NEUTRON_BINARY tx wasm instantiate $TRIBUTE_CODE_ID "$INIT_TRIBUTE" --admin $TX_SENDER_ADDRESS --label "'$TRIBUTE_SC_LABEL'" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS --output json &> ./instantiate_tribute_res.json
    sleep 10

    INSTANTIATE_TRIBUTE_TX_HASH=$(grep -o '{.*}' ./instantiate_tribute_res.json | jq -r '.txhash')
    $NEUTRON_BINARY q tx $INSTANTIATE_TRIBUTE_TX_HASH $NEUTRON_NODE_FLAG --output json &> ./instantiate_tribute_tx.json
    TRIBUTE_CONTRACT_ADDRESS=$(jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value' ./instantiate_tribute_tx.json)
}

submit_proposals() {
    echo 'Submitting proposal 1...'

    EXECUTE='{"create_proposal": {"tranche_id": 1,"title": "Proposal 1 Title", "description": "Proposal 1 Description", "deployment_duration": 1,"minimum_atom_liquidity_request":"1000"}}'
    $NEUTRON_BINARY tx wasm execute $HYDRO_CONTRACT_ADDRESS "$EXECUTE" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS
    sleep 10

    echo 'Submitting proposal 2...'

    EXECUTE='{"create_proposal": {"tranche_id": 1,"title": "Proposal 2 Title", "description": "Proposal 2 Description", "deployment_duration": 1,"minimum_atom_liquidity_request":"2000"}}'
    $NEUTRON_BINARY tx wasm execute $HYDRO_CONTRACT_ADDRESS "$EXECUTE" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS
    sleep 10

    echo 'Submitting proposal 3...'

    EXECUTE='{"create_proposal": {"tranche_id": 1,"title": "Proposal 3 Title", "description": "Proposal 3 Description", "deployment_duration": 1,"minimum_atom_liquidity_request":"3000"}}'
    $NEUTRON_BINARY tx wasm execute $HYDRO_CONTRACT_ADDRESS "$EXECUTE" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS
    sleep 10
}

add_tributes() {
    echo 'Adding proposal 1 tribute...'

    EXECUTE='{"add_tribute":{"round_id":0,"tranche_id":1,"proposal_id":0}}'
    $NEUTRON_BINARY tx wasm execute $TRIBUTE_CONTRACT_ADDRESS "$EXECUTE" --amount 10000untrn --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS
    sleep 10

    echo 'Adding proposal 2 tribute...'

    EXECUTE='{"add_tribute":{"round_id":0,"tranche_id":1,"proposal_id":1}}'
    $NEUTRON_BINARY tx wasm execute $TRIBUTE_CONTRACT_ADDRESS "$EXECUTE" --amount 10000untrn --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS
    sleep 10

    echo 'Adding proposal 3 tribute...'

    EXECUTE='{"add_tribute":{"round_id":0,"tranche_id":1,"proposal_id":2}}'
    $NEUTRON_BINARY tx wasm execute $TRIBUTE_CONTRACT_ADDRESS "$EXECUTE" --amount 10000untrn --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS
    sleep 10
}

store_hydro
store_tribute

echo 'Hydro code ID:' $HYDRO_CODE_ID
echo 'Tribute code ID:' $TRIBUTE_CODE_ID

instantiate_hydro
instantiate_tribute

echo 'Hydro contract address:' $HYDRO_CONTRACT_ADDRESS
echo 'Tribute contract address:'  $TRIBUTE_CONTRACT_ADDRESS

submit_proposals
add_tributes

echo 'Done!'
