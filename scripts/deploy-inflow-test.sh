#!/bin/bash

# Deployment script for Inflow Test Environment
# This script automates the deployment of Control Center, Vault, Mars Adapter, and IBC Adapter contracts

set -e

# Change to script directory to ensure all relative paths work correctly
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Default configuration
DEFAULT_CONFIG_FILE="deploy-config.json"

# Parse command-line arguments
CLI_DEPLOYER_WALLET=""
CLI_ADMIN_ADDRESS=""
CLI_KEYRING_BACKEND="test"  # Default value
CONFIG_FILE="$DEFAULT_CONFIG_FILE"

# Check if first argument is a config file (doesn't start with --)
if [[ $# -gt 0 && "$1" != --* ]]; then
    CONFIG_FILE="$1"
    shift
fi

# Parse remaining arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --deployer-wallet)
            CLI_DEPLOYER_WALLET="$2"
            shift 2
            ;;
        --admin-address)
            CLI_ADMIN_ADDRESS="$2"
            shift 2
            ;;
        --keyring-backend)
            CLI_KEYRING_BACKEND="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [config-file.json] [--deployer-wallet <wallet>] [--admin-address <address>] [--keyring-backend <backend>]"
            exit 1
            ;;
    esac
done

# Paths to WASM files
CONTROL_CENTER_WASM="../artifacts/control_center.wasm"
VAULT_WASM="../artifacts/vault.wasm"
MARS_ADAPTER_WASM="../artifacts/mars_adapter.wasm"
IBC_ADAPTER_WASM="../artifacts/ibc_adapter.wasm"

echo -e "${GREEN}=== Inflow Test Deployment Script ===${NC}"
echo ""

# Create default config if it doesn't exist
if [ ! -f "$CONFIG_FILE" ]; then
    # Validate required CLI parameters for new config
    if [ -z "$CLI_DEPLOYER_WALLET" ]; then
        echo -e "${RED}Error: Config file not found and --deployer-wallet not provided${NC}"
        echo "Usage: $0 [config-file.json] --deployer-wallet <wallet> [--admin-address <address>] [--keyring-backend <backend>]"
        exit 1
    fi

    # Need neutrond binary to derive address - use default if not set
    NEUTRON_BINARY="neutrond"

    # Derive deployer address from wallet
    DEPLOYER_ADDRESS=$($NEUTRON_BINARY keys show $CLI_DEPLOYER_WALLET --keyring-backend $CLI_KEYRING_BACKEND --output json 2>/dev/null | jq -r .address)

    if [ -z "$DEPLOYER_ADDRESS" ]; then
        echo -e "${RED}Error: Could not find wallet '$CLI_DEPLOYER_WALLET' in keyring${NC}"
        exit 1
    fi

    # Set admin address to deployer address if not specified
    if [ -z "$CLI_ADMIN_ADDRESS" ]; then
        CLI_ADMIN_ADDRESS="$DEPLOYER_ADDRESS"
    fi

    echo -e "${YELLOW}Config file not found. Creating config at $CONFIG_FILE${NC}"
    cat > "$CONFIG_FILE" <<EOF
{
  "chain_id": "neutron-1",
  "neutron_rpc_node": "https://rpc-lb.neutron.org/",
  "gas_price": "0.0053untrn",
  "gas_adjustment": "1.3",
  "deployer_wallet": "$CLI_DEPLOYER_WALLET",
  "admin_address": "$CLI_ADMIN_ADDRESS",
  "keyring_backend": "$CLI_KEYRING_BACKEND",
  "neutron_binary": "neutrond",
  "neutron_dir": "$HOME/.neutrond",
  "deposit_cap": "10000000",
  "deposit_denom": "ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81",
  "max_withdrawals": 10,
  "vault_subdenom": "it_uusdc",
  "mars_contract": "neutron1qdzn3l4kn7gsjna2tfpg3g3mwd6kunx4p50lfya59k02846xas6qslgs3r",
  "token_metadata": {
    "description": "A share of Inflow Test USDC vault",
    "exponent": 6,
    "display": "itusdc",
    "name": "Inflow Test USDC share",
    "symbol": "ITUSDC",
    "uri": null,
    "uri_hash": null
  },
  "code_ids": {
    "control_center": null,
    "vault": null,
    "mars_adapter": null,
    "ibc_adapter": null
  },
  "contracts": {
    "control_center": null,
    "vault": null,
    "mars_adapter": null,
    "ibc_adapter": null
  }
}
EOF
    echo -e "${GREEN}Config created with your parameters.${NC}"
    echo ""
fi

# Load configuration
NEUTRON_CHAIN_ID=$(jq -r '.chain_id' $CONFIG_FILE)
NEUTRON_NODE=$(jq -r '.neutron_rpc_node' $CONFIG_FILE)
GAS_PRICE=$(jq -r '.gas_price' $CONFIG_FILE)
GAS_ADJUSTMENT=$(jq -r '.gas_adjustment' $CONFIG_FILE)
DEPLOYER_WALLET=$(jq -r '.deployer_wallet' $CONFIG_FILE)
ADMIN_ADDRESS=$(jq -r '.admin_address' $CONFIG_FILE)
KEYRING_BACKEND=$(jq -r '.keyring_backend' $CONFIG_FILE)
NEUTRON_BINARY=$(jq -r '.neutron_binary' $CONFIG_FILE)
NEUTRON_DIR=$(jq -r '.neutron_dir' $CONFIG_FILE)

DEPOSIT_CAP=$(jq -r '.deposit_cap' $CONFIG_FILE)
DEPOSIT_DENOM=$(jq -r '.deposit_denom' $CONFIG_FILE)
MAX_WITHDRAWALS=$(jq -r '.max_withdrawals' $CONFIG_FILE)
VAULT_SUBDENOM=$(jq -r '.vault_subdenom' $CONFIG_FILE)
MARS_CONTRACT=$(jq -r '.mars_contract' $CONFIG_FILE)

# Load token metadata
TOKEN_METADATA=$(jq -c '.token_metadata' $CONFIG_FILE)

# Load existing code IDs
CONTROL_CENTER_CODE_ID=$(jq -r '.code_ids.control_center // empty' $CONFIG_FILE)
VAULT_CODE_ID=$(jq -r '.code_ids.vault // empty' $CONFIG_FILE)
MARS_ADAPTER_CODE_ID=$(jq -r '.code_ids.mars_adapter // empty' $CONFIG_FILE)
IBC_ADAPTER_CODE_ID=$(jq -r '.code_ids.ibc_adapter // empty' $CONFIG_FILE)

# Setup CLI flags
NEUTRON_CHAIN_ID_FLAG="--chain-id $NEUTRON_CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend $KEYRING_BACKEND"
TX_FLAG="--gas auto --gas-adjustment $GAS_ADJUSTMENT"
NEUTRON_NODE_FLAG="--node $NEUTRON_NODE"
NEUTRON_TX_FLAGS="$TX_FLAG --gas-prices $GAS_PRICE --chain-id $NEUTRON_CHAIN_ID $NEUTRON_NODE_FLAG $KEYRING_TEST_FLAG -y"
NEUTRON_CLI="$NEUTRON_BINARY --home $NEUTRON_DIR"

# Derive deployer address from wallet
DEPLOYER_ADDRESS=$($NEUTRON_CLI keys show $DEPLOYER_WALLET $KEYRING_TEST_FLAG --output json | jq -r .address)

echo "Configuration:"
echo "  Config File: $CONFIG_FILE"
echo "  Deployer Wallet: $DEPLOYER_WALLET"
echo "  Deployer Address: $DEPLOYER_ADDRESS"
echo "  Admin Address: $ADMIN_ADDRESS"
echo "  Node: $NEUTRON_NODE"
echo "  Chain ID: $NEUTRON_CHAIN_ID"
echo "  Gas Price: $GAS_PRICE"
echo ""

# Retry command function
retry_command() {
    set +e
    local output
    local status
    local max_attempts=${2:-0} # Optional second parameter for max attempts (0 = infinite)
    local attempt=1

    while true; do
        output=$(eval "$1" 2>&1)
        status=$?

        if [ $status -eq 0 ]; then
            echo "" >&2
            echo "$output"
            set -e
            return 0
        fi

        if [ $max_attempts -gt 0 ] && [ $attempt -ge $max_attempts ]; then
            echo "Error: Maximum retry attempts ($max_attempts) reached" >&2
            echo "$output" >&2
            set -e
            return $status
        fi

        printf "." >&2
        sleep 1
        ((attempt++))
    done
}

# Function to update config file
update_config() {
    local key=$1
    local value=$2
    local tmp_file=$(mktemp)
    jq "$key = \"$value\"" "$CONFIG_FILE" > "$tmp_file" && mv "$tmp_file" "$CONFIG_FILE"
}

# Function to store (upload) a contract
store_contract() {
    local contract_name=$1
    local wasm_path=$2
    local code_id_var=$3

    printf "Storing $contract_name wasm"

    $NEUTRON_CLI tx wasm store "$wasm_path" --from "$DEPLOYER_WALLET" $NEUTRON_TX_FLAGS --output json &> "./store_${contract_name}_res.json"
    TX_HASH=$(grep -o '{.*}' "./store_${contract_name}_res.json" | jq -r '.txhash')
    TX_RESULT=$(retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60)
    CODE_ID=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "store_code") | .attributes[] | select(.key == "code_id") | .value')

    echo "$contract_name contract stored with code ID: $CODE_ID"

    # Update config file
    update_config ".code_ids.$code_id_var" "$CODE_ID"
}

# Ask user about redeploying contracts
echo -e "${BLUE}=== Contract Deployment Options ===${NC}"
echo ""

# Control Center
if [ -n "$CONTROL_CENTER_CODE_ID" ] && [ "$CONTROL_CENTER_CODE_ID" != "null" ]; then
    echo -e "${YELLOW}Existing Control Center Code ID: $CONTROL_CENTER_CODE_ID${NC}"
    read -p "Do you want to redeploy Control Center? (y/N): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        store_contract "control_center" "$CONTROL_CENTER_WASM" "control_center"
    fi
else
    echo "No existing Control Center code ID found. Deploying..."
    store_contract "control_center" "$CONTROL_CENTER_WASM" "control_center"
fi
echo ""

# Vault
if [ -n "$VAULT_CODE_ID" ] && [ "$VAULT_CODE_ID" != "null" ]; then
    echo -e "${YELLOW}Existing Vault Code ID: $VAULT_CODE_ID${NC}"
    read -p "Do you want to redeploy Vault? (y/N): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        store_contract "vault" "$VAULT_WASM" "vault"
    fi
else
    echo "No existing Vault code ID found. Deploying..."
    store_contract "vault" "$VAULT_WASM" "vault"
fi
echo ""

# Mars Adapter
if [ -n "$MARS_ADAPTER_CODE_ID" ] && [ "$MARS_ADAPTER_CODE_ID" != "null" ]; then
    echo -e "${YELLOW}Existing Mars Adapter Code ID: $MARS_ADAPTER_CODE_ID${NC}"
    read -p "Do you want to redeploy Mars Adapter? (y/N): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        store_contract "mars_adapter" "$MARS_ADAPTER_WASM" "mars_adapter"
    fi
else
    echo "No existing Mars Adapter code ID found. Deploying..."
    store_contract "mars_adapter" "$MARS_ADAPTER_WASM" "mars_adapter"
fi
echo ""

# IBC Adapter
if [ -n "$IBC_ADAPTER_CODE_ID" ] && [ "$IBC_ADAPTER_CODE_ID" != "null" ]; then
    echo -e "${YELLOW}Existing IBC Adapter Code ID: $IBC_ADAPTER_CODE_ID${NC}"
    read -p "Do you want to redeploy IBC Adapter? (y/N): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        store_contract "ibc_adapter" "$IBC_ADAPTER_WASM" "ibc_adapter"
    fi
else
    echo "No existing IBC Adapter code ID found. Deploying..."
    store_contract "ibc_adapter" "$IBC_ADAPTER_WASM" "ibc_adapter"
fi
echo ""

# Reload all code IDs from config
CONTROL_CENTER_CODE_ID=$(jq -r '.code_ids.control_center' $CONFIG_FILE)
VAULT_CODE_ID=$(jq -r '.code_ids.vault' $CONFIG_FILE)
MARS_ADAPTER_CODE_ID=$(jq -r '.code_ids.mars_adapter' $CONFIG_FILE)
IBC_ADAPTER_CODE_ID=$(jq -r '.code_ids.ibc_adapter' $CONFIG_FILE)

echo -e "${GREEN}=== Contract Instantiation ===${NC}"
echo ""

# Step 1: Instantiate Control Center
instantiate_control_center() {
    printf "Instantiating Control Center contract"

    INIT_CONTROL_CENTER=$(cat <<EOF
{
  "subvaults": [],
  "whitelist": ["$ADMIN_ADDRESS"],
  "deposit_cap": "$DEPOSIT_CAP"
}
EOF
)

    $NEUTRON_CLI tx wasm instantiate "$CONTROL_CENTER_CODE_ID" "$INIT_CONTROL_CENTER" \
        --admin "$ADMIN_ADDRESS" \
        --label "Test Inflow Control Center" \
        --from "$DEPLOYER_WALLET" \
        $NEUTRON_TX_FLAGS \
        --output json &> ./instantiate_control_center_res.json

    TX_HASH=$(grep -o '{.*}' ./instantiate_control_center_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60)
    export CONTROL_CENTER_ADDRESS=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')

    echo "Control Center instantiated at: $CONTROL_CENTER_ADDRESS"
    update_config ".contracts.control_center" "$CONTROL_CENTER_ADDRESS"
}

echo -e "${YELLOW}Step 1: Instantiating Control Center...${NC}"
EXISTING_CONTROL_CENTER=$(jq -r '.contracts.control_center // empty' $CONFIG_FILE)
if [ -n "$EXISTING_CONTROL_CENTER" ] && [ "$EXISTING_CONTROL_CENTER" != "null" ]; then
    echo -e "${YELLOW}Existing Control Center address: $EXISTING_CONTROL_CENTER${NC}"
    read -p "Do you want to reinstantiate Control Center? (y/N): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        instantiate_control_center
    else
        CONTROL_CENTER_ADDRESS="$EXISTING_CONTROL_CENTER"
        echo "Using existing Control Center at: $CONTROL_CENTER_ADDRESS"
    fi
else
    instantiate_control_center
fi
echo ""

# Step 2: Instantiate Vault
instantiate_vault() {
    printf "Instantiating Vault contract"

    INIT_VAULT=$(jq -n \
      --arg whitelist "$ADMIN_ADDRESS" \
      --arg control_center "$CONTROL_CENTER_ADDRESS" \
      --arg deposit_denom "$DEPOSIT_DENOM" \
      --argjson max_withdrawals "$MAX_WITHDRAWALS" \
      --arg subdenom "$VAULT_SUBDENOM" \
      --argjson token_metadata "$TOKEN_METADATA" \
      '{
        whitelist: [$whitelist],
        control_center_contract: $control_center,
        deposit_denom: $deposit_denom,
        max_withdrawals_per_user: $max_withdrawals,
        subdenom: $subdenom,
        token_metadata: $token_metadata
      }')

    $NEUTRON_CLI tx wasm instantiate "$VAULT_CODE_ID" "$INIT_VAULT" \
        --admin "$ADMIN_ADDRESS" \
        --label "Test Inflow USDC Vault" \
        --from "$DEPLOYER_WALLET" \
        $NEUTRON_TX_FLAGS \
        --output json &> ./instantiate_vault_res.json

    TX_HASH=$(grep -o '{.*}' ./instantiate_vault_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60)
    export VAULT_ADDRESS=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')

    echo "Vault instantiated at: $VAULT_ADDRESS"
    update_config ".contracts.vault" "$VAULT_ADDRESS"
}

echo -e "${YELLOW}Step 2: Instantiating Vault...${NC}"
EXISTING_VAULT=$(jq -r '.contracts.vault // empty' $CONFIG_FILE)
if [ -n "$EXISTING_VAULT" ] && [ "$EXISTING_VAULT" != "null" ]; then
    echo -e "${YELLOW}Existing Vault address: $EXISTING_VAULT${NC}"
    read -p "Do you want to reinstantiate Vault? (y/N): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        instantiate_vault
    else
        VAULT_ADDRESS="$EXISTING_VAULT"
        echo "Using existing Vault at: $VAULT_ADDRESS"
    fi
else
    instantiate_vault
fi
echo ""

# Step 3: Instantiate Mars Adapter
instantiate_mars_adapter() {
    printf "Instantiating Mars Adapter contract"

    INIT_MARS_ADAPTER=$(cat <<EOF
{
  "admins": ["$ADMIN_ADDRESS"],
  "supported_denoms": ["$DEPOSIT_DENOM"],
  "mars_contract": "$MARS_CONTRACT",
  "depositor_address": "$VAULT_ADDRESS"
}
EOF
)

    $NEUTRON_CLI tx wasm instantiate "$MARS_ADAPTER_CODE_ID" "$INIT_MARS_ADAPTER" \
        --admin "$ADMIN_ADDRESS" \
        --label "Test Inflow USDC Mars Adapter" \
        --from "$DEPLOYER_WALLET" \
        $NEUTRON_TX_FLAGS \
        --output json &> ./instantiate_mars_adapter_res.json

    TX_HASH=$(grep -o '{.*}' ./instantiate_mars_adapter_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60)
    export MARS_ADAPTER_ADDRESS=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')

    echo "Mars Adapter instantiated at: $MARS_ADAPTER_ADDRESS"
    update_config ".contracts.mars_adapter" "$MARS_ADAPTER_ADDRESS"
}

echo -e "${YELLOW}Step 3: Instantiating Mars Adapter...${NC}"
EXISTING_MARS_ADAPTER=$(jq -r '.contracts.mars_adapter // empty' $CONFIG_FILE)
if [ -n "$EXISTING_MARS_ADAPTER" ] && [ "$EXISTING_MARS_ADAPTER" != "null" ]; then
    echo -e "${YELLOW}Existing Mars Adapter address: $EXISTING_MARS_ADAPTER${NC}"
    read -p "Do you want to reinstantiate Mars Adapter? (y/N): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        instantiate_mars_adapter
    else
        MARS_ADAPTER_ADDRESS="$EXISTING_MARS_ADAPTER"
        echo "Using existing Mars Adapter at: $MARS_ADAPTER_ADDRESS"
    fi
else
    instantiate_mars_adapter
fi
echo ""

# Step 4: Instantiate IBC Adapter
instantiate_ibc_adapter() {
    printf "Instantiating IBC Adapter contract"

    INIT_IBC_ADAPTER=$(cat <<EOF
{
  "admins": ["$ADMIN_ADDRESS"],
  "default_timeout_seconds": 600,
  "depositor_address": "$VAULT_ADDRESS"
}
EOF
)

    $NEUTRON_CLI tx wasm instantiate "$IBC_ADAPTER_CODE_ID" "$INIT_IBC_ADAPTER" \
        --admin "$ADMIN_ADDRESS" \
        --label "Test Inflow USDC IBC Adapter" \
        --from "$DEPLOYER_WALLET" \
        $NEUTRON_TX_FLAGS \
        --output json &> ./instantiate_ibc_adapter_res.json

    TX_HASH=$(grep -o '{.*}' ./instantiate_ibc_adapter_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60)
    export IBC_ADAPTER_ADDRESS=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')

    echo "IBC Adapter instantiated at: $IBC_ADAPTER_ADDRESS"
    update_config ".contracts.ibc_adapter" "$IBC_ADAPTER_ADDRESS"
}

echo -e "${YELLOW}Step 4: Instantiating IBC Adapter...${NC}"
EXISTING_IBC_ADAPTER=$(jq -r '.contracts.ibc_adapter // empty' $CONFIG_FILE)
if [ -n "$EXISTING_IBC_ADAPTER" ] && [ "$EXISTING_IBC_ADAPTER" != "null" ]; then
    echo -e "${YELLOW}Existing IBC Adapter address: $EXISTING_IBC_ADAPTER${NC}"
    read -p "Do you want to reinstantiate IBC Adapter? (y/N): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        instantiate_ibc_adapter
    else
        IBC_ADAPTER_ADDRESS="$EXISTING_IBC_ADAPTER"
        echo "Using existing IBC Adapter at: $IBC_ADAPTER_ADDRESS"
    fi
else
    instantiate_ibc_adapter
fi
echo ""

# Step 4a: Register Noble chain on IBC Adapter
register_noble_chain() {
    printf "Registering Noble chain on IBC Adapter"

    REGISTER_CHAIN_MSG=$(cat <<EOF
{
  "custom_action": {
    "register_chain": {
      "chain_id": "noble-1",
      "channel_from_neutron": "channel-30",
      "allowed_recipients": ["noble1k64ssp5pnkmwtndfzvgtnjmhx06w8mdvlzgrux"]
    }
  }
}
EOF
)

    $NEUTRON_CLI tx wasm execute "$IBC_ADAPTER_ADDRESS" "$REGISTER_CHAIN_MSG" \
        --from "$DEPLOYER_WALLET" \
        $NEUTRON_TX_FLAGS \
        --output json &> ./register_noble_chain_res.json

    TX_HASH=$(grep -o '{.*}' ./register_noble_chain_res.json | jq -r '.txhash')
    retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60 > /dev/null

    echo "Noble chain registered on IBC Adapter"
}

echo -e "${YELLOW}Step 4a: Registering Noble chain on IBC Adapter...${NC}"
register_noble_chain
echo ""

# Step 4b: Register USDC token on IBC Adapter
register_usdc_token() {
    printf "Registering USDC token on IBC Adapter"

    REGISTER_TOKEN_MSG=$(cat <<EOF
{
  "custom_action": {
    "register_token": {
      "denom": "ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81",
      "source_chain_id": "noble-1"
    }
  }
}
EOF
)

    $NEUTRON_CLI tx wasm execute "$IBC_ADAPTER_ADDRESS" "$REGISTER_TOKEN_MSG" \
        --from "$DEPLOYER_WALLET" \
        $NEUTRON_TX_FLAGS \
        --output json &> ./register_usdc_token_res.json

    TX_HASH=$(grep -o '{.*}' ./register_usdc_token_res.json | jq -r '.txhash')
    retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60 > /dev/null

    echo "USDC token registered on IBC Adapter"
}

echo -e "${YELLOW}Step 4b: Registering USDC token on IBC Adapter...${NC}"
register_usdc_token
echo ""

# Step 5: Register Mars Adapter on Vault
register_mars_adapter() {
    printf "Registering Mars Adapter on Vault"

    REGISTER_MARS_ADAPTER_MSG=$(cat <<EOF
{
  "register_adapter": {
    "name": "Mars Adapter",
    "address": "$MARS_ADAPTER_ADDRESS",
    "allocation_mode": "automated",
    "deployment_tracking": "not_tracked"
  }
}
EOF
)

    $NEUTRON_CLI tx wasm execute "$VAULT_ADDRESS" "$REGISTER_MARS_ADAPTER_MSG" \
        --from "$DEPLOYER_WALLET" \
        $NEUTRON_TX_FLAGS \
        --output json &> ./register_mars_adapter_res.json

    TX_HASH=$(grep -o '{.*}' ./register_mars_adapter_res.json | jq -r '.txhash')
    retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60 > /dev/null

    echo "Mars Adapter registered on Vault"
}

echo -e "${YELLOW}Step 5: Registering Mars Adapter on Vault...${NC}"
register_mars_adapter
echo ""

# Step 6: Register IBC Adapter on Vault
register_ibc_adapter() {
    printf "Registering IBC Adapter on Vault"

    REGISTER_IBC_ADAPTER_MSG=$(cat <<EOF
{
  "register_adapter": {
    "name": "IBC Adapter",
    "address": "$IBC_ADAPTER_ADDRESS",
    "allocation_mode": "manual",
    "deployment_tracking": "tracked"
  }
}
EOF
)

    $NEUTRON_CLI tx wasm execute "$VAULT_ADDRESS" "$REGISTER_IBC_ADAPTER_MSG" \
        --from "$DEPLOYER_WALLET" \
        $NEUTRON_TX_FLAGS \
        --output json &> ./register_ibc_adapter_res.json

    TX_HASH=$(grep -o '{.*}' ./register_ibc_adapter_res.json | jq -r '.txhash')
    retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60 > /dev/null

    echo "IBC Adapter registered on Vault"
}

echo -e "${YELLOW}Step 6: Registering IBC Adapter on Vault...${NC}"
register_ibc_adapter
echo ""

# Step 7: Register Subvault in Control Center
register_subvault() {
    printf "Registering Subvault in Control Center"

    ADD_SUBVAULT_MSG=$(cat <<EOF
{
  "add_subvault": {
    "address": "$VAULT_ADDRESS"
  }
}
EOF
)

    $NEUTRON_CLI tx wasm execute "$CONTROL_CENTER_ADDRESS" "$ADD_SUBVAULT_MSG" \
        --from "$DEPLOYER_WALLET" \
        $NEUTRON_TX_FLAGS \
        --output json &> ./add_subvault_res.json

    TX_HASH=$(grep -o '{.*}' ./add_subvault_res.json | jq -r '.txhash')
    retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60 > /dev/null

    echo "Subvault registered in Control Center"
}

echo -e "${YELLOW}Step 7: Registering Subvault in Control Center...${NC}"
register_subvault
echo ""

# Display summary
echo -e "${GREEN}=== Deployment Complete ===${NC}"
echo ""
echo "Code IDs:"
echo "  Control Center: $CONTROL_CENTER_CODE_ID"
echo "  Vault: $VAULT_CODE_ID"
echo "  Mars Adapter: $MARS_ADAPTER_CODE_ID"
echo "  IBC Adapter: $IBC_ADAPTER_CODE_ID"
echo ""
echo "Contract Addresses:"
echo "  Control Center: $CONTROL_CENTER_ADDRESS"
echo "  Vault: $VAULT_ADDRESS"
echo "  Mars Adapter: $MARS_ADAPTER_ADDRESS"
echo "  IBC Adapter: $IBC_ADAPTER_ADDRESS"
echo ""
echo "Configuration saved to: $CONFIG_FILE"
echo ""
echo "To use these contracts in your tests, export the addresses:"
echo "  export CONTROL_CENTER_ADDRESS=$CONTROL_CENTER_ADDRESS"
echo "  export VAULT_ADDRESS=$VAULT_ADDRESS"
echo "  export MARS_ADAPTER_ADDRESS=$MARS_ADAPTER_ADDRESS"
echo "  export IBC_ADAPTER_ADDRESS=$IBC_ADAPTER_ADDRESS"
echo ""
echo "All contracts have been instantiated and configured successfully!"
