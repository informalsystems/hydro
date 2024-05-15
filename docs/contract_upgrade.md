# Hydro Contracts Upgrade via Governance on the Controller Chain

## Preconditions
To upgrade the CosmWasm smart contracts on the host chain through the governance on the controller chain, the following preconditions must be met:

1. **Registration of Interchain Account**: An interchain account for the controller governance module must be registered on the host side. To achieve this, a governance proposal should be submitted on the controller chain. Below is an example of the proposal JSON:

    ```json
    {
        "messages": [
            {
                "@type": "/ibc.applications.interchain_accounts.controller.v1.MsgRegisterInterchainAccount",
                "owner": "cosmos10d07y265gmmuvt4z0w9aw880jnsr700j6zn9kn",
                "connection_id": "connection-0"
            }
        ],
        "metadata": "",
        "deposit": "10000000stake",
        "title": "Register Governance ICA Account",
        "summary": "Proposal for registering governance ICA account on the host chain"
    }
    ```

    - `owner`: Address of the governance module on the controller chain.
    - `deposit`: Must be given in the denomination known to the controller chain.
    - `connection_id`: Represents the ID of the connection established between the controller and host chain.

2. **Admin Address Set on the Contracts**: The admin address set on the contract(s) deployed on the host chain must be the registered ICA address of the governance module. To find the registered governance ICA address, query the controller chain:

    ```bash
    ./binary query interchain-accounts controller interchain-account [owner] [connection-id] [flags]
    ```

    - `owner`: Address of the governance module on the controller chain.

## Upgrading Contract(s)
If all preconditions are met, follow these steps to upgrade the contract:

1. **Submit Proposal**: Submit a governance proposal on the controller chain to upgrade the contract deployed on the host chain. The proposal must contain a `MsgSendTx` ICA message, which will contain serialized `MsgMigrateContract` wasm message set as packet data. When the proposal passes on the controller, `MsgSendTx` will be executed on the controller side. Finally, upon receiving this message on the host side, packet data will be deserialized, and `MsgMigrateContract` will be executed on the host chain, thus upgrading the contract.

    ```json
    {
        "messages": [
            {
                "@type": "/ibc.applications.interchain_accounts.controller.v1.MsgSendTx",
                "owner": "cosmos10d07y265gmmuvt4z0w9aw880jnsr700j6zn9kn",
                "connection_id": "connection-0",
                "packet_data": {
                    "type": "TYPE_EXECUTE_TX",
                    "data": "CrEBCiQvY29zbXdhc20ud2FzbS52MS5Nc2dNaWdyYXRlQ29udHJhY3QSiAEKP3dhc20xdWQ1am5wdWwzMDhkc3U2d21qZ3ZzNXk3aHZlNGg5dmtuMjU2d3drdTVtaHJqcHNmNHYzcTZxYzllZBI/d2FzbTE0aGoydGF2cThmcGVzZHd4eGN1NDRydHkzaGg5MHZodWpydmNtc3RsNHpyM3R4bWZ2dzlzMHBoZzRkGAIiAnt9",
                    "memo": ""
                },
                "relative_timeout": 600000000000
            }
        ],
        "metadata": "",
        "deposit": "10000000stake",
        "title": "Contract Upgrade Proposal",
        "summary": "Contract upgrade"
    }
    ```

    - `owner`: Address of the governance module on the controller chain.

     **Calculate Packet Data**: To calculate the packet data representing the `MsgMigrateContract` message, send the following transaction to the host chain:

    ```bash
    tx interchain-accounts host generate-packet-data '{
    "@type":"/cosmwasm.wasm.v1.MsgMigrateContract",
    "sender":"",
    "contract":"wasm14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9s0phg4d",
    "code_id":"2",
    "msg":{}
    }' --encoding proto3
    ```

    - `sender`: Governance module ICA address registered on the host chain. This address is set as an admin address on the contract that should be upgraded (see preconditions).
    - `contract`: Address of the contract that should be upgraded.
    - `code_id`: The new code to upgrade the contract (this code must be stored on the host chain before the upgrade starts).
    - `msg`: Message containing upgrade data.
