set -eux

if [ "$#" -ne 1 ]; then
    echo "Usage: $0 LSM_PROVIDER_CONTRACT_ADDRESS"
    exit 1
fi

export LSM_PROVIDER_CONTRACT_ADDRESS=$1

export HUB_CHAIN_RPC_ADDR=https://cosmos-rpc.publicnode.com:443
export HUB_CHAIN_REST_ADDR=https://cosmos-api.polkachu.com
export HUB_CHAIN_ID="cosmoshub-4"
export HUB_CHAIN_SIGN_KEY_NAME=submitter
export HUB_CHAIN_KEYRING_BACKEND=test
export HUB_CHAIN_HOME_DIR=$HOME/.gaia

# maximum number of validator ratios to update in a single transaction.
# lower this if you get errors about exceeding the max block size
export BATCH_SIZE=100

# Create the ICQ queries by running the go script in this folder
hub-validators-tool
