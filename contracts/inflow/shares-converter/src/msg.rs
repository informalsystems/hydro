use cosmwasm_schema::{cw_serde, QueryResponses};

#[cw_serde]
pub struct ConversionPair {
    pub neutron_shares_denom: String,
    pub cosmos_hub_shares_denom: String,
}

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    /// Optional initial conversion pairs to register at instantiation.
    pub pairs: Vec<ConversionPair>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// User sends Neutron IBC shares and receives equal Cosmos Hub vault shares back.
    Convert {},
    /// Admin: register a new conversion pair (or update an existing one).
    AddPair {
        neutron_shares_denom: String,
        cosmos_hub_shares_denom: String,
    },
    /// Admin: remove a conversion pair (e.g. once conversion window is closed).
    RemovePair { neutron_shares_denom: String },
    /// Admin: replace the current admin with a new address.
    UpdateAdmin { new_admin: String },
}

#[cw_serde]
pub struct ConfigResponse {
    pub admin: String,
}

#[cw_serde]
pub struct PairResponse {
    pub neutron_shares_denom: String,
    pub cosmos_hub_shares_denom: String,
}

#[cw_serde]
pub struct AllPairsResponse {
    pub pairs: Vec<PairResponse>,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},
    #[returns(Option<PairResponse>)]
    Pair { neutron_denom: String },
    #[returns(AllPairsResponse)]
    AllPairs {
        start_after: Option<String>,
        limit: Option<u32>,
    },
}
