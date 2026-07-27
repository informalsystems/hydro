#!/usr/bin/env bash
set -euo pipefail

# Verifies the already-deployed InflowVault contracts on Base mainnet (chain 8453).
# Run from contracts/inflow/evm/
# Requires ETHERSCAN_API_KEY in .env or exported in env.

# ── Load secrets ──────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"
if [[ -f "$ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$ENV_FILE"
fi

if [[ -z "${ETHERSCAN_API_KEY:-}" ]]; then
  echo "Error: ETHERSCAN_API_KEY not set. Get a free key at https://basescan.org/myapikey"
  exit 1
fi

# ── Deployed addresses ────────────────────────────────────────────────────────

LIB_ADAPTER="0x7f204d3471212ed7ae32e5d0a8ae9b8281f77db3"
LIB_WITHDRAWAL="0x2932bdc9ecee0ff06b6a78e9d1aca2afc372e41a"
IMPL="0x421300a190373877cef8110c84e3b0a9a6848e5b"
PROXY="0x28003bdc20de02643d81db80c9f6e2b88ff59e05"

echo "Verifying InflowVault contracts on Base (chain 8453)..."
echo ""

# ── 1. InflowAdapterLib ───────────────────────────────────────────────────────

echo "1/4  InflowAdapterLib ($LIB_ADAPTER)"
forge verify-contract \
  "$LIB_ADAPTER" \
  contracts/InflowAdapterLib.sol:InflowAdapterLib \
  --chain 8453 \
  --etherscan-api-key "$ETHERSCAN_API_KEY"

# ── 2. InflowWithdrawalQueueLib ───────────────────────────────────────────────

echo "2/4  InflowWithdrawalQueueLib ($LIB_WITHDRAWAL)"
forge verify-contract \
  "$LIB_WITHDRAWAL" \
  contracts/InflowWithdrawalQueueLib.sol:InflowWithdrawalQueueLib \
  --chain 8453 \
  --etherscan-api-key "$ETHERSCAN_API_KEY"

# ── 3. InflowVault implementation ─────────────────────────────────────────────

echo "3/4  InflowVault implementation ($IMPL)"
forge verify-contract \
  "$IMPL" \
  contracts/InflowVault.sol:InflowVault \
  --chain 8453 \
  --etherscan-api-key "$ETHERSCAN_API_KEY" \
  --libraries contracts/InflowAdapterLib.sol:InflowAdapterLib:"$LIB_ADAPTER" \
  --libraries contracts/InflowWithdrawalQueueLib.sol:InflowWithdrawalQueueLib:"$LIB_WITHDRAWAL"

# ── 4. ERC1967Proxy ───────────────────────────────────────────────────────────

echo "4/4  ERC1967Proxy ($PROXY)"
INIT_DATA="0xe38ef24d000000000000000000000000833589fcd6edb6e08f4c7c32d4f71b54bda0291300000000000000000000000000000000000000000000000000000000000001200000000000000000000000000000000000000000000000000000000000000160000000000000000000000000000000000000000000000000000000e8d4a51000000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000001a000000000000000000000000000000000000000000000000000000000000001e0000000000000000000000000000000000000000000000000016345785d8a0000000000000000000000000000fa82c937fc0f6fd3bc6c66f612cf5b539d489d210000000000000000000000000000000000000000000000000000000000000011696e666c6f775f757364635f73686172650000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000011696e666c6f775f757364635f73686172650000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001000000000000000000000000a9e3591eb3f0dd0b97803b3a04ea75db4f1fa48b0000000000000000000000000000000000000000000000000000000000000001000000000000000000000000fdd45915dbac17d527ebc024f444aede30157d0e"

forge verify-contract \
  "$PROXY" \
  lib/openzeppelin-contracts/contracts/proxy/ERC1967/ERC1967Proxy.sol:ERC1967Proxy \
  --chain 8453 \
  --etherscan-api-key "$ETHERSCAN_API_KEY" \
  --constructor-args "$(cast abi-encode "constructor(address,bytes)" "$IMPL" "$INIT_DATA")"

echo ""
echo "Done. Now go to https://basescan.org/address/$PROXY"
echo "and click More Options → 'Is this a proxy?' to link the implementation ABI."
