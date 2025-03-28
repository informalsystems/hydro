#!/bin/bash
set -eux

CONFIG_FILE="$1"
HYDRO_CONTRACT_ADDRESS="$2"

NEUTRON_CHAIN_ID=$(jq -r '.chain_id' $CONFIG_FILE)
NEUTRON_NODE=$(jq -r '.neutron_rpc_node' $CONFIG_FILE)
TX_SENDER_WALLET=$(jq -r '.tx_sender_wallet' $CONFIG_FILE)
TOKEN_TO_LOCK_1=$(jq -r '.token_to_lock_1' $CONFIG_FILE)
TX_SENDER_ADDRESS=$(neutrond keys show $TX_SENDER_WALLET --keyring-backend test | grep "address:" | sed 's/.*address: //')

NEUTRON_BINARY="neutrond"
NEUTRON_CHAIN_ID_FLAG="--chain-id $NEUTRON_CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend test"
TX_FLAG="--gas auto --gas-adjustment 1.3"
NEUTRON_NODE_FLAG="--node $NEUTRON_NODE"
NEUTRON_TX_FLAGS="$TX_FLAG --gas-prices 0.0053untrn --chain-id $NEUTRON_CHAIN_ID $NEUTRON_NODE_FLAG $KEYRING_TEST_FLAG -y"

# Queries expired user lockups. If there is any, it will be refreshed for another lock epoch length.
# If there are no expired user lockups, a new lockup will be created.
prepare_lockup_for_voting() {
    error_handler() {
        echo "Content of execute_res.json:"
        cat ./execute_res.json
    }
    trap error_handler ERR

    # Query Constants to get the lock epoch length
    QUERY='{"constants":{ }}'
    $NEUTRON_BINARY q wasm contract-state smart $HYDRO_CONTRACT_ADDRESS "$QUERY" $NEUTRON_NODE_FLAG -o json > ./query_res.json
    LOCK_EPOCH_LENGTH=$(jq '.data.constants.lock_epoch_length' query_res.json)

    # Query expired user lockups to see if there are some that can be refreshed
    QUERY='{"expired_user_lockups":{"address": "'$TX_SENDER_ADDRESS'", "start_from":0, "limit": 100}}'
    $NEUTRON_BINARY q wasm contract-state smart $HYDRO_CONTRACT_ADDRESS "$QUERY" $NEUTRON_NODE_FLAG -o json > ./query_res.json

    if [ "$(jq '.data.lockups | length' query_res.json)" -eq 0 ]; then
        LOCK_ID="-1"        
    else
        LOCK_ID=$(jq '.data.lockups[0].lock_id' query_res.json)
    fi

    if [ "$LOCK_ID" -eq -1 ]; then
        # If there are no expired lockups- create a new one
        echo 'Creating new lockup ...'

        EXECUTE='{"lock_tokens": {"lock_duration": '$LOCK_EPOCH_LENGTH' }}'
        echo $EXECUTE
        $NEUTRON_BINARY tx wasm execute $HYDRO_CONTRACT_ADDRESS "$EXECUTE" --amount 10$TOKEN_TO_LOCK_1 --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS -o json > ./execute_res.json
        sleep 10

        echo $(extract_new_lock_id)

        read LOCK_ID <<< "$(extract_new_lock_id)"
    else
        # If there is some expired lockup- refresh it
        echo 'Refreshing lockup with ID: '$LOCK_ID' ...'

        EXECUTE='{"refresh_lock_duration": {"lock_ids": ['$LOCK_ID'], "lock_duration": '$LOCK_EPOCH_LENGTH' }}'
        $NEUTRON_BINARY tx wasm execute $HYDRO_CONTRACT_ADDRESS "$EXECUTE" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS -o json > ./execute_res.json
        sleep 10
    fi

    QUERY='{"current_round": {}}'
    $NEUTRON_BINARY q wasm contract-state smart $HYDRO_CONTRACT_ADDRESS "$QUERY" $NEUTRON_NODE_FLAG -o json > ./query_res.json

    ROUND_ID=$(jq '.data.round_id' query_res.json)
}

extract_new_lock_id() {
    # Extract the txhash from the command result
    TX_HASH=$(jq -r '.txhash' ./execute_res.json)

    # Query the transaction details using the extracted TX_HASH
    $NEUTRON_BINARY q tx "$TX_HASH" $NEUTRON_NODE_FLAG -o json &> ./execute_tx.json 

    # Extract the lock_id attribute from the wasm event
    LOCK_ID=$(jq -r '.events[] | select(.type == "wasm") | .attributes[] | select(.key == "lock_id") | .value' ./execute_tx.json)

    # Echo the LOCK_ID so that it can be captured by the caller
    echo "$LOCK_ID"
}

# Use the provided LOCK_ID to vote for the first proposal in the given (ROUND_ID, TRANCHE_ID).
vote() {
    ROUND_ID="$1"
    TRANCHE_ID="$2"
    LOCK_ID="$3"

    # Query all round proposals
    QUERY='{"round_proposals":{"round_id": '$ROUND_ID', "tranche_id": '$TRANCHE_ID', "start_from":0, "limit": 100}}'
    $NEUTRON_BINARY q wasm contract-state smart $HYDRO_CONTRACT_ADDRESS "$QUERY" $NEUTRON_NODE_FLAG -o json > ./query_res.json

    # Safety check in case there are no proposals
    if [ "$(jq '.data.proposals | length' query_res.json)" -eq 0 ]; then
        echo "No proposals that can be voted on in round: '$ROUND_ID' and tranche: '$TRANCHE_ID'"
        return 0
    fi

    # Take the first proposal ID and vote for it
    PROPOSAL_ID=$(jq '.data.proposals[0].proposal_id' query_res.json)
    EXECUTE='{"vote": {"tranche_id": '$TRANCHE_ID', "proposals_votes": [{"proposal_id": '$PROPOSAL_ID', "lock_ids": ['$LOCK_ID']}] }}'
    $NEUTRON_BINARY tx wasm execute $HYDRO_CONTRACT_ADDRESS "$EXECUTE" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS -o json > ./execute_res.json
    sleep 10
}

prepare_lockup_for_voting

TRANCHE_ID=1
vote $ROUND_ID $TRANCHE_ID $LOCK_ID

TRANCHE_ID=2
vote $ROUND_ID $TRANCHE_ID $LOCK_ID