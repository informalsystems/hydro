#!/bin/bash
# Step 2: Deploy Control Center + Vault on Cosmos Hub, then deploy the Shares Converter
# and register the Neutron→Hub share pair.
#
# Usage: ./02_deploy_cosmoshub.sh <neutron-config> <cosmoshub-config>
# Example: ./02_deploy_cosmoshub.sh deploy-config-neutron.json deploy-config-cosmoshub.json

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

NEUTRON_CONFIG="${1:?Usage: $0 <neutron-config> <cosmoshub-config>}"
COSMOSHUB_CONFIG="${2:?Usage: $0 <neutron-config> <cosmoshub-config>}"

for f in "$NEUTRON_CONFIG" "$COSMOSHUB_CONFIG"; do
    if [ ! -f "$f" ]; then
        echo -e "${RED}Error: config file '$f' not found${NC}"
        exit 1
    fi
done

STATE_FILE="$SCRIPT_DIR/migration-state.json"

# ============================================================================
# Compute SHA256-based IBC denom (works before any packet has been relayed)
# ============================================================================

sha256_upper() {
    local input="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        printf '%s' "$input" | sha256sum | awk '{print toupper($1)}'
    else
        printf '%s' "$input" | shasum -a 256 | awk '{print toupper($1)}'
    fi
}

# ============================================================================
# Load configuration
# ============================================================================

HUB_BINARY=$(jq -r '.binary' "$COSMOSHUB_CONFIG")
HUB_BINARY_HOME=$(jq -r '.binary_home // empty' "$COSMOSHUB_CONFIG")
HUB_CHAIN_ID=$(jq -r '.chain_id' "$COSMOSHUB_CONFIG")
HUB_NODE=$(jq -r '.rpc_node' "$COSMOSHUB_CONFIG")
HUB_GAS_PRICE=$(jq -r '.gas_price' "$COSMOSHUB_CONFIG")
HUB_GAS_ADJUSTMENT=$(jq -r '.gas_adjustment' "$COSMOSHUB_CONFIG")
HUB_DEPLOYER_WALLET=$(jq -r '.deployer_wallet' "$COSMOSHUB_CONFIG")
HUB_ADMIN_ADDRESS=$(jq -r '.admin_address' "$COSMOSHUB_CONFIG")
HUB_KEYRING=$(jq -r '.keyring_backend' "$COSMOSHUB_CONFIG")
SHARES_CONVERTER_WASM=$(jq -r '.shares_converter_wasm // empty' "$COSMOSHUB_CONFIG")
IBC_CHANNEL_TO_NEUTRON=$(jq -r '.ibc_channel_to_neutron' "$COSMOSHUB_CONFIG")
HUB_VAULT_SUBDENOM=$(jq -r '.vault_subdenom' "$COSMOSHUB_CONFIG")

NEUTRON_VAULT_ADDR=$(jq -r '.contracts.vault // empty' "$NEUTRON_CONFIG")
NEUTRON_VAULT_SUBDENOM=$(jq -r '.vault_subdenom' "$NEUTRON_CONFIG")

if [ -n "$HUB_BINARY_HOME" ]; then
    HUB_CLI="$HUB_BINARY --home $HUB_BINARY_HOME"
else
    HUB_CLI="$HUB_BINARY"
fi

HUB_NODE_FLAG="--node $HUB_NODE"
HUB_KEYRING_FLAG="--keyring-backend $HUB_KEYRING"
HUB_TX_FLAGS="--gas auto --gas-adjustment $HUB_GAS_ADJUSTMENT --gas-prices $HUB_GAS_PRICE --chain-id $HUB_CHAIN_ID $HUB_NODE_FLAG $HUB_KEYRING_FLAG -y"

if [ -z "$SHARES_CONVERTER_WASM" ]; then
    echo -e "${RED}Error: shares_converter_wasm must be set in $COSMOSHUB_CONFIG${NC}"
    exit 1
fi

if [ -z "$NEUTRON_VAULT_ADDR" ] || [ "$NEUTRON_VAULT_ADDR" = "null" ]; then
    echo -e "${RED}Error: contracts.vault is not set in $NEUTRON_CONFIG. Deploy Neutron first (step 01).${NC}"
    exit 1
fi

# ============================================================================
# Helper functions
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

wait_for_tx() {
    local tx_hash="$1"
    local cli_cmd="$2"
    local node_flag="$3"
    echo "TX submitted: $tx_hash"
    echo "Waiting for confirmation..."
    sleep 6
    local result
    result=$(retry_command "$cli_cmd q tx $tx_hash $node_flag --output json" 60)
    local code
    code=$(echo "$result" | jq -r '.code // 1')
    if [ "$code" != "0" ]; then
        echo -e "${RED}Transaction failed (code $code)${NC}"
        echo "$result" | jq '.raw_log // .logs'
        exit 1
    fi
    echo "$result"
}

update_hub_config() {
    local key="$1"
    local value="$2"
    local tmp
    tmp=$(mktemp)
    jq "$key = \"$value\"" "$COSMOSHUB_CONFIG" > "$tmp" && mv "$tmp" "$COSMOSHUB_CONFIG"
}

update_state() {
    local key="$1"
    local value="$2"
    local tmp
    tmp=$(mktemp)
    jq "$key = \"$value\"" "$STATE_FILE" > "$tmp" && mv "$tmp" "$STATE_FILE"
}

# ============================================================================
# Step 2a: Deploy Control Center + Vault via the shared deploy script
# ============================================================================

echo -e "${BLUE}=== Step 2a: Deploy Control Center + Vault on Cosmos Hub ===${NC}"
echo ""
bash "$SCRIPT_DIR/deploy-inflow-vault.sh" "$COSMOSHUB_CONFIG"
echo ""

# Reload vault address (the deploy script persists it)
HUB_VAULT_ADDR=$(jq -r '.contracts.vault' "$COSMOSHUB_CONFIG")
HUB_CONTROL_CENTER_ADDR=$(jq -r '.contracts.control_center' "$COSMOSHUB_CONFIG")
HUB_SHARES_CONVERTER_CODE_ID=$(jq -r '.code_ids.shares_converter // empty' "$COSMOSHUB_CONFIG")
HUB_SHARES_CONVERTER_ADDR=$(jq -r '.contracts.shares_converter // empty' "$COSMOSHUB_CONFIG")

# ============================================================================
# Step 2b: Compute IBC denom of Neutron shares on Hub
# ============================================================================

echo -e "${BLUE}=== Step 2b: Compute IBC denom of Neutron vault shares on Hub ===${NC}"
echo ""

HUB_VAULT_SHARES_DENOM="factory/${HUB_VAULT_ADDR}/${HUB_VAULT_SUBDENOM}"
NEUTRON_SHARES_TRACE="transfer/${IBC_CHANNEL_TO_NEUTRON}/factory/${NEUTRON_VAULT_ADDR}/${NEUTRON_VAULT_SUBDENOM}"
IBC_HASH=$(sha256_upper "$NEUTRON_SHARES_TRACE")
NEUTRON_SHARES_IBC_DENOM="ibc/${IBC_HASH}"

echo "  Neutron vault shares denom:        factory/${NEUTRON_VAULT_ADDR}/${NEUTRON_VAULT_SUBDENOM}"
echo "  IBC trace (Hub perspective):       $NEUTRON_SHARES_TRACE"
echo "  IBC denom on Hub:                  $NEUTRON_SHARES_IBC_DENOM"
echo "  Hub vault shares denom:            $HUB_VAULT_SHARES_DENOM"
echo ""

# ============================================================================
# Step 2c: Deploy Shares Converter
# ============================================================================

echo -e "${BLUE}=== Step 2c: Deploy Shares Converter ===${NC}"
echo ""

if [ -n "$HUB_SHARES_CONVERTER_CODE_ID" ] && [ "$HUB_SHARES_CONVERTER_CODE_ID" != "null" ]; then
    echo -e "${YELLOW}Existing Shares Converter code ID: $HUB_SHARES_CONVERTER_CODE_ID${NC}"
    read -p "Redeploy Shares Converter code? (y/N): " -n 1 -r; echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        HUB_SHARES_CONVERTER_CODE_ID=""
    fi
fi

if [ -z "$HUB_SHARES_CONVERTER_CODE_ID" ] || [ "$HUB_SHARES_CONVERTER_CODE_ID" = "null" ]; then
    echo "No existing Shares Converter code ID. Deploying..."
    printf "Storing shares_converter wasm"
    $HUB_CLI tx wasm store "$SHARES_CONVERTER_WASM" \
        --from "$HUB_DEPLOYER_WALLET" \
        $HUB_TX_FLAGS --output json \
        &> ./store_shares_converter_res.json

    TX_HASH=$(grep -o '{.*}' ./store_shares_converter_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$HUB_CLI q tx $TX_HASH $HUB_NODE_FLAG --output json" 60)
    HUB_SHARES_CONVERTER_CODE_ID=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "store_code") | .attributes[] | select(.key == "code_id") | .value')
    echo "Shares Converter stored with code ID: $HUB_SHARES_CONVERTER_CODE_ID"
    update_hub_config ".code_ids.shares_converter" "$HUB_SHARES_CONVERTER_CODE_ID"
fi

if [ -n "$HUB_SHARES_CONVERTER_ADDR" ] && [ "$HUB_SHARES_CONVERTER_ADDR" != "null" ]; then
    echo -e "${YELLOW}Existing Shares Converter: $HUB_SHARES_CONVERTER_ADDR${NC}"
    read -p "Reinstantiate Shares Converter? (y/N): " -n 1 -r; echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Using existing Shares Converter at: $HUB_SHARES_CONVERTER_ADDR"
    else
        HUB_SHARES_CONVERTER_ADDR=""
    fi
fi

if [ -z "$HUB_SHARES_CONVERTER_ADDR" ] || [ "$HUB_SHARES_CONVERTER_ADDR" = "null" ]; then
    printf "Instantiating Shares Converter"

    CONVERTER_INIT_MSG=$(jq -n \
        --arg admin "$HUB_ADMIN_ADDRESS" \
        '{ "admin": $admin, "pairs": [] }')

    $HUB_CLI tx wasm instantiate "$HUB_SHARES_CONVERTER_CODE_ID" "$CONVERTER_INIT_MSG" \
        --admin "$HUB_ADMIN_ADDRESS" \
        --label "Inflow Shares Converter" \
        --from "$HUB_DEPLOYER_WALLET" \
        $HUB_TX_FLAGS --output json \
        &> ./instantiate_shares_converter_res.json

    TX_HASH=$(grep -o '{.*}' ./instantiate_shares_converter_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$HUB_CLI q tx $TX_HASH $HUB_NODE_FLAG --output json" 60)
    HUB_SHARES_CONVERTER_ADDR=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')
    echo "Shares Converter instantiated at: $HUB_SHARES_CONVERTER_ADDR"
    update_hub_config ".contracts.shares_converter" "$HUB_SHARES_CONVERTER_ADDR"
fi

# ============================================================================
# Step 2d: Register conversion pair
# ============================================================================

echo ""
echo -e "${BLUE}=== Step 2d: Register Neutron↔Hub share conversion pair ===${NC}"
echo ""
echo "  Neutron IBC denom: $NEUTRON_SHARES_IBC_DENOM"
echo "  Hub shares denom:  $HUB_VAULT_SHARES_DENOM"
echo ""

ADD_PAIR_MSG=$(jq -n \
    --arg neutron "$NEUTRON_SHARES_IBC_DENOM" \
    --arg hub "$HUB_VAULT_SHARES_DENOM" \
    '{"add_pair": {"neutron_shares_denom": $neutron, "cosmos_hub_shares_denom": $hub}}')

ADD_PAIR_OUTPUT=$($HUB_CLI tx wasm execute "$HUB_SHARES_CONVERTER_ADDR" "$ADD_PAIR_MSG" \
    --from "$HUB_DEPLOYER_WALLET" \
    $HUB_TX_FLAGS --output json 2>&1) || true

ADD_PAIR_JSON=$(extract_json "$ADD_PAIR_OUTPUT")
TX_HASH=$(echo "$ADD_PAIR_JSON" | jq -r '.txhash')
wait_for_tx "$TX_HASH" "$HUB_CLI" "$HUB_NODE_FLAG" > /dev/null
echo "Conversion pair registered."

# ============================================================================
# Persist state
# ============================================================================

update_state ".neutron_shares_ibc_denom_on_hub" "$NEUTRON_SHARES_IBC_DENOM"
update_state ".hub_vault_shares_denom" "$HUB_VAULT_SHARES_DENOM"

echo ""
echo -e "${GREEN}=== Step 2 complete ===${NC}"
echo ""
echo "Hub contracts:"
echo "  Control Center:    $HUB_CONTROL_CENTER_ADDR"
echo "  Vault:             $HUB_VAULT_ADDR"
echo "  Shares Converter:  $HUB_SHARES_CONVERTER_ADDR"
echo ""
echo "Share denoms:"
echo "  Hub vault shares:           $HUB_VAULT_SHARES_DENOM"
echo "  Neutron shares IBC denom:   $NEUTRON_SHARES_IBC_DENOM"
echo ""
echo "State saved to: $STATE_FILE"
