use cosmwasm_schema::write_api;
use inflow_control_center::msg::InstantiateMsg;
use interface::inflow_control_center::{ExecuteMsg, QueryMsg};

fn main() {
    write_api! {
        instantiate: InstantiateMsg,
        execute: ExecuteMsg,
        query: QueryMsg,
    };
}
