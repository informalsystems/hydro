use std::marker::PhantomData;

use cosmwasm_std::{
    from_json,
    testing::{mock_env, MockApi, MockQuerier, MockStorage},
    Addr, Coin, MessageInfo, OwnedDeps, Uint128,
};
use interface::inflow_control_center::{ExecuteMsg, QueryMsg};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    contract::{execute, instantiate, query},
    msg::InstantiateMsg,
    state::{DEPLOYED_AMOUNT, SUBVAULTS},
};

const WHITELIST: &str = "whitelist1";
const USER1: &str = "user1";
const USER2: &str = "user2";
const USER3: &str = "user3";
const SUBVAULT1: &str = "subvault1";
const SUBVAULT2: &str = "subvault2";
const DEFAULT_DEPOSIT_CAP: Uint128 = Uint128::new(10000000);

pub fn mock_dependencies() -> OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery> {
    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
        querier: MockQuerier::new(&[]),
        custom_query_type: PhantomData,
    }
}

pub fn get_message_info(api: &MockApi, sender: &str, funds: &[Coin]) -> MessageInfo {
    MessageInfo {
        sender: api.addr_make(sender),
        funds: funds.to_vec(),
    }
}

fn get_default_instantiate_msg(
    deposit_cap: Uint128,
    whitelist_addr: Addr,
    subvaults: Vec<Addr>,
) -> InstantiateMsg {
    InstantiateMsg {
        deposit_cap,
        whitelist: vec![whitelist_addr.to_string()],
        subvaults: subvaults
            .iter()
            .map(|subvault_addr| subvault_addr.to_string())
            .collect(),
    }
}

#[test]
fn submit_deployed_amount_test() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST);
    let user1_address = deps.api.addr_make(USER1);

    let instantiate_msg = get_default_instantiate_msg(DEFAULT_DEPOSIT_CAP, whitelist_addr, vec![]);

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Try submitting deployed amount with a user that is not whitelisted
    let exec_msg = ExecuteMsg::SubmitDeployedAmount {
        amount: Uint128::from(100u128),
    };
    let info_user_1 = get_message_info(&deps.api, user1_address.as_ref(), &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info_user_1.clone(),
        exec_msg.clone(),
    );
    assert!(res.is_err());
    assert!(res.err().unwrap().to_string().contains("Unauthorized"),);

    let query_msg = QueryMsg::DeployedAmount {};
    // Deployed amount should be zero before any submission
    let query_res = query(deps.as_ref(), env.clone(), query_msg.clone());
    assert!(query_res.is_ok());
    let value: Uint128 = from_json(query_res.unwrap()).unwrap();
    assert_eq!(value, Uint128::zero());

    let info_whitelisted = get_message_info(&deps.api, WHITELIST, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info_whitelisted.clone(),
        exec_msg,
    );
    assert!(res.is_ok());

    // Deployed amount should be updated after whitelisted user submits it
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(query_res.is_ok());
    let value: Uint128 = from_json(query_res.unwrap()).unwrap();
    assert_eq!(value, Uint128::from(100u128));
}

#[test]
fn whitelist_add_remove_test() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let user1_address = deps.api.addr_make(USER1);
    let user2_address = deps.api.addr_make(USER2);
    let user3_address = deps.api.addr_make(USER3);

    let instantiate_msg =
        get_default_instantiate_msg(DEFAULT_DEPOSIT_CAP, user1_address.clone(), vec![]);

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Have a non-whitelisted address try to add new address to the whitelist
    let info = get_message_info(&deps.api, USER2, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::AddToWhitelist {
            address: user2_address.to_string(),
        },
    )
    .unwrap_err()
    .to_string()
    .contains("Unauthorized");

    // Have a whitelisted address add a new address to the whitelist
    let info = get_message_info(&deps.api, USER1, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::AddToWhitelist {
            address: user2_address.to_string(),
        },
    );
    assert!(res.is_ok());

    // Have a whitelisted address try to add new address to the whitelist when that address is already in the list
    let info = get_message_info(&deps.api, USER1, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::AddToWhitelist {
            address: user2_address.to_string(),
        },
    );
    assert!(res.unwrap_err().to_string().contains(&format!(
        "address {user2_address} is already in the whitelist"
    )));

    // Have a whitelisted address try to remove non-whitelisted address
    let info = get_message_info(&deps.api, USER2, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RemoveFromWhitelist {
            address: user3_address.to_string(),
        },
    );
    assert!(res
        .unwrap_err()
        .to_string()
        .contains(&format!("address {user3_address} is not in the whitelist")));

    // Have a whitelisted address remove some whitelisted address
    let info = get_message_info(&deps.api, USER1, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RemoveFromWhitelist {
            address: user1_address.to_string(),
        },
    );
    assert!(res.is_ok());

    // Have a whitelisted address try to remove the last whitelisted address
    let info = get_message_info(&deps.api, USER2, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RemoveFromWhitelist {
            address: user2_address.to_string(),
        },
    );
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("cannot remove last outstanding whitelisted address"));
}

#[test]
fn subvaults_test() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_address = deps.api.addr_make(WHITELIST);
    let subvault1_address = deps.api.addr_make(SUBVAULT1);
    let subvault2_address = deps.api.addr_make(SUBVAULT2);

    let instantiate_msg = get_default_instantiate_msg(
        DEFAULT_DEPOSIT_CAP,
        whitelist_address.clone(),
        vec![subvault1_address.clone()],
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Have known vault update the deployed amount
    let deployed_amount_update_1 = Uint128::new(1000);
    let info = get_message_info(&deps.api, SUBVAULT1, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::AddToDeployedAmount {
            amount_to_add: deployed_amount_update_1,
        },
    )
    .unwrap();

    assert_eq!(
        DEPLOYED_AMOUNT.load(&deps.storage).unwrap(),
        deployed_amount_update_1
    );

    // Have unauthorized address try to update the deployed amount
    let info = get_message_info(&deps.api, SUBVAULT2, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::AddToDeployedAmount {
            amount_to_add: deployed_amount_update_1,
        },
    )
    .unwrap_err()
    .to_string()
    .contains("Unauthorized");

    // Have a whitelisted address add a new subvault
    let info = get_message_info(&deps.api, WHITELIST, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::AddSubvault {
            address: subvault2_address.to_string(),
        },
    );
    assert!(res.is_ok());

    // Have a newly added subvault update the deployed amount
    let deployed_amount_update_2 = Uint128::new(2000);
    let info = get_message_info(&deps.api, SUBVAULT2, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::AddToDeployedAmount {
            amount_to_add: deployed_amount_update_2,
        },
    );
    assert!(res.is_ok());

    assert_eq!(
        DEPLOYED_AMOUNT.load(&deps.storage).unwrap(),
        deployed_amount_update_1 + deployed_amount_update_2
    );

    // Have known vault subtract from deployed amount
    let deployed_amount_sub_1 = Uint128::new(500);
    let info = get_message_info(&deps.api, SUBVAULT1, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::SubFromDeployedAmount {
            amount_to_sub: deployed_amount_sub_1,
        },
    )
    .unwrap();

    assert_eq!(
        DEPLOYED_AMOUNT.load(&deps.storage).unwrap(),
        deployed_amount_update_1 + deployed_amount_update_2 - deployed_amount_sub_1
    );

    // Have unauthorized address try to subtract from deployed amount
    let unauthorized_address = deps.api.addr_make(USER1);
    let info = get_message_info(&deps.api, USER1, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::SubFromDeployedAmount {
            amount_to_sub: Uint128::new(100),
        },
    )
    .unwrap_err()
    .to_string()
    .contains("Unauthorized");

    // Have subvault try to subtract more than available (should cause overflow/underflow error)
    let info = get_message_info(&deps.api, SUBVAULT2, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::SubFromDeployedAmount {
            amount_to_sub: Uint128::new(10000),
        },
    );
    assert!(res.is_err());

    // Have another subvault subtract from deployed amount
    let deployed_amount_sub_2 = Uint128::new(1500);
    let info = get_message_info(&deps.api, SUBVAULT2, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::SubFromDeployedAmount {
            amount_to_sub: deployed_amount_sub_2,
        },
    );
    assert!(res.is_ok());

    assert_eq!(
        DEPLOYED_AMOUNT.load(&deps.storage).unwrap(),
        deployed_amount_update_1 + deployed_amount_update_2 - deployed_amount_sub_1 - deployed_amount_sub_2
    );

    // Have a whitelisted address remove one subvault
    let info = get_message_info(&deps.api, WHITELIST, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RemoveSubvault {
            address: subvault2_address.to_string(),
        },
    );
    assert!(res.is_ok());

    assert_eq!(
        SUBVAULTS
            .may_load(&deps.storage, subvault2_address)
            .unwrap(),
        None
    );
}
