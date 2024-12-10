#!/bin/bash
set -eux

CONFIG_FILE="$1"
HYDRO_CONTRACT_ADDRESS="$2"
TRIBUTE_CONTRACT_ADDRESS="$3"

NEUTRON_CHAIN_ID=$(jq -r '.chain_id' $CONFIG_FILE)
NEUTRON_NODE=$(jq -r '.neutron_rpc_node' $CONFIG_FILE)
TX_SENDER_WALLET=$(jq -r '.tx_sender_wallet' $CONFIG_FILE)

TRIBUTE_TOKEN_1=$(jq -r '.tribute_token_1' $CONFIG_FILE)
TRIBUTE_TOKEN_2=$(jq -r '.tribute_token_2' $CONFIG_FILE)
TRIBUTE_TOKEN_3=$(jq -r '.tribute_token_3' $CONFIG_FILE)

NEUTRON_BINARY="neutrond"
NEUTRON_CHAIN_ID_FLAG="--chain-id $NEUTRON_CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend test"
TX_FLAG="--gas auto --gas-adjustment 1.3"
NEUTRON_NODE_FLAG="--node $NEUTRON_NODE"
NEUTRON_TX_FLAGS="$TX_FLAG --gas-prices 0.0053untrn --chain-id $NEUTRON_CHAIN_ID $NEUTRON_NODE_FLAG $KEYRING_TEST_FLAG -y"

submit_proposals() {
    echo 'Submitting proposal 1...'

    EXECUTE='{"create_proposal": {"tranche_id": 1,"title": "Proposal 1 Title", "description": "Proposal 1 Description", "deployment_duration": 1,"minimum_atom_liquidity_request":"1000"}}'
    $NEUTRON_BINARY tx wasm execute $HYDRO_CONTRACT_ADDRESS "$EXECUTE" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS -o json > ./execute_res.json
    sleep 10

    echo $(extract_proposal_details)


    read PROPOSAL_ID_1 ROUND_ID_1 <<< "$(extract_proposal_details)"

    echo 'Submitting proposal 2...'

    EXECUTE='{"create_proposal": {"tranche_id": 1,"title": "Proposal 2 Title", "description": "Proposal 2 Description", "deployment_duration": 2,"minimum_atom_liquidity_request":"2000"}}'
    $NEUTRON_BINARY tx wasm execute $HYDRO_CONTRACT_ADDRESS "$EXECUTE" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS -o json > ./execute_res.json
    sleep 10

    read PROPOSAL_ID_2 ROUND_ID_2 <<< "$(extract_proposal_details)"

    echo 'Submitting proposal 3...'

    EXECUTE='{"create_proposal": {"tranche_id": 1,"title": "Proposal 3 Title", "description": "Proposal 3 Description", "deployment_duration": 3,"minimum_atom_liquidity_request":"3000"}}'
    $NEUTRON_BINARY tx wasm execute $HYDRO_CONTRACT_ADDRESS "$EXECUTE" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS -o json > ./execute_res.json
    sleep 10

    read PROPOSAL_ID_3 ROUND_ID_3 <<< "$(extract_proposal_details)"

}

add_tributes() {
    echo 'Adding proposal 1 tribute...'

    EXECUTE='{"add_tribute":{"round_id":'"$ROUND_ID_1"',"tranche_id":1,"proposal_id":'"$PROPOSAL_ID_1"'}}'
    $NEUTRON_BINARY tx wasm execute $TRIBUTE_CONTRACT_ADDRESS "$EXECUTE" --amount 10$TRIBUTE_TOKEN_1 --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS
    sleep 10

    echo 'Adding proposal 2 tribute...'

    EXECUTE='{"add_tribute":{"round_id":'"$ROUND_ID_2"',"tranche_id":1,"proposal_id":'"$PROPOSAL_ID_2"'}}'
    $NEUTRON_BINARY tx wasm execute $TRIBUTE_CONTRACT_ADDRESS "$EXECUTE" --amount 10$TRIBUTE_TOKEN_2 --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS
    sleep 10

    echo 'Adding proposal 3 tribute...'

    EXECUTE='{"add_tribute":{"round_id":'"$ROUND_ID_3"',"tranche_id":1,"proposal_id":'"$PROPOSAL_ID_3"'}}'
    $NEUTRON_BINARY tx wasm execute $TRIBUTE_CONTRACT_ADDRESS "$EXECUTE" --amount 10$TRIBUTE_TOKEN_3 --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS
    sleep 10
}

extract_proposal_details() {
    # Extract the txhash from the command result
    TX_HASH=$(jq -r '.txhash' ./execute_res.json)

    # Query the transaction details using the extracted TX_HASH
    $NEUTRON_BINARY q tx "$TX_HASH" $NEUTRON_NODE_FLAG -o json &> ./execute_tx.json 

    # Extract the proposal_id attribute from the wasm event
    PROPOSAL_ID=$(jq -r '.events[] | select(.type == "wasm") | .attributes[] | select(.key == "proposal_id") | .value' ./execute_tx.json)

    ROUND_ID=$(jq -r '.events[] | select(.type == "wasm") | .attributes[] | select(.key == "round_id") | .value' ./execute_tx.json)

    # Echo the PROPOSAL_ID so that it can be captured by the caller
    echo "$PROPOSAL_ID $ROUND_ID"
}


submit_proposals
add_tributes

echo 'Successfully created proposals and tributes'
