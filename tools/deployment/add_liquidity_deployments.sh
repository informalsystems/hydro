#!/bin/bash
set -eux

# Check for required arguments
if [ $# -lt 6 ]; then
  echo "Usage: $0 <config_file> <hydro_contract_address> <round_id> <tranche_id> <proposal_id> <deployed_fund_amount>"
  exit 1
fi

CONFIG_FILE="$1"
HYDRO_CONTRACT_ADDRESS="$2"
ROUND_ID="$3"
TRANCHE_ID="$4"
PROPOSAL_ID="$5"
DEPLOYED_FUND_AMOUNT="$6"

CHAIN_ID=$(jq -r '.chain_id' $CONFIG_FILE)
CHAIN_BINARY=$(jq -r '.chain_binary' $CONFIG_FILE)
CHAIN_NODE=$(jq -r '.chain_rpc_node' $CONFIG_FILE)
TX_SENDER_WALLET=$(jq -r '.tx_sender_wallet' $CONFIG_FILE)

CHAIN_ID_FLAG="--chain-id $CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend test"
TX_FLAG="--gas auto --gas-adjustment 1.2"
CHAIN_NODE_FLAG="--node $CHAIN_NODE"
CHAIN_TX_FLAGS="$TX_FLAG --gas-prices 0.005uatom $CHAIN_ID_FLAG $CHAIN_NODE_FLAG $KEYRING_TEST_FLAG -y"

if [ "$DEPLOYED_FUND_AMOUNT" -eq 0 ]; then
  DEPLOYED_FUNDS="[]"
else
  DEPLOYED_FUNDS='[{"amount":"'"$DEPLOYED_FUND_AMOUNT"'","denom":"uatom"}]'
fi

EXECUTE='{"add_liquidity_deployment":{"deployed_funds":'"$DEPLOYED_FUNDS"',"destinations":["Secret1a65a9xgqrlsgdszqjtxhz069pgsh8h4a83hwt0"],"funds_before_deployment":[{"amount":"1000000","denom":"uatom"}],"proposal_id":'"$PROPOSAL_ID"',"remaining_rounds":0,"round_id":'"$ROUND_ID"',"total_rounds":0,"tranche_id":'"$TRANCHE_ID"'}}'
$CHAIN_BINARY tx wasm execute $HYDRO_CONTRACT_ADDRESS "$EXECUTE" --from $TX_SENDER_WALLET $CHAIN_TX_FLAGS
sleep 15