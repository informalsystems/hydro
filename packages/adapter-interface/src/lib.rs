pub mod msg;

// Re-export the main types for convenience
pub use msg::{
    AdapterConfigResponse, AdapterExecuteMsg, AdapterQueryMsg, AvailableAmountResponse,
    InflowDepositResponse, InflowDepositsResponse, RegisteredInflowInfo, RegisteredInflowsResponse,
    TimeEstimateResponse, TotalDepositedResponse,
};
