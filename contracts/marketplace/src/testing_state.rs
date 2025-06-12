use cosmwasm_std::{coin, Addr};
use serde_json::json;

use crate::{
    state::{
        add_event, create_listing, get_events, get_listing, get_listings,
        get_listings_by_collection, get_listings_by_owner, remove_listing, EventAction,
        ListingInput, COLLECTION_INDEX, LISTINGS, OWNER_INDEX, TOKEN_INDEX,
    },
    testing::setup,
};

fn create_test_listing(
    collection: &str,
    token_id: &str,
    seller: &str,
    amount: u128,
    denom: &str,
) -> ListingInput {
    ListingInput {
        collection: Addr::unchecked(collection),
        token_id: token_id.to_string(),
        seller: Addr::unchecked(seller),
        price: coin(amount, denom),
    }
}

#[test]
fn test_create_listing() {
    let (mut deps, _) = setup();
    let listing_input = create_test_listing("collection1", "token1", "seller1", 100, "atom");

    let listing = create_listing(deps.as_mut(), listing_input).unwrap();

    // Verify that the listing has been saved
    let saved_listing = LISTINGS.load(&deps.storage, listing.listing_id).unwrap();
    assert_eq!(saved_listing, listing);

    // Verify that the indexes have been updated
    let token_index = TOKEN_INDEX
        .load(
            &deps.storage,
            (listing.collection.clone(), listing.token_id),
        )
        .unwrap();
    assert_eq!(token_index, listing.listing_id);

    let collection_index_exists = COLLECTION_INDEX.has(
        &deps.storage,
        (listing.collection.clone(), listing.listing_id),
    );
    assert!(collection_index_exists);

    let owner_index_exists =
        OWNER_INDEX.has(&deps.storage, (listing.seller.clone(), listing.listing_id));
    assert!(owner_index_exists);
}

#[test]
fn test_get_listing() {
    let (mut deps, _) = setup();
    let listing_input = create_test_listing("collection1", "token1", "seller1", 100, "atom");

    let listing = create_listing(deps.as_mut(), listing_input).unwrap();

    let retrieved_listing = get_listing(
        deps.as_ref(),
        listing.collection.clone(),
        listing.token_id.clone(),
    )
    .unwrap();

    assert_eq!(retrieved_listing, listing);
}

#[test]
fn test_remove_listing() {
    let (mut deps, _) = setup();
    let listing = create_test_listing("collection1", "token1", "seller1", 100, "atom");

    let listing = create_listing(deps.as_mut(), listing).unwrap();

    remove_listing(
        &mut deps.as_mut(),
        listing.collection.clone(),
        listing.token_id.clone(),
        listing.seller.clone(),
    )
    .unwrap();

    // Verify that the listing has been removed
    let listing_exists = LISTINGS.has(&deps.storage, listing.listing_id);
    assert!(!listing_exists);

    // Verify that the indexes have been removed
    let token_index_exists = TOKEN_INDEX.has(
        &deps.storage,
        (listing.collection.clone(), listing.token_id),
    );
    assert!(!token_index_exists);

    let collection_index_exists = COLLECTION_INDEX.has(
        &deps.storage,
        (listing.collection.clone(), listing.listing_id),
    );
    assert!(!collection_index_exists);

    let owner_index_exists =
        OWNER_INDEX.has(&deps.storage, (listing.seller.clone(), listing.listing_id));
    assert!(!owner_index_exists);
}

#[test]
fn test_get_listings() {
    let (mut deps, _) = setup();

    // Create multiple listings
    let listing1 = create_test_listing("collection1", "token1", "seller1", 100, "atom");
    let listing2 = create_test_listing("collection2", "token2", "seller2", 200, "atom");
    let listing3 = create_test_listing("collection3", "token3", "seller3", 300, "atom");

    create_listing(deps.as_mut(), listing1).unwrap();
    create_listing(deps.as_mut(), listing2).unwrap();
    create_listing(deps.as_mut(), listing3).unwrap();

    // Get all listings
    let listings = get_listings(deps.as_ref(), None, None).unwrap();
    assert_eq!(listings.len(), 3);

    // Get listings with pagination
    let paginated_listings = get_listings(deps.as_ref(), Some(0), Some(2)).unwrap();
    assert_eq!(paginated_listings.len(), 2);
}

#[test]
fn test_get_listings_by_owner() {
    let (mut deps, _) = setup();

    // Create multiple listings for different owners
    let seller1 = Addr::unchecked("cosmos1seller1");
    let seller2 = Addr::unchecked("cosmos1seller2");

    let listing1 = create_test_listing("collection1", "token1", seller1.as_ref(), 100, "atom");
    let listing2 = create_test_listing("collection2", "token2", seller1.as_ref(), 200, "atom");
    let listing3 = create_test_listing("collection3", "token3", seller2.as_ref(), 300, "atom");

    create_listing(deps.as_mut(), listing1).unwrap();
    create_listing(deps.as_mut(), listing2).unwrap();
    create_listing(deps.as_mut(), listing3).unwrap();

    // Retrieve listings of owner
    let seller1_listings =
        get_listings_by_owner(deps.as_ref(), seller1.clone(), None, None).unwrap();

    assert_eq!(seller1_listings.len(), 2);
    assert!(seller1_listings.iter().all(|l| l.seller == seller1));
}

#[test]
fn test_get_listings_by_collection() {
    let (mut deps, _) = setup();

    // Create multiple listings for different collections
    let listing1 = create_test_listing("collection1", "token1", "seller1", 100, "atom");
    let listing2 = create_test_listing("collection1", "token2", "seller2", 200, "atom");
    let listing3 = create_test_listing("collection2", "token3", "seller3", 300, "atom");

    create_listing(deps.as_mut(), listing1).unwrap();
    create_listing(deps.as_mut(), listing2).unwrap();
    create_listing(deps.as_mut(), listing3).unwrap();

    // Get listings by collection
    let collection1_listings =
        get_listings_by_collection(deps.as_ref(), Addr::unchecked("collection1"), None, None)
            .unwrap();

    assert_eq!(collection1_listings.len(), 2);
    assert!(collection1_listings
        .iter()
        .all(|l| l.collection == Addr::unchecked("collection1")));
}

#[test]
fn test_get_events() {
    let (mut deps, env) = setup();

    // Create multiple events
    add_event(
        deps.as_mut().storage,
        env.block.time,
        Addr::unchecked("collection1"),
        "token1".to_string(),
        EventAction::List,
        json!({}),
    )
    .unwrap();

    add_event(
        deps.as_mut().storage,
        env.block.time,
        Addr::unchecked("collection1"),
        "token1".to_string(),
        EventAction::Unlist,
        json!({}),
    )
    .unwrap();

    add_event(
        deps.as_mut().storage,
        env.block.time,
        Addr::unchecked("collection1"),
        "token1".to_string(),
        EventAction::Buy,
        json!({}),
    )
    .unwrap();

    // Retrieve the events
    let events = get_events(
        deps.as_ref(),
        Addr::unchecked("collection1"),
        "token1".to_string(),
        None,
        None,
    )
    .unwrap();

    assert_eq!(events.len(), 3);
    assert_eq!(events[0].action, EventAction::Buy);
    assert_eq!(events[1].action, EventAction::Unlist);
    assert_eq!(events[2].action, EventAction::List);

    // Retrieve the events for a token with no event
    let empty_events = get_events(
        deps.as_ref(),
        Addr::unchecked("collection1"),
        "token2".to_string(),
        None,
        None,
    )
    .unwrap();

    assert_eq!(empty_events.len(), 0);

    // Test step-by-step pagination
    // First query: get just the newest event (limit 1)
    let first_batch = get_events(
        deps.as_ref(),
        Addr::unchecked("collection1"),
        "token1".to_string(),
        None,
        Some(1),
    )
    .unwrap();

    assert_eq!(first_batch.len(), 1);
    assert_eq!(first_batch[0].action, EventAction::Buy);

    // Second query: get the next event using start_after with the first event's timestamp
    let second_batch = get_events(
        deps.as_ref(),
        Addr::unchecked("collection1"),
        "token1".to_string(),
        Some(first_batch[0].timestamp_nanos),
        Some(1),
    )
    .unwrap();

    assert_eq!(second_batch.len(), 1);
    assert_eq!(second_batch[0].action, EventAction::Unlist);
}
