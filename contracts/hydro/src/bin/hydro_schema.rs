use std::env::current_dir;
use std::fs::create_dir_all;

use cosmwasm_schema::{export_schema, remove_schemas, schema_for};

use hydro::msg::{ExecuteMsg, InstantiateMsg};
use hydro::query::{
    AllUserLockupsResponse, ConstantsResponse, CurrentRoundResponse, ExpiredUserLockupsResponse,
    ProposalResponse, QueryMsg, RoundEndResponse, RoundProposalsResponse,
    RoundTotalVotingPowerResponse, TopNProposalsResponse, TotalLockedTokensResponse,
    TranchesResponse, UserVoteResponse, UserVotingPowerResponse, WhitelistAdminsResponse,
    WhitelistResponse,
};

fn main() {
    let mut out_dir = current_dir().unwrap();
    out_dir.push("schema");
    create_dir_all(&out_dir).unwrap();
    remove_schemas(&out_dir).unwrap();

    export_schema(&schema_for!(InstantiateMsg), &out_dir);
    export_schema(&schema_for!(ExecuteMsg), &out_dir);
    export_schema(&schema_for!(QueryMsg), &out_dir);

    export_schema(&schema_for!(ConstantsResponse), &out_dir);
    export_schema(&schema_for!(TranchesResponse), &out_dir);
    export_schema(&schema_for!(RoundProposalsResponse), &out_dir);
    export_schema(&schema_for!(AllUserLockupsResponse), &out_dir);
    export_schema(&schema_for!(ExpiredUserLockupsResponse), &out_dir);
    export_schema(&schema_for!(UserVotingPowerResponse), &out_dir);
    export_schema(&schema_for!(UserVoteResponse), &out_dir);
    export_schema(&schema_for!(CurrentRoundResponse), &out_dir);
    export_schema(&schema_for!(RoundEndResponse), &out_dir);
    export_schema(&schema_for!(RoundTotalVotingPowerResponse), &out_dir);
    export_schema(&schema_for!(ProposalResponse), &out_dir);
    export_schema(&schema_for!(TopNProposalsResponse), &out_dir);
    export_schema(&schema_for!(WhitelistResponse), &out_dir);
    export_schema(&schema_for!(WhitelistAdminsResponse), &out_dir);
    export_schema(&schema_for!(TotalLockedTokensResponse), &out_dir);
}
