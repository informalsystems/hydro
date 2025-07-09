use cosmwasm_std::{Addr, Coin, Deps, DepsMut, StdResult, Storage, Timestamp};
use cw_storage_plus::{Bound, Item, Map};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::ContractError;

// Used for input, before listing_id is known
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ListingInput {
    pub collection: Addr,
    pub token_id: String,
    pub seller: Addr,
    pub price: Coin,
}

// Listing structure
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Listing {
    pub listing_id: u64,
    pub collection: Addr,
    pub token_id: String,
    pub seller: Addr,
    pub price: Coin,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CollectionConfig {
    pub contract_address: Addr,
    pub sell_denoms: Vec<String>,
    pub royalty_fee_bps: u16,
    pub royalty_fee_recipient: Addr,
}

/// The admin who can execute privileged actions
pub const ADMIN: Item<Addr> = Item::new("admin");

/// Next Listing ID counter (starts at 0)
pub const NEXT_LISTING_ID: Item<u64> = Item::new("next_listing_id");

/// Stores all listings
///
/// LISTINGS: key(listing_id) -> Listing
pub const LISTINGS: Map<u64, Listing> = Map::new("listings");

/// Whitelisted collections (Hydro contracts)
///
/// COLLECTION_CONFIGS: key(collection_address) -> CollectionConfig
pub const COLLECTION_CONFIGS: Map<Addr, CollectionConfig> = Map::new("collection_configs");

/// When transferring the admin permissions to a new admin,
/// we store the new admin address (proposal) to this store,
/// and the new admin need to later claim the permissions
pub const NEW_ADMIN_PROPOSAL: Item<Option<Addr>> = Item::new("new_admin_proposal");

// Secondary indexes

/// This allows to retrieve listings by collection:
/// - Check that a collection + listing is listed
/// - List the listings per collection
///
/// COLLECTION_INDEX: key(collection_addr, listing_id) -> ()
pub const COLLECTION_INDEX: Map<(Addr, u64), ()> = Map::new("collection_idx");

// This allows to retrieve listings by owner
// OWNER_INDEX: key(user_address, listing_id) -> ()
pub const OWNER_INDEX: Map<(Addr, u64), ()> = Map::new("owner_idx");

// Stores listing IDs by collection and token ID
// TOKEN_INDEX: key(collection, token_id) -> listing_id
pub const TOKEN_INDEX: Map<(Addr, String), u64> = Map::new("token_idx");

const DEFAULT_QUERY_LIMIT: u32 = 10;
const MAX_QUERY_LIMIT: u32 = 100;

// InstantiateMsg (empty for now)
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {}

pub fn set_next_listing_id(storage: &mut dyn Storage, next_id: u64) -> StdResult<()> {
    NEXT_LISTING_ID.save(storage, &next_id)
}

/// Creates a new listing in the marketplace and updates all indexes.
/// Assigns a unique listing ID and increments the counter.
pub fn create_listing(deps: DepsMut, listing_input: ListingInput) -> StdResult<Listing> {
    // Retrieves the listing_id for this new listing
    let listing_id = NEXT_LISTING_ID.load(deps.storage).unwrap_or_default();

    // Increment listing counter
    set_next_listing_id(deps.storage, listing_id + 1)?;

    // Creates the listing
    let listing = Listing {
        listing_id,
        collection: listing_input.collection,
        token_id: listing_input.token_id,
        seller: listing_input.seller,
        price: listing_input.price,
    };

    // Save the listing to store and indexes
    LISTINGS.save(deps.storage, listing_id, &listing)?;

    COLLECTION_INDEX.save(deps.storage, (listing.collection.clone(), listing_id), &())?;

    TOKEN_INDEX.save(
        deps.storage,
        (listing.collection.clone(), listing.token_id.clone()),
        &listing_id,
    )?;

    OWNER_INDEX.save(deps.storage, (listing.seller.clone(), listing_id), &())?;

    Ok(listing)
}

/// Retrieves all whitelisted collection configurations.
/// Returns a vector of all collections that can be traded in the marketplace.
pub fn get_whitelisted_collections(deps: Deps) -> StdResult<Vec<CollectionConfig>> {
    COLLECTION_CONFIGS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| item.map(|(_, config)| config))
        .collect()
}

/// Updates an existing listing with new information.
/// Used when a seller wants to change the price of their listing.
pub fn update_listing(deps: DepsMut, listing: &Listing) -> StdResult<()> {
    LISTINGS.save(deps.storage, listing.listing_id, listing)
}

/// Removes a listing from storage and updates all indexes.
/// Cleans up all references to the listing across different indexes.
///
/// # Errors
/// Returns an error if the listing is not found.
pub fn remove_listing(
    deps: &mut DepsMut,
    collection: Addr,
    token_id: String,
    seller: Addr,
) -> StdResult<u64> {
    // Retrieve the listing ID
    let listing_id = TOKEN_INDEX.load(deps.storage, (collection.clone(), token_id.clone()))?;

    // Remove from all indexes
    LISTINGS.remove(deps.storage, listing_id);
    COLLECTION_INDEX.remove(deps.storage, (collection.clone(), listing_id));
    TOKEN_INDEX.remove(deps.storage, (collection, token_id.clone()));
    OWNER_INDEX.remove(deps.storage, (seller, listing_id));

    Ok(listing_id)
}

/// Retrieves a specific listing by collection address and token ID.
///
/// # Errors
/// Returns `ListingNotFound` if no listing exists for the given NFT.
pub fn get_listing(
    deps: Deps,
    collection: Addr,
    token_id: String,
) -> Result<Listing, ContractError> {
    // Retrieve the listing ID
    let listing_id = TOKEN_INDEX
        .load(deps.storage, (collection, token_id))
        .map_err(|_| ContractError::ListingNotFound {})?;

    // Retrieve the listing
    LISTINGS
        .load(deps.storage, listing_id)
        .map_err(|_| ContractError::ListingNotFound {})
}

/// Retrieves all marketplace listings with pagination support.
/// Returns listings ordered by listing ID in ascending order.
pub fn get_listings(
    deps: Deps,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<Vec<Listing>> {
    let limit = limit.unwrap_or(DEFAULT_QUERY_LIMIT).min(MAX_QUERY_LIMIT) as usize;

    LISTINGS
        .range(
            deps.storage,
            start_after.map(Bound::exclusive),
            None,
            cosmwasm_std::Order::Ascending,
        )
        .take(limit)
        .map(|item| item.map(|(_, listing)| listing))
        .collect()
}

/// Retrieves all listings owned by a specific address with pagination.
/// Uses the owner index for efficient lookups.
pub fn get_listings_by_owner(
    deps: Deps,
    owner: Addr,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<Vec<Listing>> {
    let limit = limit.unwrap_or(DEFAULT_QUERY_LIMIT).min(MAX_QUERY_LIMIT) as usize;

    // Use OWNER_INDEX with pagination
    OWNER_INDEX
        .prefix(owner)
        .range(
            deps.storage,
            start_after.map(Bound::exclusive),
            None,
            cosmwasm_std::Order::Ascending,
        )
        .take(limit)
        .map(|item| item.and_then(|(id, _)| LISTINGS.load(deps.storage, id)))
        .collect()
}

/// Retrieves all listings for a specific collection with pagination.
/// Uses the collection index for efficient lookups.
pub fn get_listings_by_collection(
    deps: Deps,
    collection_addr: Addr,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<Vec<Listing>> {
    let limit = limit.unwrap_or(DEFAULT_QUERY_LIMIT).min(MAX_QUERY_LIMIT) as usize;

    // Use COLLECTION_INDEX with pagination
    COLLECTION_INDEX
        .prefix(collection_addr.clone())
        .range(
            deps.storage,
            start_after.map(Bound::exclusive),
            None,
            cosmwasm_std::Order::Ascending,
        )
        .take(limit)
        .map(|item| item.and_then(|(id, _)| LISTINGS.load(deps.storage, id)))
        .collect()
}

/// Retrieves the configuration for a specific collection.
/// Returns the trading rules and royalty settings for the collection.
///
/// # Errors
/// Returns an error if the collection is not found.
pub fn get_collection_config(deps: Deps, collection: Addr) -> StdResult<CollectionConfig> {
    COLLECTION_CONFIGS.load(deps.storage, collection)
}

/// Adds or updates a collection configuration in storage.
/// Stores the trading rules and royalty settings for a collection.
pub fn add_or_update_collection_config(
    storage: &mut dyn Storage,
    config: &CollectionConfig,
) -> StdResult<()> {
    COLLECTION_CONFIGS.save(storage, config.contract_address.clone(), config)
}

/// Removes a collection configuration from storage.
/// Effectively removes the collection from the marketplace whitelist.
///
/// No error is thrown if the collection does not exist
pub fn remove_collection_config(deps: DepsMut, collection: Addr) {
    COLLECTION_CONFIGS.remove(deps.storage, collection)
}

/// Verifies that the sender is the current admin.
/// Used to restrict access to admin-only functions.
///
/// # Errors
/// Returns an error if sender is not the admin.
pub fn assert_admin(deps: Deps, sender: &Addr) -> Result<(), ContractError> {
    let admin = ADMIN.load(deps.storage)?;
    if admin != *sender {
        return Err(ContractError::Unauthorized {});
    }
    Ok(())
}

/// Sets the admin address for the marketplace.
/// Updates the stored admin address in contract state.
pub fn set_admin(storage: &mut dyn Storage, admin: &Addr) -> StdResult<()> {
    ADMIN.save(storage, admin)
}

/// Sets the proposal for a new admin
/// Only that address will be able to claim the admin privileges
/// If set to None, no-one will be able to claim the privileges
pub fn set_new_admin_proposal(storage: &mut dyn Storage, new_admin: Option<Addr>) -> StdResult<()> {
    NEW_ADMIN_PROPOSAL.save(storage, &new_admin)
}

// Loads the new admin proposal
pub fn get_new_admin_proposal(storage: &dyn Storage) -> StdResult<Option<Addr>> {
    NEW_ADMIN_PROPOSAL.load(storage)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum EventAction {
    List,
    Buy,
    Unlist,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Event {
    pub timestamp_nanos: u64,
    pub action: EventAction,
    pub metadata: serde_json::Value,
}

// Store events for each NFT (collection + token_id + timestamp)
// EVENTS: key(collection, token_id, timestamp_nanos) -> Event
pub const EVENTS: Map<(Addr, String, u64), Event> = Map::new("events");

/// Adds an event to an NFT's transaction history.
/// Records marketplace actions (list, buy, unlist) with metadata and timestamp.
pub fn add_event(
    storage: &mut dyn Storage,
    block_time: Timestamp,
    collection: Addr,
    token_id: String,
    action: EventAction,
    metadata: serde_json::Value,
) -> StdResult<()> {
    let mut timestamp_nanos = block_time.nanos();

    // Find a unique timestamp by incrementing if key already exists
    while EVENTS.has(
        storage,
        (collection.clone(), token_id.clone(), timestamp_nanos),
    ) {
        timestamp_nanos += 1;
    }

    let event = Event {
        timestamp_nanos,
        action,
        metadata,
    };

    EVENTS.save(storage, (collection, token_id, timestamp_nanos), &event)
}

/// Get events for a specific NFT with pagination
///
/// Returns events in descending order (most recent first).
///
/// # Pagination
/// For pagination, use `start_after` with the timestamp of the last event you received.
/// This will return events older than that timestamp. For example:
/// - First query: `start_after: None, limit: 10` -> returns 10 most recent events
/// - Next query: `start_after: Some(events[9].timestamp_nanos), limit: 10` -> returns next 10 older events
pub fn get_events(
    deps: Deps,
    collection: Addr,
    token_id: String,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<Vec<Event>> {
    let limit = limit.unwrap_or(DEFAULT_QUERY_LIMIT).min(MAX_QUERY_LIMIT) as usize;

    // Use prefix to get all events for this collection + token_id combination
    EVENTS
        .prefix((collection, token_id))
        .range(
            deps.storage,
            None,
            start_after.map(Bound::exclusive), // need to be in max and not min, as descending order
            cosmwasm_std::Order::Descending,   // Most recent events first
        )
        .take(limit)
        .map(|item| item.map(|(_, event)| event))
        .collect()
}
