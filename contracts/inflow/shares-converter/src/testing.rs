use crate::{
    contract::{execute, instantiate, query},
    error::ContractError,
    msg::{AllPairsResponse, ConversionPair, ExecuteMsg, InstantiateMsg, PairResponse, QueryMsg},
};
use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env, MockApi};
use cosmwasm_std::{coins, from_json, Coin, DepsMut};

const ADMIN: &str = "admin";
const USER: &str = "user1";
const NEUTRON_DENOM: &str = "ibc/C744236911B9CAA806DCDD730C9EBA323CB53B822B2EBD77BF977412B2E64DA1";
const HUB_DENOM: &str =
    "factory/cosmos1qg5ega6dykkxc307y25pecuufrjkxkaggkkxh7nad0vhyhtuhw3s6ufdm4/inflow_uatom";

fn api() -> MockApi {
    MockApi::default()
}

fn do_instantiate(deps: DepsMut) {
    let admin_addr = api().addr_make(ADMIN);
    let msg = InstantiateMsg {
        admin: admin_addr.to_string(),
        pairs: vec![ConversionPair {
            neutron_shares_denom: NEUTRON_DENOM.to_string(),
            cosmos_hub_shares_denom: HUB_DENOM.to_string(),
        }],
    };
    instantiate(deps, mock_env(), message_info(&admin_addr, &[]), msg).unwrap();
}

#[test]
fn test_instantiate_registers_pairs() {
    let mut deps = mock_dependencies();
    do_instantiate(deps.as_mut());

    let res: Option<PairResponse> = from_json(
        query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::Pair {
                neutron_denom: NEUTRON_DENOM.to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();

    let pair = res.unwrap();
    assert_eq!(pair.neutron_shares_denom, NEUTRON_DENOM);
    assert_eq!(pair.cosmos_hub_shares_denom, HUB_DENOM);
}

#[test]
fn test_convert_unknown_denom_fails() {
    let mut deps = mock_dependencies();
    do_instantiate(deps.as_mut());
    let user = api().addr_make(USER);

    let err = execute(
        deps.as_mut(),
        mock_env(),
        message_info(&user, &coins(100, "ibc/UNKNOWN")),
        ExecuteMsg::Convert {},
    )
    .unwrap_err();

    assert!(matches!(err, ContractError::PairNotFound { .. }));
}

#[test]
fn test_convert_no_funds_fails() {
    let mut deps = mock_dependencies();
    do_instantiate(deps.as_mut());
    let user = api().addr_make(USER);

    let err = execute(
        deps.as_mut(),
        mock_env(),
        message_info(&user, &[]),
        ExecuteMsg::Convert {},
    )
    .unwrap_err();

    assert!(matches!(err, ContractError::InvalidFunds));
}

#[test]
fn test_convert_multiple_funds_fails() {
    let mut deps = mock_dependencies();
    do_instantiate(deps.as_mut());
    let user = api().addr_make(USER);

    let err = execute(
        deps.as_mut(),
        mock_env(),
        message_info(
            &user,
            &[
                Coin::new(100u128, NEUTRON_DENOM),
                Coin::new(50u128, "uatom"),
            ],
        ),
        ExecuteMsg::Convert {},
    )
    .unwrap_err();

    assert!(matches!(err, ContractError::InvalidFunds));
}

#[test]
fn test_convert_insufficient_balance_fails() {
    let mut deps = mock_dependencies();
    do_instantiate(deps.as_mut());
    let user = api().addr_make(USER);

    // Contract has zero Hub shares — conversion should fail
    let err = execute(
        deps.as_mut(),
        mock_env(),
        message_info(&user, &coins(100, NEUTRON_DENOM)),
        ExecuteMsg::Convert {},
    )
    .unwrap_err();

    assert!(matches!(err, ContractError::InsufficientBalance { .. }));
}

#[test]
fn test_add_pair_unauthorized() {
    let mut deps = mock_dependencies();
    do_instantiate(deps.as_mut());
    let user = api().addr_make(USER);

    let err = execute(
        deps.as_mut(),
        mock_env(),
        message_info(&user, &[]),
        ExecuteMsg::AddPair {
            neutron_shares_denom: "ibc/NEW".to_string(),
            cosmos_hub_shares_denom: "factory/x/new".to_string(),
        },
    )
    .unwrap_err();

    assert!(matches!(err, ContractError::Unauthorized));
}

#[test]
fn test_add_and_remove_pair() {
    let mut deps = mock_dependencies();
    do_instantiate(deps.as_mut());
    let admin = api().addr_make(ADMIN);

    execute(
        deps.as_mut(),
        mock_env(),
        message_info(&admin, &[]),
        ExecuteMsg::AddPair {
            neutron_shares_denom: "ibc/NEW".to_string(),
            cosmos_hub_shares_denom: "factory/x/new".to_string(),
        },
    )
    .unwrap();

    let res: Option<PairResponse> = from_json(
        query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::Pair {
                neutron_denom: "ibc/NEW".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert!(res.is_some());

    execute(
        deps.as_mut(),
        mock_env(),
        message_info(&admin, &[]),
        ExecuteMsg::RemovePair {
            neutron_shares_denom: "ibc/NEW".to_string(),
        },
    )
    .unwrap();

    let res: Option<PairResponse> = from_json(
        query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::Pair {
                neutron_denom: "ibc/NEW".to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert!(res.is_none());
}

#[test]
fn test_add_pair_already_exists() {
    let mut deps = mock_dependencies();
    do_instantiate(deps.as_mut());
    let admin = api().addr_make(ADMIN);

    execute(
        deps.as_mut(),
        mock_env(),
        message_info(&admin, &[]),
        ExecuteMsg::AddPair {
            neutron_shares_denom: "ibc/NEW".to_string(),
            cosmos_hub_shares_denom: "factory/x/new".to_string(),
        },
    )
    .unwrap();

    let err = execute(
        deps.as_mut(),
        mock_env(),
        message_info(&admin, &[]),
        ExecuteMsg::AddPair {
            neutron_shares_denom: "ibc/NEW".to_string(),
            cosmos_hub_shares_denom: "factory/x/new".to_string(),
        },
    )
    .unwrap_err();

    assert!(matches!(
        err,
        ContractError::PairAlreadyExists { denom } if denom == "ibc/NEW"
    ));
}

#[test]
fn test_remove_pair_not_found() {
    let mut deps = mock_dependencies();
    do_instantiate(deps.as_mut());
    let admin = api().addr_make(ADMIN);

    let err = execute(
        deps.as_mut(),
        mock_env(),
        message_info(&admin, &[]),
        ExecuteMsg::RemovePair {
            neutron_shares_denom: "ibc/NONEXISTENT".to_string(),
        },
    )
    .unwrap_err();

    assert!(matches!(
        err,
        ContractError::PairNotFound { denom } if denom == "ibc/NONEXISTENT"
    ));
}

#[test]
fn test_query_all_pairs() {
    let mut deps = mock_dependencies();
    do_instantiate(deps.as_mut());

    let res: AllPairsResponse = from_json(
        query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::AllPairs {
                start_after: None,
                limit: None,
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(res.pairs.len(), 1);
    assert_eq!(res.pairs[0].neutron_shares_denom, NEUTRON_DENOM);
}
