#!/bin/bash
set -eux

CONFIG_FILE="$1"

NEUTRON_CHAIN_ID=$(jq -r '.chain_id' $CONFIG_FILE)
NEUTRON_NODE=$(jq -r '.neutron_rpc_node' $CONFIG_FILE)
TX_SENDER_WALLET=$(jq -r '.tx_sender_wallet' $CONFIG_FILE)

NEUTRON_BINARY="neutrond"
NEUTRON_CHAIN_ID_FLAG="--chain-id $NEUTRON_CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend test"
TX_FLAG="--gas auto --gas-adjustment 1.3"
NEUTRON_NODE_FLAG="--node $NEUTRON_NODE"
NEUTRON_TX_FLAGS="$TX_FLAG --gas-prices 0.0053untrn --chain-id $NEUTRON_CHAIN_ID $NEUTRON_NODE_FLAG $KEYRING_TEST_FLAG -y"

submit_proposals() {
    echo 'Submitting proposal 1...'

    EXECUTE='{"create_proposal": {"tranche_id": 1,"title": "Proposal 1 Title", "description": "Proposal 1 Description", "deployment_duration": 1,"minimum_atom_liquidity_request":"1000"}}'
    $NEUTRON_BINARY tx wasm execute $HYDRO_CONTRACT_ADDRESS "$EXECUTE" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS
    sleep 10

    echo 'Submitting proposal 2...'

    EXECUTE='{"create_proposal": {"tranche_id": 1,"title": "Proposal 2 Title", "description": "Proposal 2 Description", "deployment_duration": 2,"minimum_atom_liquidity_request":"2000"}}'
    $NEUTRON_BINARY tx wasm execute $HYDRO_CONTRACT_ADDRESS "$EXECUTE" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS
    sleep 10

    echo 'Submitting proposal 3...'

    EXECUTE='{"create_proposal": {"tranche_id": 1,"title": "Proposal 3 Title", "description": "Proposal 3 Description", "deployment_duration": 3,"minimum_atom_liquidity_request":"3000"}}'
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

submit_proposals
add_tributes

echo 'Successfully created proposals and tributes'
