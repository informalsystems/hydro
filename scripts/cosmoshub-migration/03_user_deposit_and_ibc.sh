#!/bin/bash
# Step 3: User deposits ATOM into the Inflow Vault on Neutron and IBC-transfers
# the received vault shares to their address on Cosmos Hub.
#
# Usage: ./03_user_deposit_and_ibc.sh <neutron-config> <cosmoshub-config> <from-wallet> <amount-uatom>
# Example: ./03_user_deposit_and_ibc.sh deploy-config-neutron.json deploy-config-cosmoshub.json alice 1000000000

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

exec bash "$SCRIPT_DIR/deposit-neutron-then-ibc.sh" "$@"
