#!/usr/bin/env bash
set -euo pipefail

# Live smoke test against a deployed InflowVault on Arc mainnet (chain 5042):
# deposit USDC, verify hUSDC shares are minted, redeem the full balance, verify
# USDC is returned. Uses cast call/send directly so Arc's custom USDC precompiles
# are handled by Arc's real EVM (forge script simulation can't handle them).
# Run from contracts/inflow/evm/
# Reads config from .env
#
# Signing — set exactly one of these in .env (in order of preference):
#   ACCOUNT_NAME  cast keystore account name (safest — no secrets in shell history)
#   PRIVATE_KEY   raw hex key (appears in process table — avoid on shared machines)
#   MNEMONIC      24-word phrase (derived to PRIVATE_KEY at runtime — same caveat)

# ── Load config ───────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"
if [[ -f "$ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$ENV_FILE"
fi

RPC_URL="${ARC_MAINNET_RPC_URL:?Error: ARC_MAINNET_RPC_URL not set in .env}"
# Arc's USDC precompiles (isBlocklisted / NATIVE_COIN_AUTHORITY.transfer) require
# more gas than cast's default estimation provides.
GAS_LIMIT=5000000

ASSET="${ASSET:-0x3600000000000000000000000000000000000000}"
VAULT="${VAULT_ADDRESS:?Error: VAULT_ADDRESS not set in .env (set it to the proxy address printed by 06_deploy_inflow_vault_arc_mainnet.sh)}"
AMOUNT="${DEPOSIT_AMOUNT:-1000000}"   # 1 USDC (6 decimals)

# On Arc, ASSET (the USDC precompile) is a 6-decimal view onto the same balance
# that gas is paid from natively (18 decimals) — asset balanceOf == floor(native
# wei balance / 1e12). So a deposit+redeem round trip never returns to the exact
# pre-test asset balance; it's short by whatever gas the tx's cost. We track that
# gas in wei via TOTAL_GAS_WEI so the final check can account for it precisely.
TOTAL_GAS_WEI=0

# ── Resolve signing method ────────────────────────────────────────────────────

SIGN_FLAGS=()
if [[ -n "${ACCOUNT_NAME:-}" ]]; then
  SIGN_FLAGS=(--account "$ACCOUNT_NAME")
  ME=$(cast wallet address --account "$ACCOUNT_NAME" 2>/dev/null \
    || cast wallet address --account "$ACCOUNT_NAME" --show-address)
elif [[ -n "${PRIVATE_KEY:-}" ]]; then
  SIGN_FLAGS=(--private-key "$PRIVATE_KEY")
  ME=$(cast wallet address --private-key "$PRIVATE_KEY")
elif [[ -n "${MNEMONIC:-}" ]]; then
  PRIVATE_KEY=$(cast wallet private-key --mnemonic "$MNEMONIC")
  SIGN_FLAGS=(--private-key "$PRIVATE_KEY")
  ME=$(cast wallet address --private-key "$PRIVATE_KEY")
else
  echo "Error: set ACCOUNT_NAME (recommended), PRIVATE_KEY, or MNEMONIC in .env"
  exit 1
fi

echo "Caller:          $ME"
echo "Vault:           $VAULT"
echo "Asset:           $ASSET"
echo "Deposit amount:  $AMOUNT"
echo ""

# ── Helpers ───────────────────────────────────────────────────────────────────

vcall() { cast call "$1" "$2" "${@:3}" --rpc-url "$RPC_URL"; }

num() { awk '{print $1}' <<<"$1"; }

vsend() {
  local to="$1" sig="$2"; shift 2
  local receipt status hash reason gas_hex price_hex cost
  receipt=$(cast send "$to" "$sig" "$@" \
    "${SIGN_FLAGS[@]}" \
    --rpc-url "$RPC_URL" \
    --gas-limit "$GAS_LIMIT" \
    --json)
  hash=$(jq -r '.transactionHash' <<<"$receipt")
  status=$(jq -r '.status' <<<"$receipt")
  echo "  tx: $hash"
  if [[ "$status" != "0x1" ]]; then
    reason=$(jq -r '.revertReason // "unknown"' <<<"$receipt")
    echo "FAIL: transaction $hash reverted (status $status): $reason"
    exit 1
  fi
  gas_hex=$(jq -r '.gasUsed' <<<"$receipt")
  price_hex=$(jq -r '.effectiveGasPrice' <<<"$receipt")
  cost=$(python3 -c "print(int('$gas_hex',16)*int('$price_hex',16))")
  TOTAL_GAS_WEI=$(python3 -c "print($TOTAL_GAS_WEI + $cost)")
}

snapshot() {
  local label="$1"
  echo "=== $label ==="
  echo "  My asset balance:    $(vcall "$ASSET" "balanceOf(address)(uint256)" "$ME")"
  echo "  Vault asset balance: $(vcall "$ASSET" "balanceOf(address)(uint256)" "$VAULT")"
  echo "  My vault shares:     $(vcall "$VAULT" "balanceOf(address)(uint256)" "$ME")"
  echo "  Vault totalSupply:   $(vcall "$VAULT" "totalSupply()(uint256)")"
  echo "  Vault totalAssets:   $(vcall "$VAULT" "totalAssets()(uint256)")"
}

# ── Snapshot: before ─────────────────────────────────────────────────────────

NATIVE_BEFORE=$(cast balance "$ME" --rpc-url "$RPC_URL")
SHARES_BEFORE=$(num "$(vcall "$VAULT" "balanceOf(address)(uint256)" "$ME")")
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

SHARES_AFTER_DEPOSIT=$(num "$(vcall "$VAULT" "balanceOf(address)(uint256)" "$ME")")
MINTED=$((SHARES_AFTER_DEPOSIT - SHARES_BEFORE))
snapshot "AFTER DEPOSIT"
echo ""

if [[ "$MINTED" -le 0 ]]; then
  echo "FAIL: no hUSDC shares were minted."
  exit 1
fi
echo "PASS: received $MINTED hUSDC shares."
echo ""

# ── Step 3: redeem the full share balance ────────────────────────────────────

echo "=== Step 3: redeem $SHARES_AFTER_DEPOSIT shares (full balance) ==="
vsend "$VAULT" "redeem(uint256,address,address)" "$SHARES_AFTER_DEPOSIT" "$ME" "$ME"
echo ""

snapshot "AFTER REDEEM"
echo ""

NATIVE_AFTER=$(cast balance "$ME" --rpc-url "$RPC_URL")
SHARES_FINAL=$(num "$(vcall "$VAULT" "balanceOf(address)(uint256)" "$ME")")
EXPECTED_NATIVE_AFTER=$(python3 -c "print($NATIVE_BEFORE - $TOTAL_GAS_WEI)")

if [[ "$SHARES_FINAL" != "$SHARES_BEFORE" ]]; then
  echo "FAIL: hUSDC share balance did not return to its pre-test level (still holding shares — redeem may have been queued instead of instant; check for a WithdrawalQueued event on tx above)."
  exit 1
fi

# Gas is paid from this same native balance the ASSET precompile mirrors, so the
# only expected change across the whole round trip is the gas actually spent.
if [[ "$NATIVE_AFTER" != "$EXPECTED_NATIVE_AFTER" ]]; then
  echo "FAIL: balance did not return to pre-test level net of gas (expected $EXPECTED_NATIVE_AFTER wei, got $NATIVE_AFTER wei — gas spent: $TOTAL_GAS_WEI wei). Vault may not have returned the full deposited amount."
  exit 1
fi

echo "PASS: redeemed in full — deposited $(python3 -c "print(f'{$AMOUNT/1e6:.6f}')") USDC and got it all back, net of $TOTAL_GAS_WEI wei ($(python3 -c "print(f'{$TOTAL_GAS_WEI/1e18:.6f}')") USDC-equivalent) in gas across 3 transactions."
