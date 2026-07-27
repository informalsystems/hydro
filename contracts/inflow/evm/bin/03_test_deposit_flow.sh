#!/usr/bin/env bash
set -euo pipefail

# End-to-end deposit/redeem flow test against a live InflowVault + BasicInflowAdapter.
# Uses cast call/send directly so Arc testnet's custom USDC precompiles are handled
# by Arc's real EVM (forge script simulation can't handle them).
# Run from contracts/inflow/evm/
# Reads config from .env — see .env.example for all variables.

# ── Config ────────────────────────────────────────────────────────────────────

RPC_URL="https://rpc.testnet.arc.network"
# Arc's USDC precompiles (isBlocklisted / NATIVE_COIN_AUTHORITY.transfer) require
# more gas than forge's default estimation provides.
GAS_LIMIT=5000000

# ── Load secrets ──────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"
if [[ -f "$ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$ENV_FILE"
fi

if [[ -z "${PRIVATE_KEY:-}" ]]; then
  if [[ -n "${MNEMONIC:-}" ]]; then
    PRIVATE_KEY=$(cast wallet private-key --mnemonic "$MNEMONIC")
  else
    echo "Error: set PRIVATE_KEY or MNEMONIC in .env or env"
    exit 1
  fi
fi

ME=$(cast wallet address --private-key "$PRIVATE_KEY")

ASSET="${ASSET:-0x3600000000000000000000000000000000000000}"
VAULT="${VAULT_ADDRESS:?Error: VAULT_ADDRESS not set in .env}"
ADAPTER="${ADAPTER_ADDRESS:?Error: ADAPTER_ADDRESS not set in .env}"
AMOUNT="${DEPOSIT_AMOUNT:-10000000}"   # 10 USDC (6 decimals)
SKIP_REDEEM="${SKIP_REDEEM:-false}"

echo "Caller:          $ME"
echo "Vault:           $VAULT"
echo "Adapter:         $ADAPTER"
echo "Asset:           $ASSET"
echo "Deposit amount:  $AMOUNT"
echo "Skip redeem:     $SKIP_REDEEM"
echo ""

# ── Helpers ───────────────────────────────────────────────────────────────────

vcall() { cast call "$1" "$2" "${@:3}" --rpc-url "$RPC_URL"; }

snapshot() {
  local label="$1"
  echo "=== $label ==="
  echo "  My asset balance:       $(vcall "$ASSET" "balanceOf(address)(uint256)" "$ME")"
  echo "  Vault asset balance:    $(vcall "$ASSET" "balanceOf(address)(uint256)" "$VAULT")"
  echo "  Adapter asset balance:  $(vcall "$ASSET" "balanceOf(address)(uint256)" "$ADAPTER")"
  echo "  My vault shares:        $(vcall "$VAULT" "balanceOf(address)(uint256)" "$ME")"
  echo "  Vault totalAssets:      $(vcall "$VAULT" "totalAssets()(uint256)")"
}

vsend() {
  local to="$1"; local sig="$2"; shift 2
  local hash
  hash=$(cast send "$to" "$sig" "$@" \
    --private-key "$PRIVATE_KEY" \
    --rpc-url "$RPC_URL" \
    --gas-limit "$GAS_LIMIT" \
    --json | jq -r '.transactionHash')
  echo "  tx: $hash"
}

# ── Snapshot: before ─────────────────────────────────────────────────────────

snapshot "BEFORE DEPOSIT"
echo ""

# ── Step 1: approve ───────────────────────────────────────────────────────────

echo "=== Step 1: approve vault for $AMOUNT ==="
vsend "$ASSET" "approve(address,uint256)" "$VAULT" "$AMOUNT"
echo ""

# ── Step 2: deposit ───────────────────────────────────────────────────────────

echo "=== Step 2: deposit $AMOUNT into vault ==="
vsend "$VAULT" "deposit(uint256,address)" "$AMOUNT" "$ME"
echo ""

# ── Snapshot: after deposit ───────────────────────────────────────────────────

snapshot "AFTER DEPOSIT"
echo "  Adapter position(vault): $(vcall "$ADAPTER" "depositorPosition(address,address)(uint256)" "$VAULT" "$ASSET")"
echo ""

# ── Step 3: redeem ────────────────────────────────────────────────────────────

if [[ "$SKIP_REDEEM" == "true" ]]; then
  echo "Skipping redeem (SKIP_REDEEM=true)."
  exit 0
fi

SHARES=$(vcall "$VAULT" "balanceOf(address)(uint256)" "$ME" | awk '{print $1}')
if [[ "$SHARES" == "0" ]]; then
  echo "No shares to redeem."
  exit 0
fi

echo "=== Step 3: redeem $SHARES shares ==="
vsend "$VAULT" "redeem(uint256,address,address)" "$SHARES" "$ME" "$ME"
echo ""

# ── Snapshot: after redeem ────────────────────────────────────────────────────

snapshot "AFTER REDEEM"
