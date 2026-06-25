#!/usr/bin/env bash
set -euo pipefail

# Deploys InflowVault (UUPS upgradeable proxy) to Arc testnet.
# Wraps script/DeployInflowVault.s.sol — see that file for full details.
# Run from contracts/inflow/evm/
# Reads config from .env — see .env.example for all variables.

# ── Config ────────────────────────────────────────────────────────────────────

RPC_URL="https://rpc.testnet.arc.network"

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

MY_ADDRESS=$(cast wallet address --private-key "$PRIVATE_KEY")
echo "Deployer: $MY_ADDRESS"

# ── Export env vars expected by DeployInflowVault.s.sol ──────────────────────

export ASSET="${ASSET:-0x3600000000000000000000000000000000000000}"   # USDC on Arc
export VAULT_NAME="${VAULT_NAME:-inflow_usdc_share}"
export VAULT_SYMBOL="${VAULT_SYMBOL:-inflow_usdc_share}"
export DEPOSIT_CAP="${DEPOSIT_CAP:-1000000000000}"                    # 1 000 000 USDC (6 decimals)
export MAX_WITHDRAWALS_PER_USER="${MAX_WITHDRAWALS_PER_USER:-10}"
export INITIAL_ADMIN="${INITIAL_ADMIN:-$MY_ADDRESS}"
export INITIAL_DEPLOYED_AMOUNT_ADMIN="${INITIAL_DEPLOYED_AMOUNT_ADMIN:-$INITIAL_ADMIN}"
export FEE_RATE="${FEE_RATE:-0}"
export FEE_RECIPIENT="${FEE_RECIPIENT:-}"
export RPC_URL

# ── Deploy ────────────────────────────────────────────────────────────────────

echo ""
forge script script/DeployInflowVault.s.sol \
  --rpc-url "$RPC_URL" \
  --broadcast \
  --private-key "$PRIVATE_KEY" \
  -vvvv
