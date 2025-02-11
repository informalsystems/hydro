use std::env::current_dir;
use std::fs::create_dir_all;

use cosmwasm_schema::{export_schema, remove_schemas, schema_for};
use dao_interface::voting::{
    InfoResponse, TotalPowerAtHeightResponse, VotingPowerAtHeightResponse,
};
use dao_voting_adapter::{
    msg::{ExecuteMsg, InstantiateMsg},
    query::{ConfigResponse, DaoResponse, QueryMsg},
};

fn main() {
    let mut out_dir = current_dir().unwrap();
    out_dir.push("schema");
    create_dir_all(&out_dir).unwrap();
    remove_schemas(&out_dir).unwrap();

    export_schema(&schema_for!(InstantiateMsg), &out_dir);
    export_schema(&schema_for!(ExecuteMsg), &out_dir);
    export_schema(&schema_for!(QueryMsg), &out_dir);

    export_schema(&schema_for!(ConfigResponse), &out_dir);
    export_schema(&schema_for!(TotalPowerAtHeightResponse), &out_dir);
    export_schema(&schema_for!(VotingPowerAtHeightResponse), &out_dir);
    export_schema(&schema_for!(InfoResponse), &out_dir);
    export_schema(&schema_for!(DaoResponse), &out_dir);
}
