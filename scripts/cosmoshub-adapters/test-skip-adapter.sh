#!/bin/bash
# Test Skip adapter: deposit 0.1 ATOM, execute swap to min 0.05 stATOM
# Usage: ./test-skip-adapter.sh deploy-config-adapters-mainnet-test.json

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
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
SKIP=$(jq -r '.contracts.skip_adapter' "$CONFIG_FILE")

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

echo -e "${BLUE}=== Skip Adapter Test ===${NC}"
echo "Contract: $SKIP"
echo "Depositor: $DEPOSITOR"
echo ""

# Step 1: Deposit 0.1 ATOM
echo "1. Depositing 0.1 ATOM..."
OUT=$($BINARY tx wasm execute "$SKIP" \
    '{"standard_action":{"deposit":{}}}' \
    --amount 100000uatom \
    --from "$DEPLOYER" \
    $TX_FLAGS --output json 2>&1)
TXHASH=$(echo "$OUT" | grep -o '"txhash":"[^"]*"' | cut -d'"' -f4)
echo "   txhash: $TXHASH"
wait_tx "$TXHASH"
echo -e "   ${GREEN}Deposit confirmed${NC}"

# Step 2: Query available balance
echo ""
echo "2. Available for withdraw (uatom)..."
$BINARY query wasm contract-state smart "$SKIP" \
    "{\"standard_query\":{\"available_for_withdraw\":{\"depositor_address\":\"$DEPOSITOR\",\"denom\":\"uatom\"}}}" \
    --node "$NODE" --output json | jq .

# Step 3: Execute swap (executor only)
SWAP_MSG='{"custom_action":{"execute_swap":{"params":{"route_id":"atom-statom-osmosis","amount_in":"100000","min_amount_out":"50000"}}}}'
echo ""
echo "3. Executing swap (100000 uatom → min 50000 stATOM)..."
echo "   Message: $SWAP_MSG"

OUT=$($BINARY tx wasm execute "$SKIP" \
    "$SWAP_MSG" \
    --from "$DEPLOYER" \
    $TX_FLAGS --output json 2>&1)
TXHASH=$(echo "$OUT" | grep -o '"txhash":"[^"]*"' | cut -d'"' -f4)
echo "   txhash: $TXHASH"
wait_tx "$TXHASH"
echo -e "   ${GREEN}Swap tx submitted — IBC in flight${NC}"

echo ""
echo -e "${GREEN}Skip adapter test complete. Check stATOM balance at the depositor address once IBC completes.${NC}"
