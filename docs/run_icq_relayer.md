# Neutron Interchain Queries (ICQ) Relayer Setup
Repository: [ICQ Relayer](https://github.com/neutron-org/neutron-query-relayer) (v0.3.0 is the latest tag)

First we need to set up a configuration that will be used by the ICQ relayer. Create a file (e.g. “.env_icq_relayer” in current directory) with the following content (provided values are just an example on pion-1 Neutron testnet; comments explaining specific parameters can be removed):

RELAYER_NEUTRON_CHAIN_RPC_ADDR=https://rpc-falcron.pion-1.ntrn.tech:443  
RELAYER_NEUTRON_CHAIN_REST_ADDR=https://rest-falcron.pion-1.ntrn.tech  
\# *Path to a directory that contains a key that ICQ relayer will use for signing transactions.*  
\# *Usually a node home directory (e.g. $HOME/.neutrond)*  
RELAYER_NEUTRON_CHAIN_HOME_DIR=$HOME/.neutrond  
\# *The name of the key in directory above that ICQ relayer will use for signing transactions*  
RELAYER_NEUTRON_CHAIN_SIGN_KEY_NAME=wallet1  
\# *Keyring backend used to lookup the key above*  
RELAYER_NEUTRON_CHAIN_KEYRING_BACKEND=test  
RELAYER_NEUTRON_CHAIN_DENOM=untrn  
RELAYER_NEUTRON_CHAIN_GAS_PRICES=0.0055untrn  
RELAYER_NEUTRON_CHAIN_MAX_GAS_PRICE=0.011  
\# *Not sure what this is, docs doesn’t mention it, but the process fails to start if it is not specified*  
RELAYER_NEUTRON_CHAIN_GAS_PRICE_MULTIPLIER=2  
\# *Gas adjustment used after transaction gas estimation*  
RELAYER_NEUTRON_CHAIN_GAS_ADJUSTMENT=1.5  
\# *Connection ID (Neutron end) of IBC connection between Neutron and target chain (Gaia)*  
RELAYER_NEUTRON_CHAIN_CONNECTION_ID=connection-42  
RELAYER_NEUTRON_CHAIN_OUTPUT_FORMAT=json or yaml  
\# *RPC address of target chain (in our case Gaia)*  
RELAYER_TARGET_CHAIN_RPC_ADDR=https://rpc-rs.cosmos.nodestake.top:443  
\# ***Hydro contract address only**, to relay only ICQ results for our needs*  
RELAYER_REGISTRY_ADDRESSES=[HYDRO_CONTRACT_ADDRESS]  
\# *We are only using key-value ICQs*  
RELAYER_ALLOW_TX_QUERIES=false  
\# ***Essential** for Hydro to work is to set this param to true*  
RELAYER_ALLOW_KV_CALLBACKS=true  
RELAYER_STORAGE_PATH=$HOME/.neutron_queries_relayer/leveldb  
LOGGER_LEVEL=debug  
\# *For other (optional) parameters description see the github repository at the top*  

Then, execute the following command to load the previously defined environment:  
>sh export $(grep -v '^#' .env_icq_relayer | xargs)

Finally, start the Relayer:  
>neutron_query_relayer start

## Notes
- After executing start command, any misconfiguration will be seen in the logs and the relayer will fail to start. This only applies to misconfigurations such as providing invalid RPC/REST addreses, an absence of the signer key, etc. If, for example, a wrong smart contract address (or no address at all) is provided for *RELAYER_REGISTRY_ADDRESSES*, or a wrong connection ID is provided for *RELAYER_NEUTRON_CHAIN_CONNECTION_ID* parameter, then the relayer will start with whatever values it was provdied with.
- To verify if the relayer started relaying ICQ results for the Hydro smart contract, make the smart contract create at least one ICQ and run the following query against the Neutron:
    >neutrond q interchainqueries registered-queries --owners $HYDRO_CONTRACT_ADDR

    The given ICQ should have non-zero and non-null values for *last_submitted_result_local_height* and *last_submitted_result_remote_height*.