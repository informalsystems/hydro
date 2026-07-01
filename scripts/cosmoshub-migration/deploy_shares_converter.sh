#!/bin/bash
# Deploy a shared Shares Converter on Cosmos Hub and register all vault pairs.
# Idempotent: skips store/instantiate if already done, skips pairs already registered.
#
# Usage: ./deploy_shares_converter.sh <config>
# Example: ./deploy_shares_converter.sh deploy-config-shares-converter-mainnet.json

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

CONFIG="${1:?Usage: $0 <config>}"
if [ ! -f "$CONFIG" ]; then
    echo -e "${RED}Error: config file '$CONFIG' not found${NC}"
    exit 1
fi

# ============================================================================
# Load configuration
# ============================================================================

BINARY=$(jq -r '.binary' "$CONFIG")
BINARY_HOME=$(jq -r '.binary_home // empty' "$CONFIG")
CHAIN_ID=$(jq -r '.chain_id' "$CONFIG")
NODE=$(jq -r '.rpc_node' "$CONFIG")
GAS_PRICE=$(jq -r '.gas_price' "$CONFIG")
GAS_ADJUSTMENT=$(jq -r '.gas_adjustment' "$CONFIG")
DEPLOYER_WALLET=$(jq -r '.deployer_wallet' "$CONFIG")
ADMIN_ADDRESS=$(jq -r '.admin_address' "$CONFIG")
ADMIN_WALLET=$(jq -r '.admin_wallet // empty' "$CONFIG")
KEYRING=$(jq -r '.keyring_backend' "$CONFIG")
WASM=$(jq -r '.shares_converter_wasm' "$CONFIG")
IBC_CHANNEL=$(jq -r '.ibc_channel_to_neutron' "$CONFIG")

if [ -n "$BINARY_HOME" ]; then
    CLI="$BINARY --home $BINARY_HOME"
else
    CLI="$BINARY"
fi

NODE_FLAG="--node $NODE"
KEYRING_FLAG="--keyring-backend $KEYRING"
TX_FLAGS="--gas auto --gas-adjustment $GAS_ADJUSTMENT --gas-prices $GAS_PRICE --chain-id $CHAIN_ID $NODE_FLAG $KEYRING_FLAG -y"

if [ -z "$WASM" ] || [ ! -f "$WASM" ]; then
    echo -e "${RED}Error: shares_converter_wasm not found at '$WASM'${NC}"
    exit 1
fi

# ============================================================================
# Helpers
# ============================================================================

sha256_upper() {
    local input="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        printf '%s' "$input" | sha256sum | awk '{print toupper($1)}'
    else
        printf '%s' "$input" | shasum -a 256 | awk '{print toupper($1)}'
    fi
}

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
    echo "TX submitted: $tx_hash"
    echo "Waiting for confirmation..."
    sleep 6
    local result
    result=$(retry_command "$CLI q tx $tx_hash $NODE_FLAG --output json" 60)
    local code
    code=$(echo "$result" | jq -r '.code // 1')
    if [ "$code" != "0" ]; then
        echo -e "${RED}Transaction failed (code $code)${NC}"
        echo "$result" | jq '.raw_log // .logs'
        exit 1
    fi
    echo "$result"
}

update_config() {
    local key="$1"
    local value="$2"
    local tmp
    tmp=$(mktemp)
    jq "$key = \"$value\"" "$CONFIG" > "$tmp" && mv "$tmp" "$CONFIG"
}

# Execute an admin-only tx.
# If admin_wallet is set in config, signs with that keyring key.
# Otherwise prints the message to console and waits for manual confirmation (e.g. via DAODAO).
exec_admin_tx() {
    local label="$1" contract="$2" msg="$3" res_file="$4"

    if [ -n "$ADMIN_WALLET" ]; then
        $CLI tx wasm execute "$contract" "$msg" \
            --from "$ADMIN_WALLET" \
            $TX_FLAGS --output json \
            &> "$res_file"
        TX_HASH=$(grep -o '{.*}' "$res_file" | jq -r '.txhash')
        retry_command "$CLI q tx $TX_HASH $NODE_FLAG --output json" 60 > /dev/null
    else
        echo -e "${YELLOW}admin_wallet not set — execute the following on ${label} (${contract}):${NC}"
        echo ""
        echo "$msg" | jq .
        echo ""
        read -p "Press Enter once the transaction is confirmed..."
    fi
}

# ============================================================================
# Step 1: Store wasm
# ============================================================================

echo -e "${BLUE}=== Step 1: Store Shares Converter wasm ===${NC}"
echo ""

CODE_ID=$(jq -r '.code_ids.shares_converter // empty' "$CONFIG")

if [ -n "$CODE_ID" ] && [ "$CODE_ID" != "null" ]; then
    echo -e "${YELLOW}Existing code ID: $CODE_ID — skipping store.${NC}"
else
    printf "Storing shares_converter wasm"
    $CLI tx wasm store "$WASM" \
        --from "$DEPLOYER_WALLET" \
        $TX_FLAGS --output json \
        &> ./store_shares_converter_res.json

    TX_HASH=$(grep -o '{.*}' ./store_shares_converter_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$CLI q tx $TX_HASH $NODE_FLAG --output json" 60)
    CODE_ID=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "store_code") | .attributes[] | select(.key == "code_id") | .value')
    echo "Stored with code ID: $CODE_ID"
    update_config ".code_ids.shares_converter" "$CODE_ID"
fi

echo ""

# ============================================================================
# Step 2: Instantiate
# ============================================================================

echo -e "${BLUE}=== Step 2: Instantiate Shares Converter ===${NC}"
echo ""

CONVERTER_ADDR=$(jq -r '.contracts.shares_converter // empty' "$CONFIG")

if [ -n "$CONVERTER_ADDR" ] && [ "$CONVERTER_ADDR" != "null" ]; then
    echo -e "${YELLOW}Existing Shares Converter: $CONVERTER_ADDR — skipping instantiate.${NC}"
else
    printf "Instantiating Shares Converter"

    INIT_MSG=$(jq -n --arg admin "$ADMIN_ADDRESS" '{"admin": $admin, "pairs": []}')

    $CLI tx wasm instantiate "$CODE_ID" "$INIT_MSG" \
        --admin "$ADMIN_ADDRESS" \
        --label "Inflow Shares Converter" \
        --from "$DEPLOYER_WALLET" \
        $TX_FLAGS --output json \
        &> ./instantiate_shares_converter_res.json

    TX_HASH=$(grep -o '{.*}' ./instantiate_shares_converter_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$CLI q tx $TX_HASH $NODE_FLAG --output json" 60)
    CONVERTER_ADDR=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')
    echo "Instantiated at: $CONVERTER_ADDR"
    update_config ".contracts.shares_converter" "$CONVERTER_ADDR"
fi

echo ""

# ============================================================================
# Step 3: Register pairs
# ============================================================================

echo -e "${BLUE}=== Step 3: Register vault pairs ===${NC}"
echo ""

PAIR_COUNT=$(jq '.pairs | length' "$CONFIG")

for i in $(seq 0 $((PAIR_COUNT - 1))); do
    NAME=$(jq -r ".pairs[$i].name" "$CONFIG")
    NEUTRON_DENOM=$(jq -r ".pairs[$i].neutron_shares_denom" "$CONFIG")
    HUB_DENOM=$(jq -r ".pairs[$i].hub_shares_denom" "$CONFIG")

    # Compute IBC denom of the Neutron factory token as seen on Hub
    TRACE="transfer/${IBC_CHANNEL}/${NEUTRON_DENOM}"
    IBC_HASH=$(sha256_upper "$TRACE")
    IBC_DENOM="ibc/${IBC_HASH}"

    echo "Pair: $NAME"
    echo "  Neutron denom:  $NEUTRON_DENOM"
    echo "  IBC denom:      $IBC_DENOM"
    echo "  Hub denom:      $HUB_DENOM"

    # Check if pair already registered
    QUERY=$(jq -n --arg denom "$IBC_DENOM" '{"pair": {"neutron_denom": $denom}}')
    EXISTING=$($CLI query wasm contract-state smart "$CONVERTER_ADDR" "$QUERY" \
        $NODE_FLAG --output json 2>/dev/null | jq -r '.data')

    if [ "$EXISTING" != "null" ] && [ -n "$EXISTING" ]; then
        echo -e "  ${YELLOW}Already registered — skipping.${NC}"
        echo ""
        continue
    fi

    ADD_PAIR_MSG=$(jq -n \
        --arg neutron "$IBC_DENOM" \
        --arg hub "$HUB_DENOM" \
        '{"add_pair": {"neutron_shares_denom": $neutron, "cosmos_hub_shares_denom": $hub}}')

    exec_admin_tx "Shares Converter" "$CONVERTER_ADDR" "$ADD_PAIR_MSG" "./add_pair_${NAME}_res.json"
    echo -e "  ${GREEN}Registered.${NC}"
    echo ""
done

# ============================================================================
# Summary
# ============================================================================

echo -e "${GREEN}=== Done ===${NC}"
echo ""
echo "Shares Converter: $CONVERTER_ADDR"
echo "Code ID:          $CODE_ID"
echo ""
echo "Registered pairs:"
for i in $(seq 0 $((PAIR_COUNT - 1))); do
    NAME=$(jq -r ".pairs[$i].name" "$CONFIG")
    echo "  $NAME"
done
