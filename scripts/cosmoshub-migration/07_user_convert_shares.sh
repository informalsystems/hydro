#!/bin/bash
# Step 7: User sends their IBC'd Neutron vault shares to the Shares Converter
# and receives an equal amount of Cosmos Hub vault shares in return.
#
# Usage: ./07_user_convert_shares.sh <cosmoshub-config> <user-wallet>
# Example: ./07_user_convert_shares.sh deploy-config-cosmoshub.json alice

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

COSMOSHUB_CONFIG="${1:?Usage: $0 <cosmoshub-config> <user-wallet>}"
USER_WALLET="${2:?Usage: $0 <cosmoshub-config> <user-wallet>}"

if [ ! -f "$COSMOSHUB_CONFIG" ]; then
    echo -e "${RED}Error: config file '$COSMOSHUB_CONFIG' not found${NC}"
    exit 1
fi

STATE_FILE="$SCRIPT_DIR/migration-state.json"
if [ ! -f "$STATE_FILE" ]; then
    echo -e "${RED}Error: migration-state.json not found. Run step 02 first.${NC}"
    exit 1
fi

# ============================================================================
# Load configuration
# ============================================================================

HUB_BINARY=$(jq -r '.binary' "$COSMOSHUB_CONFIG")
HUB_BINARY_HOME=$(jq -r '.binary_home // empty' "$COSMOSHUB_CONFIG")
HUB_CHAIN_ID=$(jq -r '.chain_id' "$COSMOSHUB_CONFIG")
HUB_NODE=$(jq -r '.rpc_node' "$COSMOSHUB_CONFIG")
HUB_GAS_PRICE=$(jq -r '.gas_price' "$COSMOSHUB_CONFIG")
HUB_GAS_ADJUSTMENT=$(jq -r '.gas_adjustment' "$COSMOSHUB_CONFIG")
HUB_KEYRING=$(jq -r '.keyring_backend' "$COSMOSHUB_CONFIG")
HUB_CONVERTER_ADDR=$(jq -r '.contracts.shares_converter // empty' "$COSMOSHUB_CONFIG")

if [ -n "$HUB_BINARY_HOME" ]; then
    HUB_CLI="$HUB_BINARY --home $HUB_BINARY_HOME"
else
    HUB_CLI="$HUB_BINARY"
fi

HUB_TX_FLAGS="--gas auto --gas-adjustment $HUB_GAS_ADJUSTMENT --gas-prices $HUB_GAS_PRICE --chain-id $HUB_CHAIN_ID --node $HUB_NODE --keyring-backend $HUB_KEYRING -y"

if [ -z "$HUB_CONVERTER_ADDR" ] || [ "$HUB_CONVERTER_ADDR" = "null" ]; then
    echo -e "${RED}Error: contracts.shares_converter is not set in $COSMOSHUB_CONFIG${NC}"
    exit 1
fi

NEUTRON_SHARES_IBC_DENOM=$(jq -r '.neutron_shares_ibc_denom_on_hub' "$STATE_FILE")
HUB_VAULT_SHARES_DENOM=$(jq -r '.hub_vault_shares_denom' "$STATE_FILE")

if [ -z "$NEUTRON_SHARES_IBC_DENOM" ] || [ "$NEUTRON_SHARES_IBC_DENOM" = "null" ] || [ -z "$NEUTRON_SHARES_IBC_DENOM" ]; then
    echo -e "${RED}Error: neutron_shares_ibc_denom_on_hub is not set in migration-state.json. Run step 02 first.${NC}"
    exit 1
fi

# ============================================================================
# Helpers
# ============================================================================

extract_json() { echo "$1" | awk '/^\{/{p=1} p{print}'; }

retry_command() {
    set +e
    local output status max_attempts=${2:-0} attempt=1
    while true; do
        output=$(eval "$1" 2>&1)
        status=$?
        if [ $status -eq 0 ]; then
            echo "" >&2
            echo "$output"
            set -e
            return 0
        fi
        if [ $max_attempts -gt 0 ] && [ $attempt -ge $max_attempts ]; then
            echo "Error: Maximum retry attempts ($max_attempts) reached" >&2
            echo "$output" >&2
            set -e
            return $status
        fi
        printf "." >&2
        sleep 1
        ((attempt++))
    done
}

# ============================================================================
# Step 7a: Query user's IBC share balance on Hub
# ============================================================================

echo -e "${BLUE}=== Step 7: Convert Neutron IBC shares → Cosmos Hub vault shares ===${NC}"
echo ""

USER_ADDR=$($HUB_CLI keys show "$USER_WALLET" --keyring-backend "$HUB_KEYRING" -a)
if [ -z "$USER_ADDR" ]; then
    echo -e "${RED}Error: could not find key '$USER_WALLET' in Hub keyring${NC}"
    exit 1
fi

echo "  User address:           $USER_ADDR"
echo "  Neutron IBC denom:      $NEUTRON_SHARES_IBC_DENOM"
echo ""

IBC_SHARES_AMOUNT=$($HUB_CLI q bank balance "$USER_ADDR" "$NEUTRON_SHARES_IBC_DENOM" \
    --node "$HUB_NODE" --output json | jq -r '.balance.amount')

if [ -z "$IBC_SHARES_AMOUNT" ] || [ "$IBC_SHARES_AMOUNT" = "null" ] || [ "$IBC_SHARES_AMOUNT" = "0" ]; then
    echo -e "${RED}Error: no Neutron IBC shares found for $USER_ADDR${NC}"
    echo ""
    echo -e "${YELLOW}Tip: IBC packets may still be in transit. Wait for the relayer to process the packet from step 3, then retry.${NC}"
    echo "     Check balance: $HUB_CLI q bank balance $USER_ADDR $NEUTRON_SHARES_IBC_DENOM --node $HUB_NODE -o json"
    exit 1
fi

echo "  IBC shares to convert: ${IBC_SHARES_AMOUNT}${NEUTRON_SHARES_IBC_DENOM}"
echo ""

# ============================================================================
# Step 7b: Convert
# ============================================================================

CONVERT_OUTPUT=$($HUB_CLI tx wasm execute "$HUB_CONVERTER_ADDR" \
    '{"convert":{}}' \
    --amount "${IBC_SHARES_AMOUNT}${NEUTRON_SHARES_IBC_DENOM}" \
    --from "$USER_WALLET" \
    $HUB_TX_FLAGS \
    --output json 2>&1) || true

CONVERT_JSON=$(extract_json "$CONVERT_OUTPUT")
CONVERT_TX=$(echo "$CONVERT_JSON" | jq -r '.txhash')

if [ -z "$CONVERT_TX" ] || [ "$CONVERT_TX" = "null" ]; then
    echo -e "${RED}Convert failed:${NC}"
    echo "$CONVERT_JSON" | jq .
    exit 1
fi

echo "TX submitted: $CONVERT_TX"
echo "Waiting for confirmation..."
sleep 6
CONVERT_RESULT=$(retry_command "$HUB_CLI q tx $CONVERT_TX --node $HUB_NODE --output json" 60)
CONVERT_CODE=$(echo "$CONVERT_RESULT" | jq -r '.code // 1')

if [ "$CONVERT_CODE" != "0" ]; then
    echo -e "${RED}Convert failed (code $CONVERT_CODE)${NC}"
    echo "$CONVERT_RESULT" | jq '.raw_log // .logs'
    exit 1
fi

echo -e "${GREEN}Conversion successful!${NC}"
echo ""

# ============================================================================
# Query resulting Hub vault share balance
# ============================================================================

HUB_SHARES_BALANCE=$($HUB_CLI q bank balance "$USER_ADDR" "$HUB_VAULT_SHARES_DENOM" \
    --node "$HUB_NODE" --output json | jq -r '.balance.amount')

echo "  User Hub vault share balance: ${HUB_SHARES_BALANCE} ${HUB_VAULT_SHARES_DENOM}"
echo ""
echo -e "${GREEN}=== Step 7 complete ===${NC}"
echo ""
echo "The user has exchanged ${IBC_SHARES_AMOUNT} Neutron IBC shares for ${HUB_SHARES_BALANCE} Hub vault shares."
