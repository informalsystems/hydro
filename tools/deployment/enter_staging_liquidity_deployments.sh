#!/bin/bash
set -eux

CONFIG_FILE="$1"
HYDRO_CONTRACT_ADDRESS="$2"

NEUTRON_CHAIN_ID=$(jq -r '.chain_id' $CONFIG_FILE)
NEUTRON_NODE=$(jq -r '.neutron_rpc_node' $CONFIG_FILE)
TX_SENDER_WALLET=$(jq -r '.tx_sender_wallet' $CONFIG_FILE)
TX_SENDER_ADDRESS=$(neutrond keys show $TX_SENDER_WALLET --keyring-backend test | grep "address:" | sed 's/.*address: //')

NEUTRON_BINARY="neutrond"
NEUTRON_CHAIN_ID_FLAG="--chain-id $NEUTRON_CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend test"
TX_FLAG="--gas auto --gas-adjustment 1.3"
NEUTRON_NODE_FLAG="--node $NEUTRON_NODE"
NEUTRON_TX_FLAGS="$TX_FLAG --gas-prices 0.0053untrn --chain-id $NEUTRON_CHAIN_ID $NEUTRON_NODE_FLAG $KEYRING_TEST_FLAG -y"

NON_ZERO_FUNDS=1000

# Goes through all proposals of the given round and tranche and sends txs to enter liqudity deployment information.
# If the proposal got zero power, deployment info will contain zero funds. Otherwise, funds are set to NON_ZERO_FUNDS
enter_liquidity_deployments() {
    ROUND_ID="$1"
    TRANCHE_ID="$2"

    # Query all (round, tranche) liquidity deployment infos
    QUERY='{"round_tranche_liquidity_deployments":{"round_id": '$ROUND_ID', "tranche_id": '$TRANCHE_ID', "start_from":0, "limit": 100}}'
    $NEUTRON_BINARY q wasm contract-state smart $HYDRO_CONTRACT_ADDRESS "$QUERY" $NEUTRON_NODE_FLAG -o json > ./query_res.json

    # Safety check if the liquidity deployment infos are already entered. It is assumed that information
    # isn't entered partialy (e.g. only for some proposals).
    if [ "$(jq '.data.liquidity_deployments | length' query_res.json)" -ne 0 ]; then
        echo "Liquidity deployment infos are already entered for round: '$ROUND_ID' and tranche: '$TRANCHE_ID'"
        return 0
    fi

    # Query all round proposals
    QUERY='{"round_proposals":{"round_id": '$ROUND_ID', "tranche_id": '$TRANCHE_ID', "start_from":0, "limit": 100}}'
    $NEUTRON_BINARY q wasm contract-state smart $HYDRO_CONTRACT_ADDRESS "$QUERY" $NEUTRON_NODE_FLAG -o json > ./query_res.json

    # Safety check in case there are no proposals
    if [ "$(jq '.data.proposals | length' query_res.json)" -eq 0 ]; then
        echo "No proposals found in round: '$ROUND_ID' and tranche: '$TRANCHE_ID'"
        return 0
    fi

    PROPOSALS=$(jq -c '[.data.proposals[] | {proposal_id: .proposal_id, power: .power }]' query_res.json)

    # Go through all proposals and enter liquidity deployment info
    echo "$PROPOSALS" | jq -c '.[]' | while IFS= read -r proposal; do
        PROPOSAL_ID=$(echo "$proposal" | jq -r '.proposal_id')
        PROPOSAL_POWER=$(echo "$proposal" | jq -r '.power')

        if [ "$PROPOSAL_POWER" = "0" ]; then
            FUNDS=0
        else
            FUNDS=$NON_ZERO_FUNDS
        fi

        source ./tools/deployment/add_liquidity_deployments.sh $CONFIG_FILE $HYDRO_CONTRACT_ADDRESS \
            $ROUND_ID $TRANCHE_ID $PROPOSAL_ID $FUNDS
    done
}

query_previous_round() {
    QUERY='{"current_round": {}}'
    $NEUTRON_BINARY q wasm contract-state smart $HYDRO_CONTRACT_ADDRESS "$QUERY" $NEUTRON_NODE_FLAG -o json > ./query_res.json

    ROUND_ID=$(jq '.data.round_id' query_res.json)
    PREVIOUS_ROUND_ID=$((ROUND_ID - 1))
}

query_previous_round

if [ "$PREVIOUS_ROUND_ID" -eq -1 ]; then
    echo "Cannot add liquidity deployments for previous round. First round is still in progress."
    return 0
fi

TRANCHE_ID=1
enter_liquidity_deployments $PREVIOUS_ROUND_ID $TRANCHE_ID

TRANCHE_ID=2
enter_liquidity_deployments $PREVIOUS_ROUND_ID $TRANCHE_ID
