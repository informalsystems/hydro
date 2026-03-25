use claim::{msg::ExecuteMsg, msg::InstantiateMsg, query::QueryMsg};
use cosmwasm_schema::write_api;

fn main() {
    write_api! {
        instantiate: InstantiateMsg,
        execute: ExecuteMsg,
        query: QueryMsg,
    };
}
