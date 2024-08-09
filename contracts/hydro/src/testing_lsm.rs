use crate::contract::{query_user_vote, query_whitelist, query_whitelist_admins, MAX_LOCK_ENTRIES};
use crate::state::Tranche;
use crate::testing::{get_default_instantiate_msg, DEFAULT_DENOM, ONE_MONTH_IN_NANO_SECONDS};
use crate::{
    contract::{
        compute_current_round_id, execute, instantiate, query_all_user_lockups, query_constants,
        query_proposal, query_round_total_power, query_round_tranche_proposals,
        query_top_n_proposals,
    },
    msg::{ExecuteMsg, InstantiateMsg},
};
use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{BankMsg, CosmosMsg, Deps, Timestamp};
use cosmwasm_std::{Coin, StdError, StdResult};
use proptest::prelude::*;

// Performs a default initialization that can be used for tests
// that need nothing specific in the initialization.
fn do_default_initialization() -> (
    String,
    cosmwasm_std::OwnedDeps<
        cosmwasm_std::MemoryStorage,
        cosmwasm_std::testing::MockApi,
        cosmwasm_std::testing::MockQuerier,
    >,
    cosmwasm_std::Env,
) {
    let user_address = "addr0000";

    let (mut deps, env, info) = (
        mock_dependencies(),
        mock_env(),
        mock_info(user_address.clone(), &[]),
    );
    let msg = get_default_instantiate_msg();

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    (user_address.to_string(), deps, env)
}

#[test]
fn incomplete_validator_initialization_test() {
    let (user_address, mut deps, env) = do_default_initialization();

    let info1 = mock_info(
        user_address.as_str(),
        &[Coin::new(1000, DEFAULT_DENOM.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info1.clone(), msg);
    assert!(res.is_err());
    // check that the error message contains "reading validators, possibly not set yet"
    assert!(
        res.as_ref()
            .unwrap_err()
            .to_string()
            .contains("reading validators, possibly not set yet"),
        // print the error message if it's not there for debugging
        "{}",
        res.as_ref().unwrap_err(),
    );
}
