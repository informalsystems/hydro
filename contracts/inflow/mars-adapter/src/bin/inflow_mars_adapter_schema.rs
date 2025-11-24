use cosmwasm_schema::write_api;
use interface::adapter::{AdapterExecuteMsg, AdapterQueryMsg};
use mars_adapter::msg::InstantiateMsg;

fn main() {
    write_api! {
        instantiate: InstantiateMsg,
        execute: AdapterExecuteMsg,
        query: AdapterQueryMsg,
    };
}
