#!/bin/bash
# Step 4: Admin withdraws all ATOM from the Neutron vault for deployment, then
# IBC-transfers it to the admin's address on Cosmos Hub.
#
# Usage: ./04_admin_withdraw_for_deployment.sh <neutron-config> <cosmoshub-config> <admin-wallet>
# Example: ./04_admin_withdraw_for_deployment.sh deploy-config-neutron.json deploy-config-cosmoshub.json test-deployer

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
# Load configuration
# ============================================================================

N_BINARY=$(jq -r '.binary' "$NEUTRON_CONFIG")
N_BINARY_HOME=$(jq -r '.binary_home // empty' "$NEUTRON_CONFIG")
N_CHAIN_ID=$(jq -r '.chain_id' "$NEUTRON_CONFIG")
N_NODE=$(jq -r '.rpc_node' "$NEUTRON_CONFIG")
N_GAS_PRICE=$(jq -r '.gas_price' "$NEUTRON_CONFIG")
N_GAS_ADJUSTMENT=$(jq -r '.gas_adjustment' "$NEUTRON_CONFIG")
N_KEYRING=$(jq -r '.keyring_backend' "$NEUTRON_CONFIG")
DEPOSIT_DENOM=$(jq -r '.deposit_denom' "$NEUTRON_CONFIG")
DEPOSIT_DENOM_TRACE=$(jq -r '.deposit_denom_trace // empty' "$NEUTRON_CONFIG")
VAULT_ADDRESS=$(jq -r '.contracts.vault // empty' "$NEUTRON_CONFIG")

HUB_BINARY=$(jq -r '.binary' "$COSMOSHUB_CONFIG")
HUB_BINARY_HOME=$(jq -r '.binary_home // empty' "$COSMOSHUB_CONFIG")
HUB_KEYRING=$(jq -r '.keyring_backend' "$COSMOSHUB_CONFIG")

if [ -n "$N_BINARY_HOME" ]; then
    N_CLI="$N_BINARY --home $N_BINARY_HOME"
else
    N_CLI="$N_BINARY"
fi

if [ -n "$HUB_BINARY_HOME" ]; then
    HUB_CLI="$HUB_BINARY --home $HUB_BINARY_HOME"
else
    HUB_CLI="$HUB_BINARY"
fi

if [ -z "$VAULT_ADDRESS" ] || [ "$VAULT_ADDRESS" = "null" ]; then
    echo -e "${RED}Error: contracts.vault not set in $NEUTRON_CONFIG${NC}"
    exit 1
fi

if [ -z "$DEPOSIT_DENOM_TRACE" ]; then
    echo -e "${RED}Error: deposit_denom_trace must be set in $NEUTRON_CONFIG${NC}"
    exit 1
fi

# Parse IBC channel from trace: "transfer/channel-0/uatom" → "channel-0"
IBC_CHANNEL=$(echo "$DEPOSIT_DENOM_TRACE" | cut -d'/' -f2)

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

wait_for_tx() {
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

update_state() {
    local key="$1"
    local value="$2"
    local tmp
    tmp=$(mktemp)
    jq "$key = \"$value\"" "$STATE_FILE" > "$tmp" && mv "$tmp" "$STATE_FILE"
}

# ============================================================================
# Step 1: Query vault ATOM balance
# ============================================================================

echo -e "${BLUE}=== Step 4a: Query Neutron vault balance ===${NC}"
echo ""

VAULT_BALANCE=$($N_CLI q bank balance "$VAULT_ADDRESS" "$DEPOSIT_DENOM" \
    --node "$N_NODE" --output json | jq -r '.balance.amount')

if [ -z "$VAULT_BALANCE" ] || [ "$VAULT_BALANCE" = "null" ] || [ "$VAULT_BALANCE" = "0" ]; then
    echo -e "${RED}Error: Neutron vault has zero balance. Nothing to withdraw.${NC}"
    exit 1
fi

echo "  Vault:   $VAULT_ADDRESS"
echo "  Denom:   $DEPOSIT_DENOM"
echo -e "  Balance: ${GREEN}${VAULT_BALANCE}${NC}"
echo ""

# ============================================================================
# Step 2: WithdrawForDeployment
# ============================================================================

echo -e "${BLUE}=== Step 4b: Withdraw all ATOM from vault for deployment ===${NC}"
echo ""

WITHDRAW_MSG=$(jq -n --arg amount "$VAULT_BALANCE" \
    '{"withdraw_for_deployment": {"amount": $amount}}')

WITHDRAW_OUTPUT=$($N_CLI tx wasm execute "$VAULT_ADDRESS" "$WITHDRAW_MSG" \
    --from "$ADMIN_WALLET" \
    --keyring-backend "$N_KEYRING" \
    --chain-id "$N_CHAIN_ID" \
    --node "$N_NODE" \
    --gas auto \
    --gas-prices "$N_GAS_PRICE" \
    --gas-adjustment "$N_GAS_ADJUSTMENT" \
    -y --output json 2>&1) || true

WITHDRAW_JSON=$(extract_json "$WITHDRAW_OUTPUT")
WITHDRAW_TX=$(echo "$WITHDRAW_JSON" | jq -r '.txhash')
wait_for_tx "$WITHDRAW_TX" > /dev/null
echo -e "${GREEN}WithdrawForDeployment successful — ${VAULT_BALANCE} ${DEPOSIT_DENOM} sent to admin wallet${NC}"
echo ""

# ============================================================================
# Step 3: Derive Hub admin address and IBC-transfer
# ============================================================================

echo -e "${BLUE}=== Step 4c: IBC-transfer ATOM to Cosmos Hub admin ===${NC}"
echo ""

HUB_ADMIN_ADDR=$($HUB_CLI keys show "$ADMIN_WALLET" --keyring-backend "$HUB_KEYRING" -a)
if [ -z "$HUB_ADMIN_ADDR" ]; then
    echo -e "${RED}Error: could not find key '$ADMIN_WALLET' in Hub keyring${NC}"
    exit 1
fi

echo "  IBC channel:  $IBC_CHANNEL (Neutron → Hub)"
echo "  Recipient:    $HUB_ADMIN_ADDR"
echo "  Amount:       ${VAULT_BALANCE}${DEPOSIT_DENOM}"
echo ""

IBC_OUTPUT=$($N_CLI tx ibc-transfer transfer transfer "$IBC_CHANNEL" "$HUB_ADMIN_ADDR" \
    "${VAULT_BALANCE}${DEPOSIT_DENOM}" \
    --from "$ADMIN_WALLET" \
    --keyring-backend "$N_KEYRING" \
    --chain-id "$N_CHAIN_ID" \
    --node "$N_NODE" \
    --gas auto \
    --gas-prices "$N_GAS_PRICE" \
    --gas-adjustment "$N_GAS_ADJUSTMENT" \
    -y --output json 2>&1) || true

IBC_JSON=$(extract_json "$IBC_OUTPUT")
IBC_TX=$(echo "$IBC_JSON" | jq -r '.txhash')
wait_for_tx "$IBC_TX" > /dev/null
echo -e "${GREEN}IBC transfer submitted successfully.${NC}"
echo "Note: IBC packets take a few seconds to relay before appearing on Cosmos Hub."
echo ""

# ============================================================================
# Persist state
# ============================================================================

update_state ".ibc_amount_to_hub" "$VAULT_BALANCE"

echo -e "${GREEN}=== Step 4 complete ===${NC}"
echo ""
echo "  Amount IBC'd to Hub: $VAULT_BALANCE $DEPOSIT_DENOM"
echo "  Hub admin address:   $HUB_ADMIN_ADDR"
echo ""
echo "Verify Hub admin balance (wait for relay):"
echo "  $HUB_CLI q bank balance $HUB_ADMIN_ADDR uatom --node $(jq -r '.rpc_node' "$COSMOSHUB_CONFIG") -o json"
