#!/bin/bash
# Step 8: User withdraws ATOM from the Cosmos Hub vault by burning their vault shares.
# If the vault has sufficient balance the withdrawal is immediate; otherwise it is queued.
#
# Usage: ./08_user_withdraw_hub.sh <cosmoshub-config> <user-wallet>
# Example: ./08_user_withdraw_hub.sh deploy-config-cosmoshub.json alice

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

HUB_VAULT_SHARES_DENOM=$(jq -r '.hub_vault_shares_denom' "$STATE_FILE")
if [ -z "$HUB_VAULT_SHARES_DENOM" ] || [ "$HUB_VAULT_SHARES_DENOM" = "null" ]; then
    echo -e "${RED}Error: hub_vault_shares_denom is not set in migration-state.json. Run step 02 first.${NC}"
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
# Step 8a: Query user's Hub vault share balance
# ============================================================================

echo -e "${BLUE}=== Step 8: Withdraw ATOM from Cosmos Hub vault ===${NC}"
echo ""

USER_ADDR=$($HUB_CLI keys show "$USER_WALLET" --keyring-backend "$HUB_KEYRING" -a)
if [ -z "$USER_ADDR" ]; then
    echo -e "${RED}Error: could not find key '$USER_WALLET' in Hub keyring${NC}"
    exit 1
fi

echo "  User address:      $USER_ADDR"
echo "  Hub shares denom:  $HUB_VAULT_SHARES_DENOM"
echo ""

SHARES_AMOUNT=$($HUB_CLI q bank balance "$USER_ADDR" "$HUB_VAULT_SHARES_DENOM" \
    --node "$HUB_NODE" --output json | jq -r '.balance.amount')

if [ -z "$SHARES_AMOUNT" ] || [ "$SHARES_AMOUNT" = "null" ] || [ "$SHARES_AMOUNT" = "0" ]; then
    echo -e "${RED}Error: no Hub vault shares found for $USER_ADDR${NC}"
    echo "  Expected denom: $HUB_VAULT_SHARES_DENOM"
    exit 1
fi

echo "  Hub vault shares to withdraw: $SHARES_AMOUNT"
echo ""

# ============================================================================
# Step 8b: Withdraw
# ============================================================================

WITHDRAW_OUTPUT=$($HUB_CLI tx wasm execute "$HUB_VAULT_ADDR" \
    '{"withdraw":{}}' \
    --amount "${SHARES_AMOUNT}${HUB_VAULT_SHARES_DENOM}" \
    --from "$USER_WALLET" \
    $HUB_TX_FLAGS \
    --output json 2>&1) || true

WITHDRAW_JSON=$(extract_json "$WITHDRAW_OUTPUT")
WITHDRAW_TX=$(echo "$WITHDRAW_JSON" | jq -r '.txhash')

if [ -z "$WITHDRAW_TX" ] || [ "$WITHDRAW_TX" = "null" ]; then
    echo -e "${RED}Withdraw failed:${NC}"
    echo "$WITHDRAW_JSON" | jq .
    exit 1
fi

echo "TX submitted: $WITHDRAW_TX"
echo "Waiting for confirmation..."
sleep 6
WITHDRAW_RESULT=$(retry_command "$HUB_CLI q tx $WITHDRAW_TX --node $HUB_NODE --output json" 60)
WITHDRAW_CODE=$(echo "$WITHDRAW_RESULT" | jq -r '.code // 1')

if [ "$WITHDRAW_CODE" != "0" ]; then
    echo -e "${RED}Withdraw failed (code $WITHDRAW_CODE)${NC}"
    echo "$WITHDRAW_RESULT" | jq '.raw_log // .logs'
    exit 1
fi

echo -e "${GREEN}Withdraw transaction confirmed.${NC}"
echo ""

# ============================================================================
# Determine if payout was immediate or queued
# ============================================================================

# An immediate payout emits a BankMsg::Send (visible as a "coin_received" or "transfer" event).
# A queued withdrawal will have a "withdrawal_queued" or similar wasm attribute.
HAS_IMMEDIATE=$(echo "$WITHDRAW_RESULT" | \
    jq -r '.events[] | select(.type == "transfer") | .attributes[] | select(.key == "recipient" and .value == "'"$USER_ADDR"'") | .value' \
    2>/dev/null | head -1)

if [ -n "$HAS_IMMEDIATE" ]; then
    RECEIVED_AMOUNT=$(echo "$WITHDRAW_RESULT" | \
        jq -r '.events[] | select(.type == "transfer") | .attributes[] | select(.key == "amount") | .value' \
        2>/dev/null | head -1)
    echo -e "${GREEN}Payout: IMMEDIATE${NC}"
    echo "  ${DEPOSIT_DENOM} received: $RECEIVED_AMOUNT"
else
    echo -e "${YELLOW}Payout: QUEUED${NC}"
    echo "  The withdrawal was queued. An admin must call FulfillPendingWithdrawals, then you can claim via ClaimUnbondedWithdrawals."
    echo ""
    echo "  Query your pending withdrawals:"
    echo "    $HUB_CLI q wasm contract-state smart $HUB_VAULT_ADDR '{\"user_withdrawal_requests\":{\"user\":\"$USER_ADDR\",\"start\":0,\"limit\":10}}' --node $HUB_NODE -o json"
fi

echo ""
echo -e "${GREEN}=== Step 8 complete ===${NC}"
