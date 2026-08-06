#!/bin/bash
set -euo pipefail

STRIDE_API_URL="${STRIDE_API_URL:-https://stride-api.polkachu.com}"
HOST_ZONE_ID="${HOST_ZONE_ID:-cosmoshub-4}"

RESPONSE=$(curl -sf "${STRIDE_API_URL}/Stride-Labs/stride/stakeibc/host_zone/${HOST_ZONE_ID}")

REDEMPTION_RATE=$(echo "$RESPONSE" | grep -o '"redemption_rate":"[^"]*"' | head -1 | sed 's/.*:"\(.*\)"/\1/')

echo "stATOM ratio: $REDEMPTION_RATE"

: "${CONTRACT_ADDRESS:?CONTRACT_ADDRESS env var must be set to the trusted-derivative-token-info-provider contract address}"
: "${GAIA_WALLET:?GAIA_WALLET env var must be set to the keyring key name to sign with}"

GAIA_BINARY="${GAIA_BINARY:-gaiad}"
GAIA_CHAIN_ID="${GAIA_CHAIN_ID:-cosmoshub-4}"
GAIA_NODE="${GAIA_NODE:-https://cosmos-rpc.publicnode.com:443}"
GAIA_HOME="${GAIA_HOME:-$HOME/.gaia}"
GAIA_KEYRING_BACKEND="${GAIA_KEYRING_BACKEND:-test}"
GAIA_GAS_PRICES="${GAIA_GAS_PRICES:-0.005uatom}"
GAIA_GAS_ADJUSTMENT="${GAIA_GAS_ADJUSTMENT:-1.3}"
TX_CONFIRM_TIMEOUT_SECONDS="${TX_CONFIRM_TIMEOUT_SECONDS:-60}"
TX_CONFIRM_POLL_INTERVAL_SECONDS="${TX_CONFIRM_POLL_INTERVAL_SECONDS:-3}"

EXECUTE_MSG=$(printf '{"submit_token_ratio":{"ratio":"%s"}}' "$REDEMPTION_RATE")

GAIA_TX_FLAGS=(
  --chain-id "$GAIA_CHAIN_ID"
  --node "$GAIA_NODE"
  --home "$GAIA_HOME"
  --keyring-backend "$GAIA_KEYRING_BACKEND"
  --gas auto
  --gas-adjustment "$GAIA_GAS_ADJUSTMENT"
  --gas-prices "$GAIA_GAS_PRICES"
  --broadcast-mode sync
  -y
  -o json
)

BROADCAST_RESULT=$("$GAIA_BINARY" tx wasm execute "$CONTRACT_ADDRESS" "$EXECUTE_MSG" --from "$GAIA_WALLET" "${GAIA_TX_FLAGS[@]}")

BROADCAST_CODE=$(echo "$BROADCAST_RESULT" | jq -r '.code')
TX_HASH=$(echo "$BROADCAST_RESULT" | jq -r '.txhash')

if [ "$BROADCAST_CODE" != "0" ]; then
  echo "ERROR: tx broadcast rejected (code $BROADCAST_CODE): $(echo "$BROADCAST_RESULT" | jq -r '.raw_log')" >&2
  exit 1
fi

echo "Broadcast tx $TX_HASH, waiting for block inclusion..."

ELAPSED=0
TX_RESULT=""
while [ "$ELAPSED" -lt "$TX_CONFIRM_TIMEOUT_SECONDS" ]; do
  if QUERY_RESULT=$("$GAIA_BINARY" query tx "$TX_HASH" --chain-id "$GAIA_CHAIN_ID" --node "$GAIA_NODE" -o json 2>/dev/null); then
    TX_RESULT="$QUERY_RESULT"
    break
  fi
  sleep "$TX_CONFIRM_POLL_INTERVAL_SECONDS"
  ELAPSED=$((ELAPSED + TX_CONFIRM_POLL_INTERVAL_SECONDS))
done

if [ -z "$TX_RESULT" ]; then
  echo "ERROR: tx $TX_HASH not confirmed in a block within ${TX_CONFIRM_TIMEOUT_SECONDS}s" >&2
  exit 1
fi

TX_CODE=$(echo "$TX_RESULT" | jq -r '.code')
if [ "$TX_CODE" != "0" ]; then
  echo "ERROR: tx $TX_HASH included but failed on-chain (code $TX_CODE): $(echo "$TX_RESULT" | jq -r '.raw_log')" >&2
  exit 1
fi

TX_HEIGHT=$(echo "$TX_RESULT" | jq -r '.height')
echo "Tx $TX_HASH included in block $TX_HEIGHT, ratio $REDEMPTION_RATE submitted successfully."
