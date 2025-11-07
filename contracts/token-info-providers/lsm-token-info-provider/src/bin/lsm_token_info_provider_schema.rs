use cosmwasm_schema::write_api;
use lsm_token_info_provider::{
    msg::{ExecuteMsg, InstantiateMsg},
    query::QueryMsg,
};

fn main() {
    write_api! {
        instantiate: InstantiateMsg,
        execute: ExecuteMsg,
        query: QueryMsg,
    };
}
