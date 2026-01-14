# Cosmos Hub Oversight

To execute a transaction on Hydro's DAOs via a Cosmos Hub governance proposal, follow the steps below.

## Create proposal in the Hydro Oversight DAO

This DAO has a single member, an ICA controlled by the Cosmos Hub.
DAODAO offers a UI to create this proposal. For example, a proposal to replace the Hydro Committee DAO members would look like this.
Once the proposal is live, note the proposal ID.

## Create governance proposal on Cosmos Hub

Next, create a governance proposal on Cosmos Hub that votes `yes` on the Hydro Oversight DAO proposal from the ICA that is the single DAO member.
This proposal must include this message as payload:

```json
{
  "@type": "/ibc.applications.interchain_accounts.controller.v1.MsgSendTx",
  "owner": "<cosmos_hub_gov_module_account>",
  "connection_id": "<connection_id_to_neutron>",
  "packet_data": {
    "type": "TYPE_EXECUTE_TX",
    "data": "<base64_encoded_cosmos_tx>",
    "memo": ""
  },
  "relative_timeout": "600000000000"
}
```

The base64 encoded cosmos tx must be the base64 encoding of the message:

```json
{
  "messages": [
    {
      "@type": "/cosmwasm.wasm.v1.MsgExecuteContract",
      "sender": "neutron1u5gaalvlg2nr5spaxuvxuhx69yx05xfs0lnxjeuezq53px9au9esfrhj7t",
      "contract": "neutron10guejqwqzpn9kpmmp64z8vgtnr3rfscq2gf2vcw9gmgcg5n52h8qgfsdvs",
      "msg": "<base64_encoded_vote_msg>",
      "funds": []
    }
  ]
}
```

Finally, the `<base64_encoded_vote_msg>` must be the base64 encoding of the vote message for the Hydro Oversight DAO proposal, for example:

```json
{
  "vote": {
    "proposal_id": <PROPOSAL_ID_NUMBER>,
    "vote": "yes"
  }
}
```

## References

- [Oversight DAO](https://daodao.zone/dao/neutron10guejqwqzpn9kpmmp64z8vgtnr3rfscq2gf2vcw9gmgcg5n52h8qgfsdvs)
- [Cosmos Hub gov proposal creating the Interchain Account](https://www.mintscan.io/cosmos/proposals/935)
- [Cosmos Hub Governance Submitting](https://hub.cosmos.network/main/governance/submitting.html)
- [IBC Interchain Accounts Tutorial](https://tutorials.cosmos.network/academy/3-ibc/8-ica.html)
- [DAO DAO Contracts](https://github.com/DA0-DA0/dao-contracts)
- [How to use groups with ICA](https://github.com/cosmos/ibc-go/wiki/How-to-use-groups-with-ICA)
- [CosmWasm wasmd tx.proto](https://github.com/CosmWasm/wasmd/blob/main/proto/cosmwasm/wasm/v1/tx.proto) 
