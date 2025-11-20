use cosmwasm_schema::write_api;
use proxy::msg::InstantiateMsg;
use proxy::{ExecuteMsg, QueryMsg};

fn main() {
    write_api! {
        instantiate: InstantiateMsg,
        execute: ExecuteMsg,
        query: QueryMsg,
    };
}
