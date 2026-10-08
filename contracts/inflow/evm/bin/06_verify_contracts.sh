#!/usr/bin/env bash
set -euo pipefail

# Submits an already-deployed InflowVault stack for block explorer verification:
# both libraries, the implementation, and the proxy.
#
# The addresses and the proxy constructor calldata are per-deployment facts, so they
# come from the config file rather than living in this script.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

usage() {
  cat <<USAGE
Usage: $(basename "$0") [--env <target>]

  --env <target>  read .env.<target> instead of .env

Required in the config file:
  ETHERSCAN_API_KEY     explorer API key (one key works across chains)
  EXPECTED_CHAIN_ID     chain to verify on
  VERIFY_ADAPTER_LIB    deployed InflowAdapterLib address
  VERIFY_WITHDRAWAL_LIB deployed InflowWithdrawalQueueLib address
  VERIFY_IMPL           deployed InflowVault implementation address
  VERIFY_PROXY          deployed ERC1967Proxy address
  VERIFY_INIT_DATA      initialize() calldata passed to the proxy constructor
USAGE
}

parse_args "$@"
cd "$EVM_ROOT"
load_env

require_vars ETHERSCAN_API_KEY EXPECTED_CHAIN_ID \
  VERIFY_ADAPTER_LIB VERIFY_WITHDRAWAL_LIB VERIFY_IMPL VERIFY_PROXY VERIFY_INIT_DATA

echo ""
echo "1/4  InflowAdapterLib ($VERIFY_ADAPTER_LIB)"
forge verify-contract "$VERIFY_ADAPTER_LIB" \
  contracts/InflowAdapterLib.sol:InflowAdapterLib \
  --chain "$EXPECTED_CHAIN_ID" --etherscan-api-key "$ETHERSCAN_API_KEY"

echo "2/4  InflowWithdrawalQueueLib ($VERIFY_WITHDRAWAL_LIB)"
forge verify-contract "$VERIFY_WITHDRAWAL_LIB" \
  contracts/InflowWithdrawalQueueLib.sol:InflowWithdrawalQueueLib \
  --chain "$EXPECTED_CHAIN_ID" --etherscan-api-key "$ETHERSCAN_API_KEY"

echo "3/4  InflowVault implementation ($VERIFY_IMPL)"
forge verify-contract "$VERIFY_IMPL" \
  contracts/InflowVault.sol:InflowVault \
  --chain "$EXPECTED_CHAIN_ID" --etherscan-api-key "$ETHERSCAN_API_KEY" \
  --libraries contracts/InflowAdapterLib.sol:InflowAdapterLib:"$VERIFY_ADAPTER_LIB" \
  --libraries contracts/InflowWithdrawalQueueLib.sol:InflowWithdrawalQueueLib:"$VERIFY_WITHDRAWAL_LIB"

echo "4/4  ERC1967Proxy ($VERIFY_PROXY)"
forge verify-contract "$VERIFY_PROXY" \
  lib/openzeppelin-contracts/contracts/proxy/ERC1967/ERC1967Proxy.sol:ERC1967Proxy \
  --chain "$EXPECTED_CHAIN_ID" --etherscan-api-key "$ETHERSCAN_API_KEY" \
  --constructor-args "$(cast abi-encode "constructor(address,bytes)" "$VERIFY_IMPL" "$VERIFY_INIT_DATA")"

echo ""
echo "Done. On the explorer, open the proxy and use More Options -> 'Is this a proxy?'"
echo "to link the implementation ABI."
