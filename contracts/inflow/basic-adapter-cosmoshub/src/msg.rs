use cosmwasm_schema::{cw_serde, QueryResponses};

pub use interface::inflow_adapter::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AdminsResponse, AllPositionsResponse,
    AvailableAmountResponse, DepositorPositionResponse, DepositorPositionsResponse,
    RegisteredDepositorInfo, RegisteredDepositorsResponse, TimeEstimateResponse,
};

#[cw_serde]
pub struct InstantiateMsg {
    /// Admins who can manage depositors and the admin list
    pub admins: Vec<String>,
    /// Initial depositor addresses (all enabled, no capabilities)
    pub initial_depositors: Vec<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    StandardAction(AdapterInterfaceMsg),
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(cosmwasm_std::Binary)]
    StandardQuery(AdapterInterfaceQueryMsg),
}

#[cw_serde]
pub struct ConfigResponse {}
