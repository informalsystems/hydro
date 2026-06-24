#!/bin/bash
# Step 1: Deploy Control Center + Vault on Neutron.
# Usage: ./01_deploy_neutron.sh [neutron-config]
# Default config: deploy-config-neutron.json

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

NEUTRON_CONFIG="${1:-deploy-config-neutron.json}"

if [ ! -f "$NEUTRON_CONFIG" ]; then
    echo "Error: config file '$NEUTRON_CONFIG' not found"
    exit 1
fi

bash "$SCRIPT_DIR/deploy-inflow-vault.sh" "$NEUTRON_CONFIG"
