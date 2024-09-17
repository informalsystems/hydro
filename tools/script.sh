# Create the ICQ queries by running the go script in this folder
go run main.go

# populate the config for the ICQ relayer
export RELAYER_NEUTRON_CHAIN_RPC_ADDR=https://rpc-falcron.pion-1.ntrn.tech:443  
export RELAYER_NEUTRON_CHAIN_REST_ADDR=https://rest-falcron.pion-1.ntrn.tech  
export RELAYER_NEUTRON_CHAIN_HOME_DIR=$HOME/.neutrond
export RELAYER_NEUTRON_CHAIN_SIGN_KEY_NAME=money
export RELAYER_NEUTRON_CHAIN_KEYRING_BACKEND=test
export RELAYER_NEUTRON_CHAIN_DENOM=untrn  
export RELAYER_NEUTRON_CHAIN_GAS_PRICES=0.0055untrn
export RELAYER_NEUTRON_CHAIN_GAS_PRICE_MULTIPLIER=2  
export RELAYER_NEUTRON_CHAIN_MAX_GAS_PRICE=0.011  
export RELAYER_NEUTRON_CHAIN_GAS_ADJUSTMENT=1.5  
export RELAYER_NEUTRON_CHAIN_CONNECTION_ID=connection-42  
export RELAYER_NEUTRON_CHAIN_OUTPUT_FORMAT=json  
export RELAYER_TARGET_CHAIN_RPC_ADDR=https://rpc-rs.cosmos.nodestake.top:443
# this needs to be the address of the contract
export RELAYER_REGISTRY_ADDRESSES=neutron15e0r3h6nw4d9yhe2y5kslaq9t35pdk4egm5qd8nytfmzwl9msyssew5339

#####
# typically, no need to modify these  
export RELAYER_ALLOW_TX_QUERIES=false  
export RELAYER_ALLOW_KV_CALLBACKS=true  
export RELAYER_STORAGE_PATH=$HOME/.neutron_queries_relayer/leveldb
export LOGGER_LEVEL=debug
#####

# Run the relayer
cd ../../neutron-query-relayer
go run ./cmd/neutron_query_relayer start