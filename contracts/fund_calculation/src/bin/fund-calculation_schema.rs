use std::env::current_dir;
use std::fs::create_dir_all;

use cosmwasm_schema::{export_schema, remove_schemas, schema_for};

use tribute::msg::{ExecuteMsg, InstantiateMsg};
use tribute::query::{
    ConfigResponse, HistoricalTributeClaimsResponse, OutstandingTributeClaimsResponse,
    ProposalTributesResponse, QueryMsg, RoundTributesResponse,
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
    export_schema(&schema_for!(ProposalTributesResponse), &out_dir);
    export_schema(&schema_for!(HistoricalTributeClaimsResponse), &out_dir);
    export_schema(&schema_for!(RoundTributesResponse), &out_dir);
    export_schema(&schema_for!(OutstandingTributeClaimsResponse), &out_dir);
}
