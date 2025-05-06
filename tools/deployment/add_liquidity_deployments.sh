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

NEUTRON_CHAIN_ID=$(jq -r '.chain_id' $CONFIG_FILE)
NEUTRON_NODE=$(jq -r '.neutron_rpc_node' $CONFIG_FILE)
TX_SENDER_WALLET=$(jq -r '.tx_sender_wallet' $CONFIG_FILE)

NEUTRON_BINARY="neutrond"
NEUTRON_CHAIN_ID_FLAG="--chain-id $NEUTRON_CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend test"
TX_FLAG="--gas auto --gas-adjustment 1.3"
NEUTRON_NODE_FLAG="--node $NEUTRON_NODE"
NEUTRON_TX_FLAGS="$TX_FLAG --gas-prices 0.0053untrn --chain-id $NEUTRON_CHAIN_ID $NEUTRON_NODE_FLAG $KEYRING_TEST_FLAG -y"

# Customize these query parameters as needed
LIMIT=1000        # maximum number of proposals to retrieve at once
START_FROM=0       # where to start from if pagination is needed

if [ "$DEPLOYED_FUND_AMOUNT" -eq 0 ]; then
  DEPLOYED_FUNDS="[]"
else
  DEPLOYED_FUNDS='[{"amount":"'"$DEPLOYED_FUND_AMOUNT"'","denom":"uatom"}]'
fi

EXECUTE='{"add_liquidity_deployment":{"deployed_funds":'"$DEPLOYED_FUNDS"',"destinations":["Secret1a65a9xgqrlsgdszqjtxhz069pgsh8h4a83hwt0"],"funds_before_deployment":[{"amount":"1000000","denom":"uatom"}],"proposal_id":'"$PROPOSAL_ID"',"remaining_rounds":0,"round_id":'"$ROUND_ID"',"total_rounds":0,"tranche_id":'"$TRANCHE_ID"'}}'
$NEUTRON_BINARY tx wasm execute $HYDRO_CONTRACT_ADDRESS "$EXECUTE" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS
sleep 10