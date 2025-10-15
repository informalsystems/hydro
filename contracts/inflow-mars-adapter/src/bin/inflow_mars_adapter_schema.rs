use cosmwasm_schema::write_api;
use inflow_mars_adapter::msg::{AdapterExecuteMsg, AdapterQueryMsg, InstantiateMsg};

fn main() {
    write_api! {
        instantiate: InstantiateMsg,
        execute: AdapterExecuteMsg,
        query: AdapterQueryMsg,
    };
}
