use cosmwasm_schema::write_api;
use trusted_derivative_token_info_provider::{
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
