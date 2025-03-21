use std::env::current_dir;
use std::fs::create_dir_all;

use cosmwasm_schema::{export_schema, remove_schemas, schema_for};

use hydro::msg::{ExecuteMsg, InstantiateMsg};
use hydro::query::{
    AllUserLockupsResponse, AllUserLockupsWithTrancheInfosResponse, AllVotesResponse,
    AllVotesRoundTrancheResponse, ConstantsResponse, CurrentRoundResponse,
    ExpiredUserLockupsResponse, ICQManagersResponse, LiquidityDeploymentResponse, ProposalResponse,
    QueryMsg, RegisteredValidatorQueriesResponse, RoundEndResponse, RoundProposalsResponse,
    RoundTotalVotingPowerResponse, RoundTrancheLiquidityDeploymentsResponse,
    SpecificUserLockupsResponse, SpecificUserLockupsWithTrancheInfosResponse,
    TokenGroupRatioResponse, TokenInfoProvidersResponse, TopNProposalsResponse,
    TotalLockedTokensResponse, TotalPowerAtHeightResponse, TranchesResponse, UserVotesResponse,
    UserVotingPowerResponse, VotingPowerAtHeightResponse, WhitelistAdminsResponse,
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
    export_schema(
        &schema_for!(AllUserLockupsWithTrancheInfosResponse),
        &out_dir,
    );
    export_schema(&schema_for!(SpecificUserLockupsResponse), &out_dir);
    export_schema(
        &schema_for!(SpecificUserLockupsWithTrancheInfosResponse),
        &out_dir,
    );
    export_schema(&schema_for!(ExpiredUserLockupsResponse), &out_dir);
    export_schema(&schema_for!(UserVotingPowerResponse), &out_dir);
    export_schema(&schema_for!(UserVotesResponse), &out_dir);
    export_schema(&schema_for!(AllVotesResponse), &out_dir);
    export_schema(&schema_for!(AllVotesRoundTrancheResponse), &out_dir);
    export_schema(&schema_for!(CurrentRoundResponse), &out_dir);
    export_schema(&schema_for!(RoundEndResponse), &out_dir);
    export_schema(&schema_for!(RoundTotalVotingPowerResponse), &out_dir);
    export_schema(&schema_for!(ProposalResponse), &out_dir);
    export_schema(&schema_for!(TopNProposalsResponse), &out_dir);
    export_schema(&schema_for!(WhitelistResponse), &out_dir);
    export_schema(&schema_for!(WhitelistAdminsResponse), &out_dir);
    export_schema(&schema_for!(TotalLockedTokensResponse), &out_dir);
    export_schema(&schema_for!(LiquidityDeploymentResponse), &out_dir);
    export_schema(
        &schema_for!(RoundTrancheLiquidityDeploymentsResponse),
        &out_dir,
    );
    export_schema(&schema_for!(ICQManagersResponse), &out_dir);
    export_schema(&schema_for!(RegisteredValidatorQueriesResponse), &out_dir);
    export_schema(&schema_for!(TokenGroupRatioResponse), &out_dir);
    export_schema(&schema_for!(TotalPowerAtHeightResponse), &out_dir);
    export_schema(&schema_for!(VotingPowerAtHeightResponse), &out_dir);
    export_schema(&schema_for!(TokenInfoProvidersResponse), &out_dir);
}
