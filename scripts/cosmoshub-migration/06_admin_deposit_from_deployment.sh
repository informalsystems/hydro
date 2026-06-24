#!/bin/bash
# Step 6: Admin deposits the IBC'd ATOM into the Cosmos Hub vault via DepositFromDeployment.
# The amount is read from migration-state.json (written by step 4).
#
# Usage: ./06_admin_deposit_from_deployment.sh <cosmoshub-config> <admin-wallet>
# Example: ./06_admin_deposit_from_deployment.sh deploy-config-cosmoshub.json test-deployer

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

COSMOSHUB_CONFIG="${1:?Usage: $0 <cosmoshub-config> <admin-wallet>}"
ADMIN_WALLET="${2:?Usage: $0 <cosmoshub-config> <admin-wallet>}"

if [ ! -f "$COSMOSHUB_CONFIG" ]; then
    echo -e "${RED}Error: config file '$COSMOSHUB_CONFIG' not found${NC}"
    exit 1
fi

STATE_FILE="$SCRIPT_DIR/migration-state.json"
if [ ! -f "$STATE_FILE" ]; then
    echo -e "${RED}Error: migration-state.json not found. Run step 04 first.${NC}"
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
HUB_VAULT_ADDR=$(jq -r '.contracts.vault // empty' "$COSMOSHUB_CONFIG")
DEPOSIT_DENOM=$(jq -r '.deposit_denom' "$COSMOSHUB_CONFIG")

if [ -n "$HUB_BINARY_HOME" ]; then
    HUB_CLI="$HUB_BINARY --home $HUB_BINARY_HOME"
else
    HUB_CLI="$HUB_BINARY"
fi

HUB_TX_FLAGS="--gas auto --gas-adjustment $HUB_GAS_ADJUSTMENT --gas-prices $HUB_GAS_PRICE --chain-id $HUB_CHAIN_ID --node $HUB_NODE --keyring-backend $HUB_KEYRING -y"

if [ -z "$HUB_VAULT_ADDR" ] || [ "$HUB_VAULT_ADDR" = "null" ]; then
    echo -e "${RED}Error: contracts.vault is not set in $COSMOSHUB_CONFIG${NC}"
    exit 1
fi

IBC_AMOUNT=$(jq -r '.ibc_amount_to_hub' "$STATE_FILE")
if [ -z "$IBC_AMOUNT" ] || [ "$IBC_AMOUNT" = "null" ] || [ "$IBC_AMOUNT" = "0" ]; then
    echo -e "${RED}Error: ibc_amount_to_hub is not set in migration-state.json. Run step 04 first.${NC}"
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
# Deposit from deployment
# ============================================================================

echo -e "${BLUE}=== Step 6: DepositFromDeployment on Cosmos Hub vault ===${NC}"
echo ""
echo "  Vault:   $HUB_VAULT_ADDR"
echo "  Amount:  ${IBC_AMOUNT}${DEPOSIT_DENOM}"
echo ""

DEPOSIT_OUTPUT=$($HUB_CLI tx wasm execute "$HUB_VAULT_ADDR" \
    '{"deposit_from_deployment":{}}' \
    --amount "${IBC_AMOUNT}${DEPOSIT_DENOM}" \
    --from "$ADMIN_WALLET" \
    $HUB_TX_FLAGS \
    --output json 2>&1) || true

DEPOSIT_JSON=$(extract_json "$DEPOSIT_OUTPUT")
DEPOSIT_TX=$(echo "$DEPOSIT_JSON" | jq -r '.txhash')

if [ -z "$DEPOSIT_TX" ] || [ "$DEPOSIT_TX" = "null" ]; then
    echo -e "${RED}DepositFromDeployment failed:${NC}"
    echo "$DEPOSIT_JSON" | jq .
    exit 1
fi

echo "TX submitted: $DEPOSIT_TX"
echo "Waiting for confirmation..."
sleep 6
DEPOSIT_RESULT=$(retry_command "$HUB_CLI q tx $DEPOSIT_TX --node $HUB_NODE --output json" 60)
DEPOSIT_CODE=$(echo "$DEPOSIT_RESULT" | jq -r '.code // 1')

if [ "$DEPOSIT_CODE" != "0" ]; then
    echo -e "${RED}DepositFromDeployment failed (code $DEPOSIT_CODE)${NC}"
    echo "$DEPOSIT_RESULT" | jq '.raw_log // .logs'
    exit 1
fi

echo -e "${GREEN}DepositFromDeployment successful.${NC}"
echo ""

# Query new pool info
POOL_INFO=$($HUB_CLI q wasm contract-state smart "$HUB_VAULT_ADDR" \
    '{"pool_info":{}}' \
    --node "$HUB_NODE" --output json | jq -r '.data' 2>/dev/null || echo '{}')

echo "Hub vault pool info after deposit:"
echo "  Total pool value:    $(echo "$POOL_INFO" | jq -r '.total_pool_value // "n/a"')"
echo "  Total shares issued: $(echo "$POOL_INFO" | jq -r '.total_shares_issued // "n/a"')"
echo ""

echo -e "${GREEN}=== Step 6 complete ===${NC}"
echo ""
echo "The Cosmos Hub vault now holds ${IBC_AMOUNT} ${DEPOSIT_DENOM} from the Neutron migration."
