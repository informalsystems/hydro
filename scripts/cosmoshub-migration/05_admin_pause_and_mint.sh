#!/bin/bash
# Step 5: Pause the Neutron vault, query the current pool state from the Neutron
# Control Center, and call MintForMigration on the Cosmos Hub vault.
#
# Usage: ./05_admin_pause_and_mint.sh <neutron-config> <cosmoshub-config> <admin-wallet>
# Example: ./05_admin_pause_and_mint.sh deploy-config-neutron.json deploy-config-cosmoshub.json test-deployer

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

NEUTRON_CONFIG="${1:?Usage: $0 <neutron-config> <cosmoshub-config> <admin-wallet>}"
COSMOSHUB_CONFIG="${2:?Usage: $0 <neutron-config> <cosmoshub-config> <admin-wallet>}"
ADMIN_WALLET="${3:?Usage: $0 <neutron-config> <cosmoshub-config> <admin-wallet>}"

for f in "$NEUTRON_CONFIG" "$COSMOSHUB_CONFIG"; do
    if [ ! -f "$f" ]; then
        echo -e "${RED}Error: config file '$f' not found${NC}"
        exit 1
    fi
done

STATE_FILE="$SCRIPT_DIR/migration-state.json"

# ============================================================================
# Load Neutron configuration
# ============================================================================

N_BINARY=$(jq -r '.binary' "$NEUTRON_CONFIG")
N_BINARY_HOME=$(jq -r '.binary_home // empty' "$NEUTRON_CONFIG")
N_CHAIN_ID=$(jq -r '.chain_id' "$NEUTRON_CONFIG")
N_NODE=$(jq -r '.rpc_node' "$NEUTRON_CONFIG")
N_GAS_PRICE=$(jq -r '.gas_price' "$NEUTRON_CONFIG")
N_GAS_ADJUSTMENT=$(jq -r '.gas_adjustment' "$NEUTRON_CONFIG")
N_KEYRING=$(jq -r '.keyring_backend' "$NEUTRON_CONFIG")
N_VAULT_ADDR=$(jq -r '.contracts.vault // empty' "$NEUTRON_CONFIG")
N_CC_ADDR=$(jq -r '.contracts.control_center // empty' "$NEUTRON_CONFIG")

if [ -n "$N_BINARY_HOME" ]; then
    N_CLI="$N_BINARY --home $N_BINARY_HOME"
else
    N_CLI="$N_BINARY"
fi

# ============================================================================
# Load Hub configuration
# ============================================================================

HUB_BINARY=$(jq -r '.binary' "$COSMOSHUB_CONFIG")
HUB_BINARY_HOME=$(jq -r '.binary_home // empty' "$COSMOSHUB_CONFIG")
HUB_CHAIN_ID=$(jq -r '.chain_id' "$COSMOSHUB_CONFIG")
HUB_NODE=$(jq -r '.rpc_node' "$COSMOSHUB_CONFIG")
HUB_GAS_PRICE=$(jq -r '.gas_price' "$COSMOSHUB_CONFIG")
HUB_GAS_ADJUSTMENT=$(jq -r '.gas_adjustment' "$COSMOSHUB_CONFIG")
HUB_KEYRING=$(jq -r '.keyring_backend' "$COSMOSHUB_CONFIG")
HUB_VAULT_ADDR=$(jq -r '.contracts.vault // empty' "$COSMOSHUB_CONFIG")
HUB_CONVERTER_ADDR=$(jq -r '.contracts.shares_converter // empty' "$COSMOSHUB_CONFIG")

if [ -n "$HUB_BINARY_HOME" ]; then
    HUB_CLI="$HUB_BINARY --home $HUB_BINARY_HOME"
else
    HUB_CLI="$HUB_BINARY"
fi

HUB_TX_FLAGS="--gas auto --gas-adjustment $HUB_GAS_ADJUSTMENT --gas-prices $HUB_GAS_PRICE --chain-id $HUB_CHAIN_ID --node $HUB_NODE --keyring-backend $HUB_KEYRING -y"

for addr_name in "N_VAULT_ADDR:contracts.vault (neutron)" "N_CC_ADDR:contracts.control_center (neutron)" "HUB_VAULT_ADDR:contracts.vault (hub)" "HUB_CONVERTER_ADDR:contracts.shares_converter (hub)"; do
    varname="${addr_name%%:*}"
    label="${addr_name##*:}"
    val="${!varname}"
    if [ -z "$val" ] || [ "$val" = "null" ]; then
        echo -e "${RED}Error: $label is not set in config${NC}"
        exit 1
    fi
done

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

wait_for_neutron_tx() {
    local tx_hash="$1"
    echo "TX submitted: $tx_hash"
    echo "Waiting for confirmation..."
    sleep 6
    local result
    result=$(retry_command "$N_CLI q tx $tx_hash --node $N_NODE --output json" 60)
    local code
    code=$(echo "$result" | jq -r '.code // 1')
    if [ "$code" != "0" ]; then
        echo -e "${RED}Transaction failed (code $code)${NC}"
        echo "$result" | jq '.raw_log // .logs'
        exit 1
    fi
    echo "$result"
}

wait_for_hub_tx() {
    local tx_hash="$1"
    echo "TX submitted: $tx_hash"
    echo "Waiting for confirmation..."
    sleep 6
    local result
    result=$(retry_command "$HUB_CLI q tx $tx_hash --node $HUB_NODE --output json" 60)
    local code
    code=$(echo "$result" | jq -r '.code // 1')
    if [ "$code" != "0" ]; then
        echo -e "${RED}Transaction failed (code $code)${NC}"
        echo "$result" | jq '.raw_log // .logs'
        exit 1
    fi
    echo "$result"
}

# ============================================================================
# Step 5a: Pause the Neutron vault
# ============================================================================

echo -e "${BLUE}=== Step 5a: Pause Neutron vault ===${NC}"
echo ""

PAUSE_OUTPUT=$($N_CLI tx wasm execute "$N_VAULT_ADDR" '{"pause":{}}' \
    --from "$ADMIN_WALLET" \
    --keyring-backend "$N_KEYRING" \
    --chain-id "$N_CHAIN_ID" \
    --node "$N_NODE" \
    --gas auto \
    --gas-prices "$N_GAS_PRICE" \
    --gas-adjustment "$N_GAS_ADJUSTMENT" \
    -y --output json 2>&1) || true

PAUSE_JSON=$(extract_json "$PAUSE_OUTPUT")
PAUSE_TX=$(echo "$PAUSE_JSON" | jq -r '.txhash')
wait_for_neutron_tx "$PAUSE_TX" > /dev/null
echo -e "${GREEN}Neutron vault is now PAUSED. No new deposits or withdrawals are accepted.${NC}"
echo ""

# ============================================================================
# Step 5b: Query Neutron Control Center pool info
# ============================================================================

echo -e "${BLUE}=== Step 5b: Query Neutron Control Center pool state ===${NC}"
echo ""

POOL_INFO=$($N_CLI q wasm contract-state smart "$N_CC_ADDR" \
    '{"pool_info":{}}' \
    --node "$N_NODE" --output json | jq -r '.data')

TOTAL_SHARES_ISSUED=$(echo "$POOL_INFO" | jq -r '.total_shares_issued')
TOTAL_POOL_VALUE=$(echo "$POOL_INFO" | jq -r '.total_pool_value')

echo "  Total pool value:    $TOTAL_POOL_VALUE"
echo "  Total shares issued: $TOTAL_SHARES_ISSUED"
echo ""

# ============================================================================
# Step 5c: Query Neutron Control Center deployed amount
# ============================================================================

echo -e "${BLUE}=== Step 5c: Query Neutron Control Center deployed amount ===${NC}"
echo ""

DEPLOYED_AMOUNT=$($N_CLI q wasm contract-state smart "$N_CC_ADDR" \
    '{"deployed_amount":{}}' \
    --node "$N_NODE" --output json | jq -r '.data')

echo "  Deployed amount (Neutron CC): $DEPLOYED_AMOUNT"
echo "  (This includes adapter deployments + the ATOM just withdrawn for IBC in step 4.)"
echo ""

# ============================================================================
# Step 5d: Call MintForMigration on Hub vault
# ============================================================================

echo -e "${BLUE}=== Step 5d: MintForMigration on Cosmos Hub vault ===${NC}"
echo ""
echo -e "${YELLOW}Parameters to be used:${NC}"
echo "  shares_to_mint:      $TOTAL_SHARES_ISSUED"
echo "  deployed_amount:     $DEPLOYED_AMOUNT"
echo "  conversion_contract: $HUB_CONVERTER_ADDR"
echo ""

MINT_MSG=$(jq -n \
    --arg shares "$TOTAL_SHARES_ISSUED" \
    --arg deployed "$DEPLOYED_AMOUNT" \
    --arg converter "$HUB_CONVERTER_ADDR" \
    '{"mint_for_migration": {"shares_to_mint": $shares, "deployed_amount": $deployed, "conversion_contract": $converter}}')

MINT_OUTPUT=$($HUB_CLI tx wasm execute "$HUB_VAULT_ADDR" "$MINT_MSG" \
    --from "$ADMIN_WALLET" \
    $HUB_TX_FLAGS \
    --output json 2>&1) || true

MINT_JSON=$(extract_json "$MINT_OUTPUT")
MINT_TX=$(echo "$MINT_JSON" | jq -r '.txhash')
MINT_RESULT=$(wait_for_hub_tx "$MINT_TX")
echo -e "${GREEN}MintForMigration successful.${NC}"
echo ""

# Print result attributes
echo "Result attributes:"
echo "$MINT_RESULT" | jq -r '.events[] | select(.type == "wasm") | .attributes[] | "  \(.key): \(.value)"' 2>/dev/null || true
echo ""

echo -e "${GREEN}=== Step 5 complete ===${NC}"
echo ""
echo "  Neutron vault: PAUSED"
echo "  Hub vault shares minted: $TOTAL_SHARES_ISSUED → Shares Converter ($HUB_CONVERTER_ADDR)"
echo "  Hub CC deployed amount set to: $DEPLOYED_AMOUNT"
