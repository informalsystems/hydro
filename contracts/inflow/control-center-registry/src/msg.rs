use cosmwasm_schema::cw_serde;

pub use interface::inflow_control_center_registry::{
    ConfigResponse, ControlCentersResponse, QueryMsg,
};

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub control_centers: Vec<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    UpdateAdmin { new_admin: String },
    AddControlCenter { address: String },
    RemoveControlCenter { address: String },
}
