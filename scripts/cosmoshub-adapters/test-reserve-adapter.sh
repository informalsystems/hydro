#!/bin/bash
# Test reserve adapter: deposit 0.1 ATOM, query balance, withdraw 0.1 ATOM
# Usage: ./test-reserve-adapter.sh deploy-config-adapters-mainnet-test.json

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

if [ -z "$1" ]; then
    echo -e "${RED}Usage: $0 <config-file.json>${NC}"
    exit 1
fi

CONFIG_FILE="$1"
BINARY=$(jq -r '.binary' "$CONFIG_FILE")
NODE=$(jq -r '.rpc_node' "$CONFIG_FILE")
CHAIN_ID=$(jq -r '.chain_id' "$CONFIG_FILE")
GAS_PRICE=$(jq -r '.gas_price' "$CONFIG_FILE")
GAS_ADJUSTMENT=$(jq -r '.gas_adjustment' "$CONFIG_FILE")
KEYRING=$(jq -r '.keyring_backend' "$CONFIG_FILE")
DEPLOYER=$(jq -r '.deployer_wallet' "$CONFIG_FILE")
DEPOSITOR=$(jq -r '.vault_address' "$CONFIG_FILE")
RESERVE=$(jq -r '.contracts.reserve_adapter' "$CONFIG_FILE")

TX_FLAGS="--node $NODE --chain-id $CHAIN_ID --gas auto --gas-adjustment $GAS_ADJUSTMENT --gas-prices $GAS_PRICE --keyring-backend $KEYRING -y"

wait_tx() {
    local txhash="$1"
    local result
    for i in $(seq 1 30); do
        result=$($BINARY q tx "$txhash" --node "$NODE" --output json 2>/dev/null || true)
        if echo "$result" | jq -e '.height' &>/dev/null; then
            local code; code=$(echo "$result" | jq -r '.code // 0')
            if [ "$code" != "0" ]; then
                echo -e "${RED}Tx failed: $(echo "$result" | jq -r '.raw_log')${NC}"
                exit 1
            fi
            return 0
        fi
        sleep 1
    done
    echo -e "${RED}Tx not confirmed after 30s${NC}"; exit 1
}

echo -e "${BLUE}=== Reserve Adapter Test ===${NC}"
echo "Contract: $RESERVE"
echo "Depositor: $DEPOSITOR"
echo ""

# Step 1: Deposit 0.1 ATOM
echo "1. Depositing 0.1 ATOM..."
OUT=$($BINARY tx wasm execute "$RESERVE" \
    '{"standard_action":{"deposit":{}}}' \
    --amount 100000uatom \
    --from "$DEPLOYER" \
    $TX_FLAGS --output json 2>&1)
TXHASH=$(echo "$OUT" | grep -o '"txhash":"[^"]*"' | cut -d'"' -f4)
echo "   txhash: $TXHASH"
wait_tx "$TXHASH"
echo -e "   ${GREEN}Deposit confirmed${NC}"

# Step 2: Query balance
echo ""
echo "2. Querying available balance..."
$BINARY query wasm contract-state smart "$RESERVE" \
    "{\"standard_query\":{\"available_for_withdraw\":{\"depositor_address\":\"$DEPOSITOR\",\"denom\":\"uatom\"}}}" \
    --node "$NODE" --output json | jq .

# Step 3: Withdraw 0.1 ATOM
echo ""
echo "3. Withdrawing 0.1 ATOM..."
OUT=$($BINARY tx wasm execute "$RESERVE" \
    '{"standard_action":{"withdraw":{"coin":{"denom":"uatom","amount":"100000"}}}}' \
    --from "$DEPLOYER" \
    $TX_FLAGS --output json 2>&1)
TXHASH=$(echo "$OUT" | grep -o '"txhash":"[^"]*"' | cut -d'"' -f4)
echo "   txhash: $TXHASH"
wait_tx "$TXHASH"
echo -e "   ${GREEN}Withdraw confirmed${NC}"

# Step 4: Query balance again
echo ""
echo "4. Final balance (should be 0)..."
$BINARY query wasm contract-state smart "$RESERVE" \
    "{\"standard_query\":{\"available_for_withdraw\":{\"depositor_address\":\"$DEPOSITOR\",\"denom\":\"uatom\"}}}" \
    --node "$NODE" --output json | jq .

echo ""
echo -e "${GREEN}Reserve adapter test complete.${NC}"
