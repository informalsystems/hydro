use crate::contract::{execute, instantiate, query, MAX_FEE_BPS};
use crate::error::ContractError;
use crate::msg::{Collection, CollectionConfig, ExecuteMsg, InstantiateMsg};
use crate::query::{
    EventsResponse, ListingResponse, ListingsByCollectionResponse, ListingsByOwnerResponse,
    ListingsResponse, QueryMsg, WhitelistedCollectionsResponse,
};
use crate::state::{self, EventAction, ListingInput};
use cosmwasm_std::testing::{mock_dependencies_with_balances, mock_env, MockApi, MockQuerier};
use cosmwasm_std::{
    coin, from_json, Addr, Api, BankMsg, Binary, Coin, ContractResult, CosmosMsg, Empty, Env,
    MemoryStorage, MessageInfo, OwnedDeps, SystemError, SystemResult, WasmQuery,
};

const ROYALTY_FEE_BPS: u16 = 250;
const SELL_DENOM: &str = "uatom";

pub fn setup() -> (OwnedDeps<MemoryStorage, MockApi, MockQuerier<Empty>>, Env) {
    let deps = mock_dependencies_with_balances(&[]);
    let env = mock_env();
    (deps, env)
}

fn mock_nft_contract(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier<Empty>>,
    owner: &str,
    contract_addr_to_approve: Option<Addr>,
) {
    let owner_response = format!(r#"{{"owner":"{}","approvals":[]}}"#, owner);

    // Mock the NFT contract responses
    deps.querier.update_wasm(
        move |msg| {
            let msg_str = match msg {
                WasmQuery::Smart { msg, .. } => String::from_utf8(msg.to_vec()).unwrap(),
                WasmQuery::Raw { .. } => "".to_string(),
                WasmQuery::ContractInfo { .. } => "".to_string(),
                _ => "".to_string(),
            };
            if msg_str.contains("owner_of") {
                SystemResult::Ok(ContractResult::Ok(Binary::from(
                    owner_response.as_bytes().to_vec(),
                )))
            } else if msg_str.contains("approval") && contract_addr_to_approve.is_some() {
                let marketplace_addr = contract_addr_to_approve.as_ref().unwrap().to_string();
                let approval_response = format!(
                    r#"{{"approval":{{"spender":"{}","expires":{{"never":{{}}}}}}}}"#,
                    marketplace_addr
                );
                SystemResult::Ok(ContractResult::Ok(Binary::from(
                    approval_response.as_bytes().to_vec(),
                )))
            } else if matches!(msg, WasmQuery::ContractInfo { .. }) {
                // Simulate a valid contract info response
                SystemResult::Ok(ContractResult::Ok(Binary::from(
                    r#"{"code_id":1,"creator":"creator","admin":"admin","label":"nft_contract","pinned":false}"#.as_bytes().to_vec(),
                )))
            } else {
                SystemResult::Err(SystemError::Unknown {})
            }
        });
}

#[test]
fn test_instantiate_valid_collections() {
    let (mut deps, env) = setup();
    let admin = deps.api.addr_make("admin");
    let royalty_fee_addr = deps.api.addr_make("royalties");
    let collection1_addr = deps.api.addr_make("collection1");
    let collection2_addr = deps.api.addr_make("collection2");

    mock_nft_contract(
        &mut deps,
        collection1_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    mock_nft_contract(
        &mut deps,
        collection2_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    let msg = InstantiateMsg {
        admin: admin.to_string(),
        collections: vec![
            Collection {
                address: collection1_addr.into_string(),
                config: CollectionConfig {
                    sell_denoms: vec![SELL_DENOM.to_string()],
                    royalty_fee_bps: 0,
                    royalty_fee_recipient: royalty_fee_addr.clone().into_string(),
                },
            },
            Collection {
                address: collection2_addr.into_string(),
                config: CollectionConfig {
                    sell_denoms: vec![SELL_DENOM.to_string()],
                    royalty_fee_bps: MAX_FEE_BPS,
                    royalty_fee_recipient: royalty_fee_addr.clone().into_string(),
                },
            },
        ],
    };
    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };
    let res = instantiate(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(res.attributes[0].key, "action");
    assert_eq!(res.attributes[0].value, "instantiate");
    assert_eq!(res.attributes[1].key, "admin");
    assert_eq!(res.attributes[1].value, admin.as_str());
    assert_eq!(res.attributes[2].key, "collections");
    assert_eq!(res.attributes[2].value, "2");
}

#[test]
fn test_instantiate_invalid_collection_addr_err() {
    let (mut deps, env) = setup();
    let admin = deps.api.addr_make("admin");
    let royalty_fee_addr = deps.api.addr_make("royalties");
    let invalid_collection_addr = deps.api.addr_make("invalid");

    let msg = InstantiateMsg {
        admin: admin.to_string(),
        collections: vec![Collection {
            address: invalid_collection_addr.clone().into_string(),
            config: CollectionConfig {
                sell_denoms: vec![SELL_DENOM.to_string()],
                royalty_fee_bps: 0,
                royalty_fee_recipient: royalty_fee_addr.clone().into_string(),
            },
        }],
    };
    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };
    assert_eq!(
        instantiate(deps.as_mut(), env, info, msg).unwrap_err(),
        ContractError::NotAContract {
            address: invalid_collection_addr.into_string()
        }
    );
}
#[test]
fn test_instantiate_invalid_royalty_fee_err() {
    let (mut deps, env) = setup();
    let admin = deps.api.addr_make("admin");
    let royalty_fee_addr = deps.api.addr_make("royalties");
    let collection_addr = deps.api.addr_make("collection");

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    let msg = InstantiateMsg {
        admin: admin.to_string(),
        collections: vec![Collection {
            address: collection_addr.clone().into_string(),
            config: CollectionConfig {
                sell_denoms: vec![SELL_DENOM.to_string()],
                royalty_fee_bps: MAX_FEE_BPS + 1,
                royalty_fee_recipient: royalty_fee_addr.clone().into_string(),
            },
        }],
    };
    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };
    assert_eq!(
        instantiate(deps.as_mut(), env, info, msg).unwrap_err(),
        ContractError::InvalidRoyaltyFee {
            max_fee_bps: MAX_FEE_BPS
        }
    );
}

#[test]
fn test_instantiate_invalid_sell_denoms_err() {
    let (mut deps, env) = setup();
    let admin = deps.api.addr_make("admin");
    let royalty_fee_addr = deps.api.addr_make("royalties");
    let collection_addr = deps.api.addr_make("collection");

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    let msg = InstantiateMsg {
        admin: admin.to_string(),
        collections: vec![Collection {
            address: collection_addr.clone().into_string(),
            config: CollectionConfig {
                sell_denoms: vec![],
                royalty_fee_bps: 0,
                royalty_fee_recipient: royalty_fee_addr.clone().into_string(),
            },
        }],
    };
    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };
    assert_eq!(
        instantiate(deps.as_mut(), env, info, msg).unwrap_err(),
        ContractError::EmptySellDenoms {}
    );
}

#[test]
fn test_instantiate_invalid_royalty_recipient_err() {
    let (mut deps, env) = setup();
    let admin = deps.api.addr_make("admin");
    let collection_addr = deps.api.addr_make("collection");

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    let msg = InstantiateMsg {
        admin: admin.to_string(),
        collections: vec![Collection {
            address: collection_addr.clone().into_string(),
            config: CollectionConfig {
                sell_denoms: vec![SELL_DENOM.to_string()],
                royalty_fee_bps: MAX_FEE_BPS,
                royalty_fee_recipient: "some_invalid_address".to_owned(),
            },
        }],
    };
    let info = MessageInfo {
        sender: deps.api.addr_make("creator"),
        funds: vec![],
    };
    assert_eq!(
        instantiate(deps.as_mut(), env, info, msg).unwrap_err(),
        ContractError::Std(deps.api.addr_validate("some_invalid_address").unwrap_err())
    );
}

#[test]
fn test_instantiate_invalid_denom_err() {
    let (mut deps, env) = setup();
    let admin = deps.api.addr_make("admin");
    let royalty_fee_addr = deps.api.addr_make("royalties");
    let collection_addr = deps.api.addr_make("collection");

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    let invalid_denoms = [
        "".to_owned(),                           // empty
        std::iter::repeat_n('c', 129).collect(), // over 128 chars
        "1abcd".to_owned(),                      // non-alphabetic start
        "at*m".to_owned(),                       // invalid character
    ];

    for denom in invalid_denoms {
        let msg = InstantiateMsg {
            admin: admin.to_string(),
            collections: vec![Collection {
                address: collection_addr.clone().into_string(),
                config: CollectionConfig {
                    sell_denoms: vec![denom.clone()],
                    royalty_fee_bps: 0,
                    royalty_fee_recipient: royalty_fee_addr.clone().into_string(),
                },
            }],
        };
        let info = MessageInfo {
            sender: deps.api.addr_make("creator"),
            funds: vec![],
        };
        assert_eq!(
            instantiate(deps.as_mut(), env.clone(), info, msg).unwrap_err(),
            ContractError::InvalidDenom { denom }
        );
    }
}

#[test]
fn test_execute_add_or_update_valid_collection() {
    let (mut deps, env) = setup();

    let admin = deps.api.addr_make("admin");
    let royalty_fee_addr = deps.api.addr_make("royalties");
    let collection_addr = deps.api.addr_make("collection");

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    instantiate(
        deps.as_mut(),
        env.clone(),
        MessageInfo {
            sender: admin.clone(),
            funds: vec![],
        },
        InstantiateMsg {
            admin: admin.clone().into_string(),
            collections: vec![],
        },
    )
    .unwrap();

    let res = execute(
        deps.as_mut(),
        env.clone(),
        MessageInfo {
            sender: admin.clone(),
            funds: vec![],
        },
        ExecuteMsg::AddOrUpdateCollection {
            collection_address: collection_addr.to_string(),
            config: CollectionConfig {
                sell_denoms: vec![SELL_DENOM.to_string()],
                royalty_fee_bps: 0,
                royalty_fee_recipient: royalty_fee_addr.clone().into_string(),
            },
        },
    )
    .unwrap();

    assert_eq!(res.attributes[0].key, "action");
    assert_eq!(res.attributes[0].value, "add_or_update_collection");
    assert_eq!(res.attributes[1].key, "collection");
    assert_eq!(res.attributes[1].value, collection_addr.as_str());
    assert_eq!(res.attributes[2].key, "sell_denoms");
    assert_eq!(res.attributes[2].value, SELL_DENOM);
    assert_eq!(res.attributes[3].key, "royalty_fee_recipient");
    assert_eq!(res.attributes[3].value, royalty_fee_addr.as_str());
    assert_eq!(res.attributes[4].key, "royalty_fee_bps");
    assert_eq!(res.attributes[4].value, "0");
}
#[test]
fn test_execute_add_or_update_collection_invalid_collection_addr_err() {
    let (mut deps, env) = setup();
    let admin = deps.api.addr_make("admin");
    let royalty_fee_addr = deps.api.addr_make("royalties");
    let invalid_collection_addr = deps.api.addr_make("invalid");

    // Initialize contract first
    instantiate(
        deps.as_mut(),
        env.clone(),
        MessageInfo {
            sender: admin.clone(),
            funds: vec![],
        },
        InstantiateMsg {
            admin: admin.clone().into_string(),
            collections: vec![],
        },
    )
    .unwrap();

    let info = MessageInfo {
        sender: admin.clone(),
        funds: vec![],
    };
    assert_eq!(
        execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::AddOrUpdateCollection {
                collection_address: invalid_collection_addr.clone().into_string(),
                config: CollectionConfig {
                    sell_denoms: vec![SELL_DENOM.to_string()],
                    royalty_fee_bps: 0,
                    royalty_fee_recipient: royalty_fee_addr.clone().into_string(),
                },
            }
        )
        .unwrap_err(),
        ContractError::NotAContract {
            address: invalid_collection_addr.into_string()
        }
    );
}

#[test]
fn test_execute_add_or_update_collection_invalid_royalty_fee_err() {
    let (mut deps, env) = setup();
    let admin = deps.api.addr_make("admin");
    let royalty_fee_addr = deps.api.addr_make("royalties");
    let collection_addr = deps.api.addr_make("collection");

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    // Initialize contract first
    instantiate(
        deps.as_mut(),
        env.clone(),
        MessageInfo {
            sender: admin.clone(),
            funds: vec![],
        },
        InstantiateMsg {
            admin: admin.clone().into_string(),
            collections: vec![],
        },
    )
    .unwrap();

    let info = MessageInfo {
        sender: admin.clone(),
        funds: vec![],
    };
    assert_eq!(
        execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::AddOrUpdateCollection {
                collection_address: collection_addr.clone().into_string(),
                config: CollectionConfig {
                    sell_denoms: vec![SELL_DENOM.to_string()],
                    royalty_fee_bps: MAX_FEE_BPS + 1,
                    royalty_fee_recipient: royalty_fee_addr.clone().into_string(),
                },
            }
        )
        .unwrap_err(),
        ContractError::InvalidRoyaltyFee {
            max_fee_bps: MAX_FEE_BPS
        }
    );
}

#[test]
fn test_execute_add_or_update_collection_invalid_sell_denoms_err() {
    let (mut deps, env) = setup();
    let admin = deps.api.addr_make("admin");
    let royalty_fee_addr = deps.api.addr_make("royalties");
    let collection_addr = deps.api.addr_make("collection");

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    // Initialize contract first
    instantiate(
        deps.as_mut(),
        env.clone(),
        MessageInfo {
            sender: admin.clone(),
            funds: vec![],
        },
        InstantiateMsg {
            admin: admin.clone().into_string(),
            collections: vec![],
        },
    )
    .unwrap();

    let info = MessageInfo {
        sender: admin.clone(),
        funds: vec![],
    };
    assert_eq!(
        execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::AddOrUpdateCollection {
                collection_address: collection_addr.clone().into_string(),
                config: CollectionConfig {
                    sell_denoms: vec![],
                    royalty_fee_bps: 0,
                    royalty_fee_recipient: royalty_fee_addr.clone().into_string(),
                },
            }
        )
        .unwrap_err(),
        ContractError::EmptySellDenoms {}
    );
}

#[test]
fn test_execute_add_or_update_collection_invalid_royalty_recipient_err() {
    let (mut deps, env) = setup();
    let admin = deps.api.addr_make("admin");
    let collection_addr = deps.api.addr_make("collection");

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    // Initialize contract first
    instantiate(
        deps.as_mut(),
        env.clone(),
        MessageInfo {
            sender: admin.clone(),
            funds: vec![],
        },
        InstantiateMsg {
            admin: admin.clone().into_string(),
            collections: vec![],
        },
    )
    .unwrap();

    let info = MessageInfo {
        sender: admin.clone(),
        funds: vec![],
    };
    assert_eq!(
        execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::AddOrUpdateCollection {
                collection_address: collection_addr.clone().into_string(),
                config: CollectionConfig {
                    sell_denoms: vec![SELL_DENOM.to_string()],
                    royalty_fee_bps: MAX_FEE_BPS,
                    royalty_fee_recipient: "some_invalid_address".to_owned(),
                },
            }
        )
        .unwrap_err(),
        ContractError::Std(deps.api.addr_validate("some_invalid_address").unwrap_err())
    );
}

#[test]
fn test_execute_add_or_update_collection_invalid_denom_err() {
    let (mut deps, env) = setup();
    let admin = deps.api.addr_make("admin");
    let royalty_fee_addr = deps.api.addr_make("royalties");
    let collection_addr = deps.api.addr_make("collection");

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    // Initialize contract first
    instantiate(
        deps.as_mut(),
        env.clone(),
        MessageInfo {
            sender: admin.clone(),
            funds: vec![],
        },
        InstantiateMsg {
            admin: admin.clone().into_string(),
            collections: vec![],
        },
    )
    .unwrap();

    let invalid_denoms = [
        "".to_owned(),                           // empty
        std::iter::repeat_n('c', 129).collect(), // over 128 chars
        "1abcd".to_owned(),                      // non-alphabetic start
        "at*m".to_owned(),                       // invalid character
    ];

    for denom in invalid_denoms {
        let info = MessageInfo {
            sender: admin.clone(),
            funds: vec![],
        };
        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                info,
                ExecuteMsg::AddOrUpdateCollection {
                    collection_address: collection_addr.clone().into_string(),
                    config: CollectionConfig {
                        sell_denoms: vec![denom.clone()],
                        royalty_fee_bps: 0,
                        royalty_fee_recipient: royalty_fee_addr.clone().into_string(),
                    },
                }
            )
            .unwrap_err(),
            ContractError::InvalidDenom { denom }
        );
    }
}

#[test]
fn test_execute_list() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();

    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );
    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    let info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let msg = ExecuteMsg::List {
        collection: collection.clone(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(
        res.attributes
            .iter()
            .find(|a| a.key == "action")
            .map(|a| a.value.as_str()),
        Some("list")
    );

    // Verify the listing was created
    let listing = state::get_listing(deps.as_ref(), collection_addr, token_id.to_string()).unwrap();
    assert_eq!(listing.seller, seller_addr);
    assert_eq!(listing.price, price);
}

#[test]
pub fn test_execute_update_listing() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let collection_addr = deps.api.addr_make("collection");

    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    // First listing
    let info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let msg = ExecuteMsg::List {
        collection: collection_addr.to_string(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // Update listing (same collection / token_id)
    let msg_update_listing = ExecuteMsg::List {
        collection: collection_addr.to_string(),
        token_id: token_id.to_string(),
        price: coin(200, SELL_DENOM),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg_update_listing).unwrap();
    assert_eq!(
        res.attributes
            .iter()
            .find(|a| a.key == "action")
            .map(|a| a.value.as_str()),
        Some("update_listing")
    );

    let listing_binary = query(
        deps.as_ref(),
        env.clone(),
        QueryMsg::Listing {
            collection: collection_addr.to_string(),
            token_id: token_id.to_string(),
        },
    )
    .unwrap();

    let listing: ListingResponse = from_json(listing_binary).unwrap();
    assert_eq!(listing.listing.price, coin(200, SELL_DENOM));
    assert_eq!(listing.listing.seller, seller_addr);
}

#[test]
pub fn test_execute_update_listing_with_new_owner() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let new_owner = "new_owner";
    let new_owner_addr = deps.api.addr_make(new_owner);

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let collection_addr = deps.api.addr_make("collection");

    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    // First owner lists the NFT
    let info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let msg = ExecuteMsg::List {
        collection: collection_addr.to_string(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // Mock NFT contract for new owner, Marketplace still has approval
    mock_nft_contract(
        &mut deps,
        new_owner_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    // Second owner lists NFT
    let new_owner_info = MessageInfo {
        sender: new_owner_addr.clone(),
        funds: vec![],
    };
    let msg_update_listing = ExecuteMsg::List {
        collection: collection_addr.to_string(),
        token_id: token_id.to_string(),
        price: coin(200, SELL_DENOM),
    };

    let res = execute(
        deps.as_mut(),
        env.clone(),
        new_owner_info.clone(),
        msg_update_listing,
    )
    .unwrap();
    assert_eq!(
        res.attributes
            .iter()
            .find(|a| a.key == "action")
            .map(|a| a.value.as_str()),
        Some("update_listing")
    );

    let listing_binary = query(
        deps.as_ref(),
        env.clone(),
        QueryMsg::Listing {
            collection: collection_addr.to_string(),
            token_id: token_id.to_string(),
        },
    )
    .unwrap();

    let listing: ListingResponse = from_json(listing_binary).unwrap();
    assert_eq!(listing.listing.price, coin(200, SELL_DENOM));
    assert_eq!(listing.listing.seller, new_owner_addr);
}

#[test]
pub fn test_execute_update_listing_fail_not_owner() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let unauthorized_addr = deps.api.addr_make("unauthorized");

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let collection_addr = deps.api.addr_make("collection");

    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    // Seller lists a NFT
    let info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let msg = ExecuteMsg::List {
        collection: collection_addr.to_string(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };

    execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Unauthorized seller tries to list the NFT
    let msg_update_listing = ExecuteMsg::List {
        collection: collection_addr.to_string(),
        token_id: token_id.to_string(),
        price: coin(200, SELL_DENOM),
    };
    let info_unauthorized = MessageInfo {
        sender: unauthorized_addr.clone(),
        funds: vec![],
    };

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info_unauthorized.clone(),
        msg_update_listing,
    );
    assert!(res.is_err());
    assert!(res
        .err()
        .unwrap()
        .to_string()
        .contains("NFT not owned by sender"));
}

#[test]
fn test_execute_list_fail_zero_price() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();

    let token_id = "token1";
    let price = coin(0, SELL_DENOM); // Zero price

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );
    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    // Seller is the owner, marketplace is approved
    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    // Unauthorized tries to list the NFT
    let info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let msg = ExecuteMsg::List {
        collection: collection.clone(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };

    let err =
        execute(deps.as_mut(), env, info, msg).expect_err("should fail because price is invalid");
    assert_eq!(err, ContractError::ZeroPrice {});
}

#[test]
fn test_execute_list_fail_not_owner() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let unauthorized_addr = deps.api.addr_make("unauthorized");

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();

    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );
    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    // Seller is the owner, marketplace is approved
    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    // Unauthorized tries to list the NFT
    let info = MessageInfo {
        sender: unauthorized_addr.clone(),
        funds: vec![],
    };
    let msg = ExecuteMsg::List {
        collection: collection.clone(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };

    let err = execute(deps.as_mut(), env, info, msg)
        .expect_err("should fail because seller is not the owner");
    assert!(err.to_string().contains("NFT not owned by sender"));
}

#[test]
fn test_execute_list_fail_marketplace_not_approved() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();

    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(&mut deps, seller_addr.as_str(), None);

    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    let info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let msg = ExecuteMsg::List {
        collection: collection.clone(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };

    let err = execute(deps.as_mut(), env, info, msg)
        .expect_err("should fail because contract address is not approved");
    assert!(err
        .to_string()
        .contains("Marketplace is not allowed to transfer this NFT"));
}

#[test]
fn test_execute_list_fail_collection_not_exists() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();

    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    let info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let msg = ExecuteMsg::List {
        collection: collection.clone(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };

    let err = execute(deps.as_mut(), env, info, msg)
        .expect_err("should fail because collection is not whitelisted");
    assert!(err
        .to_string()
        .contains(&format!("Collection {} is not whitelisted", collection)));
}

#[test]
fn test_collection_management() {
    let (mut deps, env) = setup();

    let collection_addr = deps.api.addr_make("collection");

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );
    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr.clone(),
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    // Verify collection was added
    let saved_config =
        state::get_collection_config(deps.as_ref(), collection_addr.clone()).unwrap();
    assert_eq!(saved_config.sell_denoms, vec![SELL_DENOM.to_string()]);

    let info = MessageInfo {
        sender: admin_addr.clone(),
        funds: vec![],
    };

    // Test updating collection
    let new_config = CollectionConfig {
        sell_denoms: vec!["stake".to_string()],
        royalty_fee_bps: 0,
        royalty_fee_recipient: deps.api.addr_make("hydro").to_string(),
    };
    let msg = ExecuteMsg::AddOrUpdateCollection {
        collection_address: collection_addr.to_string(),
        config: new_config.clone(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
    assert_eq!(res.attributes[0].value, "add_or_update_collection");

    // Verify collection was updated
    let saved_config =
        state::get_collection_config(deps.as_ref(), collection_addr.clone()).unwrap();
    assert_eq!(saved_config.sell_denoms, vec!["stake".to_string()]);

    // Test removing collection
    let msg = ExecuteMsg::RemoveCollection {
        collection: collection_addr.to_string(),
    };
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert_eq!(res.attributes[0].value, "remove_collection");

    // Verify collection was removed
    let query_msg = QueryMsg::Listing {
        collection: collection_addr.to_string(),
        token_id: "token1".to_string(),
    };
    let err = query(deps.as_ref(), env, query_msg).unwrap_err();
    assert!(err.to_string().contains("This token is not for sale"));
}

#[test]
fn test_unauthorized_collection_management() {
    let (mut deps, env) = setup();

    let admin_info = MessageInfo {
        sender: deps.api.addr_make("admin"),
        funds: vec![],
    };
    let msg = InstantiateMsg {
        admin: deps.api.addr_make("admin").to_string(),
        collections: vec![],
    };
    let res = instantiate(deps.as_mut(), env.clone(), admin_info, msg);
    assert!(res.is_ok());

    let collection_addr = deps.api.addr_make("collection");
    let config = CollectionConfig {
        sell_denoms: vec![SELL_DENOM.to_string()],
        royalty_fee_bps: 0,
        royalty_fee_recipient: deps.api.addr_make("hydro").to_string(),
    };

    // Test unauthorized user
    let info = MessageInfo {
        sender: deps.api.addr_make("unauthorized"),
        funds: vec![],
    };
    let msg = ExecuteMsg::AddOrUpdateCollection {
        collection_address: collection_addr.to_string(),
        config: config.clone(),
    };
    let err = execute(deps.as_mut(), env.clone(), info, msg).unwrap_err();
    assert!(err.to_string().contains("Unauthorized"));
}

#[test]
fn test_execute_buy() {
    let (mut deps, env) = setup();
    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);
    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);
    let buyer = "buyer";
    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();
    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    let collection_config =
        instantiate_marketplace_with_collection(&mut deps, &env, admin_addr, &collection_addr, 250);

    // Create a listing first
    let listing_msg = ExecuteMsg::List {
        collection: collection_addr.to_string(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };
    let seller_info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env.clone(), seller_info, listing_msg);
    assert!(res.is_ok(), "Failed to list NFT");

    let info = MessageInfo {
        sender: deps.api.addr_make(buyer),
        funds: vec![Coin::new(100u128, SELL_DENOM)],
    };
    let msg = ExecuteMsg::Buy {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };

    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Verify the listing was removed
    let query_msg = QueryMsg::Listing {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };
    let err = query(deps.as_ref(), env, query_msg).unwrap_err();
    assert!(err.to_string().contains("This token is not for sale"));

    // Verify royalty attributes
    let royalty_amount_attr = res
        .attributes
        .iter()
        .find(|attr| attr.key == "royalty_amount")
        .unwrap();
    assert_eq!(royalty_amount_attr.value, "2"); // 100 * 2.5% = 2.5, rounded down to 2
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "success" && attr.value == "true"));
    // Verify messages
    let messages = res.messages;
    assert_eq!(messages.len(), 3); // NFT transfer + seller payment + royalty payment

    // Verify seller payment (100 - 2 = 98)
    let seller_msg = messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, amount }) = &msg.msg {
            to_address == &deps.api.addr_make(seller).to_string() && amount[0].amount.u128() == 98
        } else {
            false
        }
    });
    assert!(
        seller_msg.is_some(),
        "Seller payment message not found or incorrect amount"
    );

    // Verify royalty payment
    let royalty_msg = messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, amount }) = &msg.msg {
            to_address == &collection_config.royalty_fee_recipient.to_string()
                && amount[0].amount.u128() == 2
        } else {
            false
        }
    });
    assert!(
        royalty_msg.is_some(),
        "Royalty payment message not found or incorrect amount"
    );
}

#[test]
fn test_execute_buy_clean_listing_when_listing_exist_and_marketplace_not_approved() {
    let (mut deps, env) = setup();
    let seller = "seller";
    let buyer = "buyer";
    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);
    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();
    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );
    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    // Create a listing first
    let listing_input = ListingInput {
        collection: collection_addr.clone(),
        token_id: token_id.to_string(),
        seller: deps.api.addr_make(seller),
        price: price.clone(),
    };
    state::create_listing(deps.as_mut(), listing_input).unwrap();
    mock_nft_contract(&mut deps, seller, None);

    let info = MessageInfo {
        sender: deps.api.addr_make(buyer),
        funds: vec![Coin::new(100u128, SELL_DENOM)],
    };
    let msg = ExecuteMsg::Buy {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(
        res.is_ok(),
        "Failed to clean listing when marketplace is not approved"
    );
    let res = res.unwrap();
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "success" && attr.value == "false"));

    // Verify refund message is included
    let refund_msg = res.messages.iter().find(|msg| {
        if let CosmosMsg::Bank(BankMsg::Send { to_address, amount }) = &msg.msg {
            to_address == &info.sender.to_string() && amount == &info.funds
        } else {
            false
        }
    });
    assert!(
        refund_msg.is_some(),
        "Refund message should be present when marketplace is not approved"
    );

    let query_msg = QueryMsg::Listing {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };
    let err = query(deps.as_ref(), env, query_msg).unwrap_err();
    assert!(err.to_string().contains("This token is not for sale"));
}

#[test]
fn test_execute_buy_fail_collection_not_whitelisted() {
    let (mut deps, env) = setup();
    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);
    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);
    let buyer = "buyer";
    let collection = "collection";
    let collection_addr = deps.api.addr_make(collection);

    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_ref(),
        Some(env.contract.address.clone()),
    );
    let _collection_config = instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr.clone(),
        &collection_addr,
        ROYALTY_FEE_BPS,
    );
    // Create a listing first
    let listing_msg = ExecuteMsg::List {
        collection: collection_addr.to_string(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };
    let seller_info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env.clone(), seller_info, listing_msg);
    println!("res: {:?}", res);
    assert!(res.is_ok(), "Failed to list NFT");

    let remove_collection_msg = ExecuteMsg::RemoveCollection {
        collection: collection_addr.to_string(),
    };
    let admin_info = MessageInfo {
        sender: admin_addr.clone(),
        funds: vec![],
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        admin_info,
        remove_collection_msg,
    );
    assert!(res.is_ok(), "Failed to remove collection");

    let info = MessageInfo {
        sender: deps.api.addr_make(buyer),
        funds: vec![Coin::new(100u128, SELL_DENOM)],
    };
    let msg = ExecuteMsg::Buy {
        collection: collection_addr.to_string(),
        token_id: token_id.to_string(),
    };

    let res = execute(deps.as_mut(), env, info, msg)
        .expect_err("should fail because collection is not whitelisted");
    assert!(res.to_string().contains(&format!(
        "Collection {} is not whitelisted",
        collection_addr
    )));
}

#[test]
fn test_execute_buy_fail_denom_not_accepted() {
    let (mut deps, env) = setup();
    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);
    let buyer = "buyer";
    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);
    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();
    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );
    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );
    // Create a listing first
    let listing_msg = ExecuteMsg::List {
        collection: collection_addr.to_string(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };
    let seller_info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env.clone(), seller_info, listing_msg);
    assert!(res.is_ok(), "Failed to list NFT");

    let info = MessageInfo {
        sender: deps.api.addr_make(buyer),
        funds: vec![Coin::new(100u128, "stake")],
    };
    let msg = ExecuteMsg::Buy {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };

    let res =
        execute(deps.as_mut(), env, info, msg).expect_err("should fail because not accepted denom");
    assert!(res.to_string().contains("Payment mismatch"));
}

#[test]
fn test_execute_unlist() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();

    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    let seller_info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };

    let list_msg = ExecuteMsg::List {
        collection: collection.clone(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };
    let res = execute(deps.as_mut(), env.clone(), seller_info.clone(), list_msg);
    assert!(res.is_ok(), "Failed to list NFT");

    let unlist_msg = ExecuteMsg::Unlist {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };
    let res = execute(deps.as_mut(), env.clone(), seller_info, unlist_msg);
    assert!(res.is_ok(), "Failed to unlist the listing");

    // Verify the listing was removed
    let query_msg = QueryMsg::Listing {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };
    let res = query(deps.as_ref(), env, query_msg);
    assert!(res.is_err(), "The listing should be removed");

    let err = res.unwrap_err();
    assert!(err.to_string().contains("This token is not for sale"));
}

#[test]
fn test_execute_permissionless_unlist_marketplace_not_approved_success() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();

    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    // Mock NFT contract where seller is the owner, and the Marketplace is approved
    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    // Create a listing
    let seller_info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let list_msg = ExecuteMsg::List {
        collection: collection_addr.to_string(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };
    let res = execute(deps.as_mut(), env.clone(), seller_info, list_msg);
    assert!(res.is_ok(), "Failed to list NFT");

    // Mock NFT contract where seller is still the owner, but the Marketplace is not approved anymore
    mock_nft_contract(&mut deps, seller_addr.as_str(), None);

    let anybody = "anybody";
    let anybody_addr = deps.api.addr_make(anybody);

    let info = MessageInfo {
        sender: anybody_addr,
        funds: vec![],
    };
    let msg = ExecuteMsg::Unlist {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };

    let res = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(
        res.is_ok(),
        "Unlist should be permissionless when marketplace is not approved"
    );

    // Verify the listing was removed
    let query_msg = QueryMsg::Listing {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };
    let err = query(deps.as_ref(), env, query_msg).unwrap_err();
    assert!(err.to_string().contains("This token is not for sale"));
}

/// Test that anybody can unlist a listing where the seller is not the owner anymore
#[test]
fn test_execute_permissionless_unlist_seller_not_owner_success() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let new_owner = "new_owner";
    let new_owner_addr = deps.api.addr_make(new_owner);

    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();

    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    // Create a listing for "seller"
    let seller_info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };

    let list_msg = ExecuteMsg::List {
        collection: collection_addr.to_string(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };
    let res = execute(deps.as_mut(), env.clone(), seller_info, list_msg);
    assert!(res.is_ok(), "Failed to list NFT");

    let anybody = "anybody";
    let anybody_addr = deps.api.addr_make(anybody);

    // Mock nft contract to simulate that there is a new_owner for the nft but the marketplace is not aware of that
    // We should still be able to unlist, even if the Marketplace is still approved
    // (typically the marketplace would not be approved anymore, as approvals are removed during transfer)
    mock_nft_contract(
        &mut deps,
        new_owner_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    let info = MessageInfo {
        sender: anybody_addr,
        funds: vec![],
    };
    let msg = ExecuteMsg::Unlist {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };

    let res = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(
        res.is_ok(),
        "Unlist should be permissionless when registered seller is not owner"
    );

    // Verify the listing was removed
    let query_msg = QueryMsg::Listing {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };
    let err = query(deps.as_ref(), env, query_msg).unwrap_err();
    assert!(err.to_string().contains("This token is not for sale"));
}

#[test]
fn test_execute_unlist_fail_sender_is_not_owner_and_seller_is_not_owner() {
    let (mut deps, env) = setup();
    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);
    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);
    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();
    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    // Create a listing for "seller"
    let seller_info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };

    let list_msg = ExecuteMsg::List {
        collection: collection_addr.to_string(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };
    let res = execute(deps.as_mut(), env.clone(), seller_info, list_msg);
    assert!(res.is_ok(), "Failed to list NFT");

    let anybody = "anybody";
    let anybody_addr = deps.api.addr_make(anybody);

    let info = MessageInfo {
        sender: anybody_addr,
        funds: vec![],
    };
    let msg = ExecuteMsg::Unlist {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };

    let res = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(res.is_err(), "Should fail because sender is not the owner");
}

#[test]
fn test_execute_buy_insufficient_funds() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let buyer = "buyer";
    let buyer_addr = deps.api.addr_make(buyer);

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();

    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    // Create a listing first
    let listing_input = ListingInput {
        collection: collection_addr.clone(),
        token_id: token_id.to_string(),
        seller: deps.api.addr_make(seller),
        price: price.clone(),
    };
    state::create_listing(deps.as_mut(), listing_input).unwrap();

    let info = MessageInfo {
        sender: buyer_addr,
        funds: vec![Coin::new(50u128, SELL_DENOM)], // Payment mismatch
    };
    let msg = ExecuteMsg::Buy {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };

    let res = execute(deps.as_mut(), env, info, msg);
    println!("{:?}", res);
    assert!(res.is_err(), "The purchase should fail");
    let err = res.unwrap_err();

    assert!(err.to_string().contains("Payment mismatch"));
}

fn instantiate_marketplace_with_collection(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
    env: &Env,
    admin_addr: Addr,
    collection_addr: &Addr,
    royalty_fee_bps: u16,
) -> CollectionConfig {
    let config = CollectionConfig {
        sell_denoms: vec![SELL_DENOM.to_string()],
        royalty_fee_bps,
        royalty_fee_recipient: deps.api.addr_make("hydro").to_string(),
    };
    let admin_info = MessageInfo {
        sender: admin_addr.clone(),
        funds: vec![],
    };
    let instantiate_msg = InstantiateMsg {
        admin: admin_addr.to_string(),
        collections: vec![Collection {
            address: collection_addr.to_string(),
            config: config.clone(),
        }],
    };
    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        instantiate_msg,
    );
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);
    config
}

fn instantiate_marketplace_with_collection_with_multiple_denoms(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
    env: &Env,
    admin_addr: Addr,
    collection_addr: &Addr,
    royalty_fee_bps: u16,
) -> CollectionConfig {
    let config = CollectionConfig {
        sell_denoms: vec![SELL_DENOM.to_string(), "stake".to_string()],
        royalty_fee_bps,
        royalty_fee_recipient: deps.api.addr_make("hydro").to_string(),
    };
    let admin_info = MessageInfo {
        sender: admin_addr.clone(),
        funds: vec![],
    };
    let instantiate_msg = InstantiateMsg {
        admin: admin_addr.to_string(),
        collections: vec![Collection {
            address: collection_addr.to_string(),
            config: config.clone(),
        }],
    };
    mock_nft_contract(deps, admin_addr.as_str(), None);
    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        instantiate_msg,
    );
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);
    config
}

#[test]
fn test_execute_unlist_unauthorized() {
    let (mut deps, env) = setup();
    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);
    let unauthorized = "unauthorized";
    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);
    let collection = "collection";
    let collection_addr = deps.api.addr_make(collection);
    let collection = collection_addr.to_string();
    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    let config = CollectionConfig {
        sell_denoms: vec![SELL_DENOM.to_string()],
        royalty_fee_bps: 0,
        royalty_fee_recipient: deps.api.addr_make("hydro").to_string(),
    };
    let admin_info = MessageInfo {
        sender: admin_addr.clone(),
        funds: vec![],
    };
    let instantiate_msg = InstantiateMsg {
        admin: admin_addr.to_string(),
        collections: vec![Collection {
            address: collection_addr.to_string(),
            config: config.clone(),
        }],
    };
    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        instantiate_msg,
    );
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Create a listing first
    let listing_input = ListingInput {
        collection: collection_addr.clone(),
        token_id: token_id.to_string(),
        seller: deps.api.addr_make(seller),
        price: price.clone(),
    };
    state::create_listing(deps.as_mut(), listing_input).unwrap();

    let unlist_info = MessageInfo {
        sender: deps.api.addr_make(unauthorized),
        funds: vec![],
    };
    let msg = ExecuteMsg::Unlist {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };

    let err = execute(deps.as_mut(), env, unlist_info, msg).unwrap_err();
    assert!(err
        .to_string()
        .contains("Only seller can unlist this listing"));
}

#[test]
fn test_execute_list_multiple_denoms() {
    let (mut deps, env) = setup();
    let seller = "seller";
    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);
    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();
    let token_id = "token1";

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    instantiate_marketplace_with_collection_with_multiple_denoms(
        &mut deps,
        &env.clone(),
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    let seller_addr = deps.api.addr_make(seller);
    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    // Test with atom
    let info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let msg = ExecuteMsg::List {
        collection: collection.clone(),
        token_id: token_id.to_string(),
        price: coin(100, SELL_DENOM),
    };
    execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Test with stake
    let msg = ExecuteMsg::List {
        collection: collection.clone(),
        token_id: "token2".to_string(),
        price: coin(100, "stake"),
    };
    execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Test with invalid denom
    let msg = ExecuteMsg::List {
        collection: collection.clone(),
        token_id: "token3".to_string(),
        price: coin(100, "invalid"),
    };
    let err = execute(deps.as_mut(), env, info, msg).expect_err("should fail with invalid denom");
    assert!(err.to_string().contains("Denom invalid"));
}

#[test]
fn test_query_get_listing() {
    let (mut deps, env) = setup();
    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);
    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);
    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();
    let token_id = "token1";
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );
    instantiate_marketplace_with_collection(
        &mut deps,
        &env,
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    let info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let msg = ExecuteMsg::List {
        collection: collection.clone(),
        token_id: token_id.to_string(),
        price: price.clone(),
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Query the listing
    let query_msg = QueryMsg::Listing {
        collection: collection.clone(),
        token_id: token_id.to_string(),
    };
    let res = query(deps.as_ref(), env, query_msg).unwrap();
    let listing: ListingResponse = from_json(res).unwrap();

    assert_eq!(listing.listing.collection, collection_addr);
    assert_eq!(listing.listing.token_id, token_id);
    assert_eq!(listing.listing.seller, seller_addr);
    assert_eq!(listing.listing.price, price);
}

#[test]
fn test_query_get_listings() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let collection_addr = deps.api.addr_make("collection");

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let collection = collection_addr.to_string();
    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );
    instantiate_marketplace_with_collection(
        &mut deps,
        &env.clone(),
        admin_addr,
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    // List multiple NFTs
    for i in 1..=3 {
        let info = MessageInfo {
            sender: seller_addr.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::List {
            collection: collection.clone(),
            token_id: format!("token{}", i),
            price: price.clone(),
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    // Query all listings
    let query_msg = QueryMsg::Listings {
        start_after: None,
        limit: None,
    };
    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let listings: ListingsResponse = from_json(res).unwrap();

    assert_eq!(listings.listings.len(), 3);
    for (i, listing) in listings.listings.iter().enumerate() {
        assert_eq!(listing.token_id, format!("token{}", i + 1));
    }

    // Test pagination
    let query_msg = QueryMsg::Listings {
        start_after: Some(0),
        limit: Some(1),
    };
    let res = query(deps.as_ref(), env, query_msg).unwrap();
    let listings: ListingsResponse = from_json(res).unwrap();
    assert_eq!(listings.listings.len(), 1);
    assert_eq!(listings.listings[0].token_id, "token2");
}

#[test]
fn test_query_get_whitelisted_collections() {
    let (mut deps, env) = setup();
    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);
    let collection_addr = deps.api.addr_make("collection");
    let collection2_addr = deps.api.addr_make("collection2");
    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );
    instantiate_marketplace_with_collection(
        &mut deps,
        &env.clone(),
        admin_addr.clone(),
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    // Add collections to whitelist
    let config = CollectionConfig {
        sell_denoms: vec![SELL_DENOM.to_string()],
        royalty_fee_bps: 0,
        royalty_fee_recipient: deps.api.addr_make("hydro").to_string(),
    };
    let admin_info = MessageInfo {
        sender: admin_addr.clone(),
        funds: vec![],
    };
    let add_msg2 = ExecuteMsg::AddOrUpdateCollection {
        collection_address: collection2_addr.to_string(),
        config: config.clone(),
    };
    let res = execute(deps.as_mut(), env.clone(), admin_info, add_msg2);
    assert!(
        res.is_ok(),
        "Failed to add collection to whitelist: {:?}",
        res
    );

    // Query all listings
    let query_msg = QueryMsg::WhitelistedCollections {};
    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let collections: WhitelistedCollectionsResponse = from_json(res).unwrap();

    assert_eq!(collections.collections.len(), 2);
    assert_eq!(collections.collections[0].contract_address, collection_addr);
    assert_eq!(
        collections.collections[1].contract_address,
        collection2_addr
    );
}

#[test]
fn test_query_get_listings_by_owner() {
    let (mut deps, env) = setup();

    let seller1 = "seller1";
    let seller2 = "seller2";

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();

    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        collection_addr.as_str(),
        Some(env.contract.address.clone()),
    );
    instantiate_marketplace_with_collection(
        &mut deps,
        &env.clone(),
        admin_addr.clone(),
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    let seller1_addr = deps.api.addr_make(seller1);
    let seller2_addr = deps.api.addr_make(seller2);
    mock_nft_contract(
        &mut deps,
        seller1_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    // List NFTs from seller1
    for i in 1..=2 {
        let info = MessageInfo {
            sender: seller1_addr.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::List {
            collection: collection.clone(),
            token_id: format!("token{}", i),
            price: price.clone(),
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    mock_nft_contract(
        &mut deps,
        seller2_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    // List NFT from seller2
    let info = MessageInfo {
        sender: seller2_addr.clone(),
        funds: vec![],
    };
    let msg = ExecuteMsg::List {
        collection: collection.clone(),
        token_id: "token3".to_string(),
        price: price.clone(),
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Query listings by seller1
    let query_msg = QueryMsg::ListingsByOwner {
        owner: seller1_addr.to_string(),
        start_after: None,
        limit: None,
    };
    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let listings: ListingsByOwnerResponse = from_json(res).unwrap();

    assert_eq!(listings.listings.len(), 2);
    for listing in listings.listings {
        assert_eq!(listing.seller, seller1_addr);
    }

    // Query listings by seller2
    let query_msg = QueryMsg::ListingsByOwner {
        owner: seller2_addr.to_string(),
        start_after: None,
        limit: None,
    };
    let res = query(deps.as_ref(), env, query_msg).unwrap();
    let listings: ListingsByOwnerResponse = from_json(res).unwrap();

    assert_eq!(listings.listings.len(), 1);
    assert_eq!(listings.listings[0].seller, seller2_addr);
}

#[test]
fn test_query_get_listings_by_collection() {
    let (mut deps, env) = setup();

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let collection1_addr = deps.api.addr_make("collection");
    let collection2_addr = deps.api.addr_make("collection2");

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let price = coin(100, SELL_DENOM);

    // Add collections to whitelist
    let config = CollectionConfig {
        sell_denoms: vec![SELL_DENOM.to_string()],
        royalty_fee_bps: 0,
        royalty_fee_recipient: deps.api.addr_make("hydro").to_string(),
    };
    let admin_info = MessageInfo {
        sender: admin_addr.clone(),
        funds: vec![],
    };
    let add_msg2 = ExecuteMsg::AddOrUpdateCollection {
        collection_address: collection2_addr.to_string(),
        config: config.clone(),
    };
    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );
    instantiate_marketplace_with_collection(
        &mut deps,
        &env.clone(),
        admin_addr.clone(),
        &collection1_addr,
        ROYALTY_FEE_BPS,
    );
    execute(deps.as_mut(), env.clone(), admin_info, add_msg2).unwrap();

    // List NFTs from collection1
    for i in 1..=2 {
        let info = MessageInfo {
            sender: seller_addr.clone(),
            funds: vec![],
        };
        let msg = ExecuteMsg::List {
            collection: collection1_addr.to_string(),
            token_id: format!("token{}", i),
            price: price.clone(),
        };
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    // List NFT from collection2
    let info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let msg = ExecuteMsg::List {
        collection: collection2_addr.to_string(),
        token_id: "token3".to_string(),
        price: price.clone(),
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Query listings from collection1
    let query_msg = QueryMsg::ListingsByCollection {
        collection: collection1_addr.to_string(),
        start_after: None,
        limit: None,
    };
    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let listings: ListingsByCollectionResponse = from_json(res).unwrap();

    assert_eq!(listings.listings.len(), 2);
    for listing in listings.listings {
        assert_eq!(listing.collection, collection1_addr);
    }

    // Query listings from collection2
    let query_msg = QueryMsg::ListingsByCollection {
        collection: collection2_addr.to_string(),
        start_after: None,
        limit: None,
    };
    let res = query(deps.as_ref(), env, query_msg).unwrap();
    let listings: ListingsByCollectionResponse = from_json(res).unwrap();

    assert_eq!(listings.listings.len(), 1);
    assert_eq!(listings.listings[0].collection, collection2_addr);
}

#[test]
fn test_change_admin() {
    let (mut deps, env) = setup();
    let old_admin = "old_admin";
    let old_admin_addr = deps.api.addr_make(old_admin);
    let new_admin = "new_admin";
    let new_admin_addr = deps.api.addr_make(new_admin);
    let unauthorized_addr = deps.api.addr_make("unauthorized");
    let collection_addr = deps.api.addr_make("collection");

    mock_nft_contract(
        &mut deps,
        old_admin_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    instantiate_marketplace_with_collection(
        &mut deps,
        &env.clone(),
        old_admin_addr.clone(),
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    let propose_new_admin_msg = ExecuteMsg::ProposeNewAdmin {
        new_admin: Some(new_admin_addr.to_string()),
    };
    let propose_info = MessageInfo {
        sender: old_admin_addr.clone(),
        funds: vec![],
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        propose_info.clone(),
        propose_new_admin_msg.clone(),
    );
    assert!(res.is_ok(), "Old admin should be able to propose new admin");

    let claim_admin_role_msg = ExecuteMsg::ClaimAdminRole {};
    let info = MessageInfo {
        sender: unauthorized_addr.clone(),
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env.clone(), info, claim_admin_role_msg);
    assert!(
        res.is_err(),
        "Unauthorized address should not be able to claim admin role"
    );
    assert_eq!(
        res.unwrap_err(),
        ContractError::NotNewAdmin {
            caller: unauthorized_addr.to_string(),
            new_admin: new_admin_addr.to_string()
        }
    );
    let claim_admin_role_msg = ExecuteMsg::ClaimAdminRole {};
    let info = MessageInfo {
        sender: new_admin_addr.clone(),
        funds: vec![],
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        claim_admin_role_msg.clone(),
    );
    assert!(res.is_ok(), "New admin should be able to claim admin role");

    // New admin should not be able to claim again, as proposed admin should be reset.
    let res = execute(deps.as_mut(), env.clone(), info, claim_admin_role_msg);
    assert!(
        res.is_err(),
        "New admin should not be able to claim admin role again"
    );

    let res = execute(
        deps.as_mut(),
        env.clone(),
        propose_info.clone(),
        propose_new_admin_msg.clone(),
    );
    assert!(
        res.is_err(),
        "Old admin should not be able to propose new admin anymore"
    );

    let propose_new_admin_msg = ExecuteMsg::ProposeNewAdmin { new_admin: None };
    let propose_info = MessageInfo {
        sender: new_admin_addr.clone(),
        funds: vec![],
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        propose_info.clone(),
        propose_new_admin_msg.clone(),
    );
    assert!(
        res.is_ok(),
        "New admin should be able to propose new admin even None"
    );
}

#[test]
fn test_reset_proposed_admin() {
    let (mut deps, env) = setup();

    let old_admin = "old_admin";
    let old_admin_addr = deps.api.addr_make(old_admin);

    let new_admin = "new_admin";
    let new_admin_addr = deps.api.addr_make(new_admin);

    let collection_addr = deps.api.addr_make("collection");

    mock_nft_contract(
        &mut deps,
        old_admin_addr.as_str(),
        Some(env.contract.address.clone()),
    );

    instantiate_marketplace_with_collection(
        &mut deps,
        &env.clone(),
        old_admin_addr.clone(),
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    let propose_new_admin_msg = ExecuteMsg::ProposeNewAdmin {
        new_admin: Some(new_admin_addr.to_string()),
    };
    let propose_info = MessageInfo {
        sender: old_admin_addr.clone(),
        funds: vec![],
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        propose_info.clone(),
        propose_new_admin_msg.clone(),
    );
    assert!(res.is_ok(), "Old admin should be able to propose new admin");

    let reset_new_admin_msg = ExecuteMsg::ProposeNewAdmin { new_admin: None };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        propose_info.clone(),
        reset_new_admin_msg.clone(),
    );
    assert!(
        res.is_ok(),
        "Old admin should be able to reset new admin proposal"
    );

    let claim_admin_role_msg = ExecuteMsg::ClaimAdminRole {};
    let claim_admin_role_info = MessageInfo {
        sender: new_admin_addr.clone(),
        funds: vec![],
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        claim_admin_role_info,
        claim_admin_role_msg,
    );
    assert!(
        res.is_err(),
        "New admin should not be able to claim admin role, as it's not proposed anymore"
    );
    assert_eq!(res.unwrap_err(), ContractError::NoNewAdminProposed {});
}

#[test]
fn test_query_get_events() {
    let (mut deps, env) = setup();

    let collection_addr = deps.api.addr_make("collection");
    let collection = collection_addr.to_string();

    let admin = "admin";
    let admin_addr = deps.api.addr_make(admin);

    let seller = "seller";
    let seller_addr = deps.api.addr_make(seller);

    let buyer = "buyer";
    let buyer_addr = deps.api.addr_make(buyer);

    let price = coin(100, SELL_DENOM);

    mock_nft_contract(
        &mut deps,
        seller_addr.as_str(),
        Some(env.contract.address.clone()),
    );
    instantiate_marketplace_with_collection(
        &mut deps,
        &env.clone(),
        admin_addr.clone(),
        &collection_addr,
        ROYALTY_FEE_BPS,
    );

    let list_msg = ExecuteMsg::List {
        collection: collection.clone(),
        token_id: "token1".to_string(),
        price: price.clone(),
    };
    let list_info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env.clone(), list_info, list_msg);
    assert!(res.is_ok(), "NFT should be added to marketplace");

    let query_msg = QueryMsg::Events {
        collection: collection.clone(),
        token_id: "token1".to_string(),
        start_after: None,
        limit: None,
    };
    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let events: EventsResponse = from_json(res).unwrap();
    assert_eq!(events.events.len(), 1);
    assert_eq!(events.events[0].action, EventAction::List);

    let metadata = events.events[0].metadata.clone();
    assert_eq!(
        metadata.get("price").unwrap().get("amount").unwrap(),
        &price.amount.to_string()
    );
    assert_eq!(
        metadata.get("price").unwrap().get("denom").unwrap(),
        &price.denom
    );
    assert_eq!(metadata.get("seller").unwrap(), &seller_addr.to_string());

    let execute_msg = ExecuteMsg::Unlist {
        collection: collection.clone(),
        token_id: "token1".to_string(),
    };
    let unlist_info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env.clone(), unlist_info, execute_msg);
    assert!(res.is_ok(), "NFT should be unlisted from marketplace");

    let query_msg = QueryMsg::Events {
        collection: collection.clone(),
        token_id: "token1".to_string(),
        start_after: None,
        limit: None,
    };
    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let events: EventsResponse = from_json(res).unwrap();
    assert_eq!(events.events.len(), 2);
    assert_eq!(events.events[0].action, EventAction::Unlist);
    assert_eq!(events.events[1].action, EventAction::List);

    let list_msg = ExecuteMsg::List {
        collection: collection.clone(),
        token_id: "token1".to_string(),
        price: price.clone(),
    };
    let list_info = MessageInfo {
        sender: seller_addr.clone(),
        funds: vec![],
    };
    let res = execute(deps.as_mut(), env.clone(), list_info, list_msg);
    assert!(res.is_ok(), "NFT should be added to marketplace");

    let buy_msg = ExecuteMsg::Buy {
        collection: collection.clone(),
        token_id: "token1".to_string(),
    };
    let buy_info = MessageInfo {
        sender: buyer_addr.clone(),
        funds: vec![price.clone()],
    };
    let res = execute(deps.as_mut(), env.clone(), buy_info, buy_msg);
    assert!(res.is_ok(), "NFT should be bought from marketplace");

    let query_msg = QueryMsg::Events {
        collection: collection.clone(),
        token_id: "token1".to_string(),
        start_after: None,
        limit: None,
    };
    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let events: EventsResponse = from_json(res).unwrap();
    assert_eq!(events.events.len(), 4);
    assert_eq!(events.events[0].action, EventAction::Buy);
    assert_eq!(events.events[1].action, EventAction::List);
    assert_eq!(events.events[2].action, EventAction::Unlist);
    assert_eq!(events.events[3].action, EventAction::List);
}
