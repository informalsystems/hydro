#!/bin/bash
# Test CCTP adapter: deposit 0.5 USDC, bridge to Base
# Usage: ./test-cctp-adapter.sh deploy-config.json
#
# NOTE on bridging fee:
#   TransferFunds requires the executor to send USDC alongside the tx call.
#   This fee is forwarded to Noble Orbiter's relayer (noble_fee_recipient).
#   The bridged amount comes from the adapter's internal balance.
#   Adjust BRIDGE_FEE_UUSDC below based on current Noble Orbiter fee schedule.

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
CCTP=$(jq -r '.contracts.cctp_adapter' "$CONFIG_FILE")
USDC_DENOM=$(jq -r '.cctp_adapter.usdc_denom' "$CONFIG_FILE")

# Amount to deposit into the adapter: 0.5 USDC = 500000 uUSDC
DEPOSIT_UUSDC=500000
# Bridge amount taken from adapter balance (the full deposit)
BRIDGE_AMOUNT_UUSDC=500000
# Fee sent alongside the TransferFunds call from the executor's wallet (not from adapter balance).
# Noble Orbiter relayer fee observed ~1836 uUSDC. Using 3000 to ensure full amount arrives.
BRIDGE_FEE_UUSDC=3000

DEST_CHAIN=$(jq -r '.cctp_adapter.initial_chains[0].chain_id' "$CONFIG_FILE")
DEST_RECIPIENT=$(jq -r '.cctp_adapter.initial_chains[0].initial_allowed_destination_addresses[0]' "$CONFIG_FILE")

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

echo -e "${BLUE}=== CCTP Adapter Test ===${NC}"
echo "Contract:    $CCTP"
echo "Depositor:   $DEPOSITOR"
echo "USDC denom:  $USDC_DENOM"
echo "Destination: $DEST_CHAIN / 0x$DEST_RECIPIENT"
echo "Bridge fee:  $BRIDGE_FEE_UUSDC uUSDC (sent with TransferFunds call)"
echo ""

# Step 1: Deposit 0.5 USDC into the adapter
echo "1. Depositing 0.5 USDC (${DEPOSIT_UUSDC} uUSDC)..."
OUT=$($BINARY tx wasm execute "$CCTP" \
    '{"standard_action":{"deposit":{}}}' \
    --amount "${DEPOSIT_UUSDC}${USDC_DENOM}" \
    --from "$DEPLOYER" \
    $TX_FLAGS --output json 2>&1)
TXHASH=$(echo "$OUT" | grep -o '"txhash":"[^"]*"' | cut -d'"' -f4)
echo "   txhash: $TXHASH"
wait_tx "$TXHASH"
echo -e "   ${GREEN}Deposit confirmed${NC}"

# Step 2: Query available balance
echo ""
echo "2. Available for withdraw (USDC)..."
$BINARY query wasm contract-state smart "$CCTP" \
    "{\"standard_query\":{\"available_for_withdraw\":{\"depositor_address\":\"$DEPOSITOR\",\"denom\":\"$USDC_DENOM\"}}}" \
    --node "$NODE" --output json | jq .

# Step 3: Execute bridge (executor only)
# The executor sends BRIDGE_FEE_UUSDC as info.funds — this is the Noble Orbiter relayer fee.
# The BRIDGE_AMOUNT_UUSDC is taken from the adapter's internal balance.
BRIDGE_MSG=$(jq -n \
    --arg chain "$DEST_CHAIN" \
    --arg recipient "$DEST_RECIPIENT" \
    --arg amount "$BRIDGE_AMOUNT_UUSDC" \
    '{
        custom_action: {
            transfer_funds: {
                amount: $amount,
                instructions: {
                    chain_id: $chain,
                    recipient: $recipient
                }
            }
        }
    }')

echo ""
echo "3. Bridging ${BRIDGE_AMOUNT_UUSDC} uUSDC to Base (fee: ${BRIDGE_FEE_UUSDC} uUSDC sent with call)..."
echo "   Message:"
echo "$BRIDGE_MSG" | jq .

OUT=$($BINARY tx wasm execute "$CCTP" \
    "$BRIDGE_MSG" \
    --amount "${BRIDGE_FEE_UUSDC}${USDC_DENOM}" \
    --from "$DEPLOYER" \
    $TX_FLAGS --output json 2>&1)
TXHASH=$(echo "$OUT" | grep -o '"txhash":"[^"]*"' | cut -d'"' -f4)
echo "   txhash: $TXHASH"
wait_tx "$TXHASH"
echo -e "   ${GREEN}Bridge tx submitted — CCTP transfer in flight via Noble${NC}"

echo ""
echo -e "${GREEN}CCTP adapter test complete. Monitor the CCTP transfer on Base scan.${NC}"
