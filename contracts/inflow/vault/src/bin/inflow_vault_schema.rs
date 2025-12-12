use cosmwasm_schema::write_api;
use interface::inflow_vault::{ExecuteMsg, QueryMsg};
use vault::msg::InstantiateMsg;

fn main() {
    write_api! {
        instantiate: InstantiateMsg,
        execute: ExecuteMsg,
        query: QueryMsg,
    };
}
