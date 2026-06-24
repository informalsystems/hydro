#!/bin/bash
# Deposit into the Inflow Vault on Neutron and IBC-transfer the received shares to Cosmos Hub.
# The Cosmos Hub recipient address is derived by querying the Cosmos Hub keyring for the same
# wallet name.
#
# Usage:
#   ./deposit-neutron-then-ibc.sh <neutron-config> <cosmoshub-config> <from-wallet> <amount-uatom>
#
# Example:
#   ./deposit-neutron-then-ibc.sh deploy-config-neutron.json deploy-config-cosmoshub.json alice 1000000000

set -eo pipefail

RED='\033[0;31m'
# Extract the JSON object from CLI output, skipping any leading non-JSON lines (e.g. "gas estimate: ...")
extract_json() { echo "$1" | awk '/^\{/{p=1} p{print}'; }
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

NEUTRON_CONFIG="${1:-}"
COSMOSHUB_CONFIG="${2:-}"
FROM_WALLET="${3:-}"
AMOUNT_UATOM="${4:-}"

if [ -z "$NEUTRON_CONFIG" ] || [ -z "$COSMOSHUB_CONFIG" ] || [ -z "$FROM_WALLET" ] || [ -z "$AMOUNT_UATOM" ]; then
    echo -e "${RED}Usage: $0 <neutron-config> <cosmoshub-config> <from-wallet> <amount-uatom>${NC}"
    echo ""
    echo "  neutron-config    path to Neutron deploy config (e.g. deploy-config-neutron.json)"
    echo "  cosmoshub-config  path to Cosmos Hub deploy config (e.g. deploy-config-cosmoshub.json)"
    echo "  from-wallet       key name in keyring (e.g. alice)"
    echo "  amount-uatom      amount in micro-units (e.g. 1000000000 for 1000 ATOM)"
    exit 1
fi

for f in "$NEUTRON_CONFIG" "$COSMOSHUB_CONFIG"; do
    if [ ! -f "$f" ]; then
        echo -e "${RED}Error: config file '$f' not found${NC}"
        exit 1
    fi
done

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
DEPOSIT_DENOM=$(jq -r '.deposit_denom' "$NEUTRON_CONFIG")
DEPOSIT_DENOM_TRACE=$(jq -r '.deposit_denom_trace // empty' "$NEUTRON_CONFIG")
VAULT_SUBDENOM=$(jq -r '.vault_subdenom' "$NEUTRON_CONFIG")
VAULT_ADDRESS=$(jq -r '.contracts.vault // empty' "$NEUTRON_CONFIG")

if [ -n "$N_BINARY_HOME" ]; then
    N_CLI="$N_BINARY --home $N_BINARY_HOME"
else
    N_CLI="$N_BINARY"
fi

if [ -z "$VAULT_ADDRESS" ] || [ "$VAULT_ADDRESS" = "null" ]; then
    echo -e "${RED}Error: contracts.vault is not set in $NEUTRON_CONFIG${NC}"
    exit 1
fi

if [ -z "$DEPOSIT_DENOM_TRACE" ]; then
    echo -e "${RED}Error: deposit_denom_trace must be set in $NEUTRON_CONFIG (e.g. transfer/channel-0/uatom)${NC}"
    exit 1
fi

# Parse IBC channel from trace: "transfer/channel-0/uatom" → "channel-0"
IBC_CHANNEL=$(echo "$DEPOSIT_DENOM_TRACE" | cut -d'/' -f2)

SHARES_DENOM="factory/${VAULT_ADDRESS}/${VAULT_SUBDENOM}"

# ============================================================================
# Load Cosmos Hub configuration and derive recipient address
# ============================================================================

HUB_BINARY=$(jq -r '.binary' "$COSMOSHUB_CONFIG")
HUB_BINARY_HOME=$(jq -r '.binary_home // empty' "$COSMOSHUB_CONFIG")
HUB_NODE=$(jq -r '.rpc_node' "$COSMOSHUB_CONFIG")
HUB_KEYRING=$(jq -r '.keyring_backend' "$COSMOSHUB_CONFIG")

if [ -n "$HUB_BINARY_HOME" ]; then
    HUB_CLI="$HUB_BINARY --home $HUB_BINARY_HOME"
else
    HUB_CLI="$HUB_BINARY"
fi

COSMOS_HUB_RECIPIENT=$($HUB_CLI keys show "$FROM_WALLET" --keyring-backend "$HUB_KEYRING" -a)

if [ -z "$COSMOS_HUB_RECIPIENT" ]; then
    echo -e "${RED}Error: could not find key '$FROM_WALLET' in $HUB_BINARY keyring${NC}"
    exit 1
fi

# ============================================================================
# Step 1: Deposit
# ============================================================================

echo -e "${YELLOW}Step 1: Depositing into vault${NC}"
echo "  Chain:   $N_CHAIN_ID"
echo "  Vault:   $VAULT_ADDRESS"
echo "  From:    $FROM_WALLET"
echo "  Amount:  ${AMOUNT_UATOM}${DEPOSIT_DENOM}"
echo ""

DEPOSITOR_ADDR=$($N_CLI keys show "$FROM_WALLET" --keyring-backend "$N_KEYRING" -a)

DEPOSIT_OUTPUT=$($N_CLI tx wasm execute "$VAULT_ADDRESS" \
    '{"deposit": {}}' \
    --amount "${AMOUNT_UATOM}${DEPOSIT_DENOM}" \
    --from "$FROM_WALLET" \
    --keyring-backend "$N_KEYRING" \
    --chain-id "$N_CHAIN_ID" \
    --node "$N_NODE" \
    --gas auto \
    --gas-prices "$N_GAS_PRICE" \
    --gas-adjustment "$N_GAS_ADJUSTMENT" \
    -y \
    --output json 2>&1) || true

DEPOSIT_JSON=$(extract_json "$DEPOSIT_OUTPUT")
if ! echo "$DEPOSIT_JSON" | jq -e . >/dev/null 2>&1; then
    echo -e "${RED}Deposit failed:${NC}"
    echo "$DEPOSIT_OUTPUT"
    exit 1
fi

DEPOSIT_TX=$(echo "$DEPOSIT_JSON" | jq -r '.txhash')
if [ -z "$DEPOSIT_TX" ] || [ "$DEPOSIT_TX" = "null" ]; then
    echo -e "${RED}Deposit failed:${NC}"
    echo "$DEPOSIT_JSON" | jq .
    exit 1
fi

echo "TX submitted: $DEPOSIT_TX"
echo "Waiting for confirmation..."
sleep 6

DEPOSIT_RESULT=$($N_CLI q tx "$DEPOSIT_TX" --node "$N_NODE" --output json 2>/dev/null)
DEPOSIT_CODE=$(echo "$DEPOSIT_RESULT" | jq -r '.code // 1')

if [ "$DEPOSIT_CODE" != "0" ]; then
    echo -e "${RED}Deposit failed (code $DEPOSIT_CODE)${NC}"
    echo "$DEPOSIT_RESULT" | jq '.raw_log // .logs'
    exit 1
fi

echo -e "${GREEN}Deposit successful!${NC}"
echo ""

# ============================================================================
# Step 2: Query received shares
# ============================================================================

echo -e "${YELLOW}Step 2: Querying vault shares balance${NC}"
echo "  Address: $DEPOSITOR_ADDR"
echo "  Denom:   $SHARES_DENOM"
echo ""

SHARES_AMOUNT=$($N_CLI q bank balance "$DEPOSITOR_ADDR" "$SHARES_DENOM" \
    --node "$N_NODE" \
    --output json | jq -r '.balance.amount')

if [ -z "$SHARES_AMOUNT" ] || [ "$SHARES_AMOUNT" = "null" ] || [ "$SHARES_AMOUNT" = "0" ]; then
    echo -e "${RED}Error: no vault shares found for $DEPOSITOR_ADDR${NC}"
    exit 1
fi

echo -e "${GREEN}Shares balance: ${SHARES_AMOUNT}${SHARES_DENOM}${NC}"
echo ""

# ============================================================================
# Step 3: IBC transfer shares to Cosmos Hub
# ============================================================================

echo -e "${YELLOW}Step 3: IBC transferring shares to Cosmos Hub${NC}"
echo "  Channel:   $IBC_CHANNEL"
echo "  Recipient: $COSMOS_HUB_RECIPIENT"
echo "  Amount:    ${SHARES_AMOUNT}${SHARES_DENOM}"
echo ""

IBC_OUTPUT=$($N_CLI tx ibc-transfer transfer transfer "$IBC_CHANNEL" "$COSMOS_HUB_RECIPIENT" \
    "${SHARES_AMOUNT}${SHARES_DENOM}" \
    --from "$FROM_WALLET" \
    --keyring-backend "$N_KEYRING" \
    --chain-id "$N_CHAIN_ID" \
    --node "$N_NODE" \
    --gas auto \
    --gas-prices "$N_GAS_PRICE" \
    --gas-adjustment "$N_GAS_ADJUSTMENT" \
    -y \
    --output json 2>&1) || true

IBC_JSON=$(extract_json "$IBC_OUTPUT")
if ! echo "$IBC_JSON" | jq -e . >/dev/null 2>&1; then
    echo -e "${RED}IBC transfer failed:${NC}"
    echo "$IBC_OUTPUT"
    exit 1
fi

IBC_TX=$(echo "$IBC_JSON" | jq -r '.txhash')
if [ -z "$IBC_TX" ] || [ "$IBC_TX" = "null" ]; then
    echo -e "${RED}IBC transfer failed:${NC}"
    echo "$IBC_JSON" | jq .
    exit 1
fi

echo "TX submitted: $IBC_TX"
echo "Waiting for confirmation..."
sleep 6

IBC_RESULT=$($N_CLI q tx "$IBC_TX" --node "$N_NODE" --output json 2>/dev/null)
IBC_CODE=$(echo "$IBC_RESULT" | jq -r '.code // 1')

if [ "$IBC_CODE" != "0" ]; then
    echo -e "${RED}IBC transfer failed (code $IBC_CODE)${NC}"
    echo "$IBC_RESULT" | jq '.raw_log // .logs'
    exit 1
fi

echo -e "${GREEN}IBC transfer submitted successfully!${NC}"
echo ""
echo "Note: IBC packets take a few seconds to relay. Check the recipient balance on Cosmos Hub:"
echo "  $HUB_CLI q bank balances $COSMOS_HUB_RECIPIENT --node $HUB_NODE -o json"
