#!/bin/bash
# Full Neutron → Cosmos Hub Inflow Vault migration scenario.
# Walks through all 8 steps in sequence, pausing between each for operator review.
#
# Usage: ./run_all.sh
# Edit the variables below before running.

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# ============================================================================
# Configuration — edit these before running
# ============================================================================

NEUTRON_CONFIG="deploy-config-neutron.json"
COSMOSHUB_CONFIG="deploy-config-cosmoshub.json"
ADMIN_WALLET="test-deployer"
USER_WALLET="alice"
DEPOSIT_AMOUNT="100000000"   # amount in uatom (100 ATOM = 100000000 uatom)

# ============================================================================
# Colors and helpers
# ============================================================================

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

CURRENT_STEP=0
TOTAL_STEPS=8

next_step() {
    CURRENT_STEP=$((CURRENT_STEP + 1))
    echo ""
    echo -e "${CYAN}${BOLD}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}${BOLD}  Step ${CURRENT_STEP}/${TOTAL_STEPS}: $1${NC}"
    echo -e "${CYAN}${BOLD}╚══════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    read -r -p "  ▶  Press Enter to run this step, or Ctrl+C to abort... "
    echo ""
}

step_done() {
    echo ""
    echo -e "${GREEN}  ✓ Step ${CURRENT_STEP} complete${NC}"
}

# ============================================================================
# Validate configs exist
# ============================================================================

for f in "$NEUTRON_CONFIG" "$COSMOSHUB_CONFIG"; do
    if [ ! -f "$f" ]; then
        echo -e "${RED}Error: config file '$f' not found${NC}"
        exit 1
    fi
done

# ============================================================================
# Welcome banner
# ============================================================================

N_CHAIN=$(jq -r '.chain_id' "$NEUTRON_CONFIG")
H_CHAIN=$(jq -r '.chain_id' "$COSMOSHUB_CONFIG")

echo ""
echo -e "${BOLD}╔══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}       Neutron → Cosmos Hub Inflow Vault Migration             ${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo "  Neutron config:    $NEUTRON_CONFIG  ($N_CHAIN)"
echo "  Cosmos Hub config: $COSMOSHUB_CONFIG  ($H_CHAIN)"
echo "  Admin wallet:      $ADMIN_WALLET"
echo "  User wallet:       $USER_WALLET"
echo "  Deposit amount:    $DEPOSIT_AMOUNT uatom"
echo ""
echo "  This script will walk you through all 8 steps of the migration."
echo "  You will be prompted before each step."
echo ""
read -r -p "  Ready to begin? Press Enter to start, or Ctrl+C to abort... "
echo ""

# ============================================================================
# Step 1: Deploy on Neutron
# ============================================================================

next_step "Deploy Control Center + Vault on Neutron"
bash "$SCRIPT_DIR/01_deploy_neutron.sh" "$NEUTRON_CONFIG"
step_done

# ============================================================================
# Step 2: Deploy on Cosmos Hub + Shares Converter
# ============================================================================

next_step "Deploy Control Center + Vault + Shares Converter on Cosmos Hub"
bash "$SCRIPT_DIR/02_deploy_cosmoshub.sh" "$NEUTRON_CONFIG" "$COSMOSHUB_CONFIG"
step_done

# ============================================================================
# Step 3: User deposits ATOM on Neutron and IBC-transfers shares to Hub
# ============================================================================

next_step "User: Deposit ${DEPOSIT_AMOUNT} uatom on Neutron and IBC-transfer shares to Hub"
bash "$SCRIPT_DIR/03_user_deposit_and_ibc.sh" \
    "$NEUTRON_CONFIG" "$COSMOSHUB_CONFIG" "$USER_WALLET" "$DEPOSIT_AMOUNT"
step_done

# ============================================================================
# Step 4: Admin withdraws vault ATOM for deployment and IBC-sends to Hub
# ============================================================================

next_step "Admin: Withdraw all ATOM from Neutron vault and IBC-send to Cosmos Hub"
bash "$SCRIPT_DIR/04_admin_withdraw_for_deployment.sh" \
    "$NEUTRON_CONFIG" "$COSMOSHUB_CONFIG" "$ADMIN_WALLET"
step_done

# ============================================================================
# Step 5: Admin pauses Neutron vault and calls MintForMigration on Hub
# ============================================================================

next_step "Admin: Pause Neutron vault and call MintForMigration on Hub vault"
bash "$SCRIPT_DIR/05_admin_pause_and_mint.sh" \
    "$NEUTRON_CONFIG" "$COSMOSHUB_CONFIG" "$ADMIN_WALLET"
step_done

# ============================================================================
# Step 6: Admin deposits IBC'd ATOM into Hub vault
# ============================================================================

next_step "Admin: Deposit IBC'd ATOM into Cosmos Hub vault (DepositFromDeployment)"
bash "$SCRIPT_DIR/06_admin_deposit_from_deployment.sh" \
    "$COSMOSHUB_CONFIG" "$ADMIN_WALLET"
step_done

# ============================================================================
# Step 7: User converts Neutron IBC shares to Hub vault shares
# ============================================================================

next_step "User: Convert Neutron IBC shares → Cosmos Hub vault shares"
bash "$SCRIPT_DIR/07_user_convert_shares.sh" \
    "$COSMOSHUB_CONFIG" "$USER_WALLET"
step_done

# ============================================================================
# Step 8: User withdraws ATOM from Hub vault
# ============================================================================

next_step "User: Withdraw ATOM from Cosmos Hub vault"
bash "$SCRIPT_DIR/08_user_withdraw_hub.sh" \
    "$COSMOSHUB_CONFIG" "$USER_WALLET"
step_done

# ============================================================================
# Final summary
# ============================================================================

echo ""
echo -e "${BOLD}${GREEN}╔══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}${GREEN}         Migration scenario complete!                          ${NC}"
echo -e "${BOLD}${GREEN}╚══════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo "  All 8 steps have been executed successfully."
echo ""
echo "  State file:  $SCRIPT_DIR/migration-state.json"
echo "  Hub config:  $COSMOSHUB_CONFIG"
echo ""
