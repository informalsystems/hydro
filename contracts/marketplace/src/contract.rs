#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    coins, to_json_binary, Addr, BankMsg, Binary, Coin, Decimal, Deps, DepsMut, Env, MessageInfo,
    Response, StdError, Uint128,
};
use cw2::set_contract_version;
use hydro::msg::ExecuteMsg as HydroExecuteMsg;
use hydro::query::{ApprovalResponse, OwnerOfResponse, QueryMsg as HydroQueryMsg};
use neutron_sdk::bindings::msg::NeutronMsg;
use serde_json::json;

use crate::error::ContractError;
use crate::msg::{CollectionConfig, ExecuteMsg, InstantiateMsg};
use crate::query::{
    EventsResponse, ListingResponse, ListingsByCollectionResponse, ListingsByOwnerResponse,
    ListingsResponse, QueryMsg, WhitelistedCollectionsResponse,
};
use crate::state::{self, CollectionConfig as ValidCollectionConfig, EventAction};

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// MAX_FEE_BPS = 100%
pub const MAX_FEE_BPS: u16 = 10000;

/// Validates a collection configuration and returns the validated addresses
fn validate_collection_config(
    deps: Deps,
    collection_address: &str,
    config: CollectionConfig,
) -> Result<ValidCollectionConfig, ContractError> {
    // Validate collection address and check it's a contract
    let contract_address = deps.api.addr_validate(collection_address)?;

    if deps
        .querier
        .query_wasm_contract_info(contract_address.clone())
        .is_err()
    {
        return Err(ContractError::NotAContract {
            address: contract_address.into_string(),
        });
    }

    // Validate royalty fee recipient address
    let royalty_fee_recipient = deps.api.addr_validate(&config.royalty_fee_recipient)?;

    // Validate royalty fee is not more than MAX_FEE_BPS
    if config.royalty_fee_bps > MAX_FEE_BPS {
        return Err(ContractError::InvalidRoyaltyFee {
            max_fee_bps: MAX_FEE_BPS,
        });
    }

    // Validate sell_denoms is not empty
    if config.sell_denoms.is_empty() {
        return Err(ContractError::EmptySellDenoms {});
    }

    // Validate denom formats
    for denom in &config.sell_denoms {
        if denom.is_empty() || denom.len() > 128 {
            return Err(ContractError::InvalidDenom {
                denom: denom.clone(),
            });
        }
        // Basic denom validation - should start with letter and contain only alphanumeric, dash, underscore
        if !denom
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        {
            return Err(ContractError::InvalidDenom {
                denom: denom.clone(),
            });
        }
        if !denom
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/')
        {
            return Err(ContractError::InvalidDenom {
                denom: denom.clone(),
            });
        }
    }

    Ok(ValidCollectionConfig {
        contract_address,
        sell_denoms: config.sell_denoms,
        royalty_fee_bps: config.royalty_fee_bps,
        royalty_fee_recipient,
    })
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let collections_len = msg.collections.len();
    for collection in msg.collections {
        let config =
            validate_collection_config(deps.as_ref(), &collection.address, collection.config)?;
        state::add_or_update_collection_config(deps.storage, &config)?;
    }

    let admin = deps.api.addr_validate(&msg.admin)?;
    state::set_admin(deps.storage, &admin)?;

    // Initialize NEXT_LISTING_ID
    state::set_next_listing_id(deps.storage, 0)?;

    // Initialize NEW_ADMIN_PROPOSAL
    state::set_new_admin_proposal(deps.storage, None)?;

    // Set contract version
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("admin", admin.to_string())
        .add_attribute("collections", collections_len.to_string()))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        ExecuteMsg::List {
            collection,
            token_id,
            price,
        } => execute_list(deps, env, info, collection, token_id, price),
        ExecuteMsg::Buy {
            collection,
            token_id,
        } => execute_buy(deps, env, info, collection, token_id),
        ExecuteMsg::Unlist {
            collection,
            token_id,
        } => execute_unlist(deps, env, info, collection, token_id),
        ExecuteMsg::AddOrUpdateCollection {
            collection_address,
            config,
        } => execute_add_or_update_collection(deps, info, collection_address, config),
        ExecuteMsg::RemoveCollection { collection } => {
            execute_remove_collection(deps, info, collection)
        }
        ExecuteMsg::ProposeNewAdmin { new_admin } => {
            execute_propose_new_admin(deps, info, new_admin)
        }
        ExecuteMsg::ClaimAdminRole {} => execute_claim_admin_role(deps, info),
    }
}

/// Proposes a new admin for the marketplace.
/// Only the current admin can propose a new admin. The new admin must claim the role.
///
/// Setting `new_admin` to `None` removes any existing proposed admin,
/// preventing anyone from claiming admin privileges.
///
/// # Errors
/// Returns `Unauthorized` if sender is not the current admin.
/// Returns an error if the new admin address is invalid.
pub fn execute_propose_new_admin(
    deps: DepsMut,
    info: MessageInfo,
    new_admin: Option<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Check that the sender is admin
    state::assert_admin(deps.as_ref(), &info.sender)?;

    let maybe_new_admin_addr = new_admin
        .as_deref()
        .map(|a| deps.api.addr_validate(a))
        .transpose()?;

    state::set_new_admin_proposal(deps.storage, maybe_new_admin_addr)?;

    Ok(Response::default()
        .add_attribute("action", "propose_new_admin")
        .add_attribute("new_admin", new_admin.as_deref().unwrap_or("None")))
}

/// Claims the admin role for the marketplace.
/// Only the proposed new admin can claim the admin role.
///
/// # Errors
/// Returns `NoNewAdminProposed` if no admin proposal exists.
/// Returns `NotNewAdmin` if sender is not the proposed new admin.
pub fn execute_claim_admin_role(
    deps: DepsMut,
    info: MessageInfo,
) -> Result<Response<NeutronMsg>, ContractError> {
    let new_admin =
        state::get_new_admin_proposal(deps.storage)?.ok_or(ContractError::NoNewAdminProposed {})?;

    if new_admin != info.sender {
        return Err(ContractError::NotNewAdmin {
            caller: info.sender.to_string(),
            new_admin: new_admin.to_string(),
        });
    }

    // Change admin
    state::set_admin(deps.storage, &new_admin)?;

    // Reset new admin proposal
    state::set_new_admin_proposal(deps.storage, None)?;

    Ok(Response::new()
        .add_attribute("action", "claim_admin_role")
        .add_attribute("new_admin", new_admin.to_string()))
}

/// Removes a collection from the marketplace whitelist.
/// Only the admin can remove collections.
///
/// Note: Existing listings from the collection still exist (so they can be
/// immediately reinstated by just adding the collection back),
/// but cannot be purchased while the collection is not whitelisted
///
/// # Errors
/// Returns `Unauthorized` if sender is not the current admin.
/// Returns an error if the collection address is invalid.
pub fn execute_remove_collection(
    deps: DepsMut,
    info: MessageInfo,
    collection: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Check that the sender is admin
    state::assert_admin(deps.as_ref(), &info.sender)?;

    // Check that the collection is a valid address
    let collection_addr = deps.api.addr_validate(&collection)?;
    state::remove_collection_config(deps, collection_addr);

    Ok(Response::new()
        .add_attribute("action", "remove_collection")
        .add_attribute("collection", collection))
}

/// Adds a new collection to the marketplace or updates an existing one.
/// Only the admin can add or update collection configurations.
///
/// # Errors
/// Returns `Unauthorized` if sender is not the current admin.
/// Returns `NotAContract` if collection address is not a contract.
/// Returns `InvalidRoyaltyFee` if royalty fee exceeds maximum.
/// Returns `EmptySellDenoms` if no sell denominations are provided.
/// Returns `InvalidDenom` if any denomination format is invalid.
pub fn execute_add_or_update_collection(
    deps: DepsMut,
    info: MessageInfo,
    collection: String,
    config: CollectionConfig,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Check the sender is admin
    state::assert_admin(deps.as_ref(), &info.sender)?;

    // Check the collection and configuration are valid
    // NOTE: If sell_denoms is modified in the new config, it will only apply for new listings
    let config = validate_collection_config(deps.as_ref(), &collection, config)?;

    state::add_or_update_collection_config(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("action", "add_or_update_collection")
        .add_attribute("collection", collection)
        .add_attribute("sell_denoms", config.sell_denoms.join(","))
        .add_attribute(
            "royalty_fee_recipient",
            config.royalty_fee_recipient.into_string(),
        )
        .add_attribute("royalty_fee_bps", config.royalty_fee_bps.to_string()))
}

/// Lists an NFT for sale in the marketplace.
/// Creates a non-custodial listing where the user retains ownership but approves the marketplace.
///
/// # Errors
/// Returns `CollectionNotWhitelisted` if collection is not whitelisted.
/// Returns `ZeroPrice` if the price amount is zero.
/// Returns `DenomNotAccepted` if the price denomination is not accepted for this collection.
/// Returns `MarketplaceNotAllowedToTransferNft` if marketplace is not approved to transfer the NFT.
/// Returns `NftNotOwnedBySender` if sender does not own the NFT.
pub fn execute_list(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    collection: String,
    token_id: String,
    price: Coin,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Check that the collection is valid and is whitelisted
    let collection_addr = deps.api.addr_validate(&collection)?;
    let collection_config = state::get_collection_config(deps.as_ref(), collection_addr.clone())
        .map_err(|_| ContractError::CollectionNotWhitelisted {
            collection: collection.clone(),
        })?;

    // Validate listing price is not zero
    if price.amount.is_zero() {
        return Err(ContractError::ZeroPrice {});
    }

    // Validate price denomination is accepted
    if !collection_config.sell_denoms.contains(&price.denom) {
        return Err(ContractError::DenomNotAccepted {
            denom: price.denom,
            collection: collection.clone(),
        });
    }

    let marketplace_contract_address = env.contract.address.to_string();
    // Marketplace must be approved to transfer the NFT
    validate_marketplace_approval(
        deps.as_ref(),
        marketplace_contract_address,
        collection.clone(),
        token_id.clone(),
    )?;

    // Only owner can list nft, verify sender is owner
    verify_nft_ownership(
        deps.as_ref(),
        info.sender.clone(),
        collection.clone(),
        token_id.clone(),
    )?;

    let response = Response::new()
        .add_attribute("collection", collection)
        .add_attribute("token_id", token_id.clone())
        .add_attribute("price", price.to_string())
        .add_attribute("seller", info.sender.clone());

    if let Ok(mut listing) =
        state::get_listing(deps.as_ref(), collection_addr.clone(), token_id.clone())
    {
        // Listing already exists, update price and seller
        listing.price = price.clone();
        listing.seller = info.sender;
        state::update_listing(deps, &listing)?;

        return Ok(response
            .add_attribute("action", "update_listing")
            .add_attribute("listing_id", listing.listing_id.to_string()));
    }

    // Listing does not exist, create new listing
    let listing_input = state::ListingInput {
        collection: collection_addr.clone(),
        token_id: token_id.clone(),
        seller: info.sender.clone(),
        price: price.clone(),
    };

    // Record the list event
    state::add_event(
        deps.storage,
        env.block.time,
        collection_addr.clone(),
        token_id.clone(),
        EventAction::List,
        json!({
            "price": price,
            "seller": info.sender.to_string(),
        }),
    )?;

    let listing = state::create_listing(deps, listing_input)?;

    Ok(response
        .add_attribute("action", "list")
        .add_attribute("listing_id", listing.listing_id.to_string()))
}

/// Purchases an NFT from a marketplace listing.
/// Transfers the NFT to the buyer and distributes payment to seller and royalty recipient.
///
/// Note: This method returns a `success` attribute, so the frontend can display an error message
///  even when the function returns Ok (so that changes such as unlisting on an invalid listing persist)
///  - a "false" value in `success` means that the frontend should display an error message
///  - a "true" value in `success` means that the frontend should display a successful message
///
/// # Errors
/// Returns `ListingNotFound` if no listing exists for the NFT.
/// Returns `CollectionNotWhitelisted` if collection is no longer whitelisted.
/// Returns `InsufficientFunds` if buyer did not send enough funds.
pub fn execute_buy(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    collection: String,
    token_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let collection_addr = deps.api.addr_validate(&collection)?;

    // Retrieve the listing
    let listing = state::get_listing(deps.as_ref(), collection_addr.clone(), token_id.clone())?;

    // Check if the collection is whitelisted
    // An error means that the collection was whitelisted at the listing creation but not anymore
    let collection_config = state::get_collection_config(deps.as_ref(), collection_addr.clone())
        .map_err(|_| ContractError::CollectionNotWhitelisted {
            collection: collection.clone(),
        })?;

    // Check that the registered seller is still the owner
    let is_selling_owner = verify_nft_ownership(
        deps.as_ref(),
        listing.seller.clone(),
        collection.clone(),
        token_id.clone(),
    );

    // Check that the Marketplace is still approved to transfer the NFT
    let marketplace_contract_address = env.contract.address.to_string();
    let approval = validate_marketplace_approval(
        deps.as_ref(),
        marketplace_contract_address,
        collection.clone(),
        token_id.clone(),
    );

    // In case the registered seller on the listing is not the owner anymore,
    // or the marketplace is not approved anymore
    // => We need to clean the state by removing listing
    if is_selling_owner.is_err() || approval.is_err() {
        state::remove_listing(
            &mut deps,
            collection_addr.clone(),
            token_id.clone(),
            listing.seller.clone(),
        )?;

        // Refund payment to sender
        let refund_sender_msg = BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: info.funds,
        };

        // Returning an Ok response, so that the listing removal can persist.
        // It is up to the frontend to display the error message.
        return Ok(Response::new()
            .add_message(refund_sender_msg)
            .add_attribute("action", "buy_nft")
            .add_attribute("collection", collection)
            .add_attribute("token_id", token_id)
            .add_attribute("success", "false"));
    }

    // Check sent funds
    match info.funds.as_slice() {
        [coin] if coin == &listing.price => {}
        _ => return Err(ContractError::PaymentMismatch {}),
    }

    // Calculate royalty fee using Decimal with checked operations, royalty fee is rounded down
    let royalty_amount = Decimal::from_ratio(listing.price.amount, Uint128::one())
        .checked_mul(Decimal::bps(collection_config.royalty_fee_bps.into()))
        .expect("royalty calculation should never overflow")
        .to_uint_floor();

    let seller_amount = listing
        .price
        .amount
        .checked_sub(royalty_amount)
        .map_err(|e| ContractError::Std(StdError::overflow(e)))?;

    // Prepare messages
    let transfer_nft = HydroExecuteMsg::TransferNft {
        recipient: info.sender.to_string(),
        token_id: token_id.clone(),
    };

    let nft_transfer_msg = cosmwasm_std::WasmMsg::Execute {
        contract_addr: collection_addr.to_string(),
        msg: to_json_binary(&transfer_nft)?,
        funds: vec![],
    };

    // Send payment to seller (minus royalty fees)
    let pay_seller_msg = BankMsg::Send {
        to_address: listing.seller.to_string(),
        amount: coins(seller_amount.u128(), &listing.price.denom),
    };

    // Send royalty fee to recipient
    let pay_royalty_msg = BankMsg::Send {
        to_address: collection_config.royalty_fee_recipient.to_string(),
        amount: coins(royalty_amount.u128(), &listing.price.denom),
    };

    // Remove the listing in marketplace
    state::remove_listing(
        &mut deps,
        collection_addr.clone(),
        token_id.clone(),
        listing.seller.clone(),
    )?;

    // Record the buy event
    state::add_event(
        deps.storage,
        env.block.time,
        collection_addr.clone(),
        token_id.clone(),
        EventAction::Buy,
        json!({
            "price": listing.clone().price,
            "buyer": info.sender.to_string(),
            "seller": listing.seller.to_string(),
        }),
    )?;

    let res = Response::new()
        .add_message(nft_transfer_msg)
        .add_message(pay_seller_msg)
        .add_message(pay_royalty_msg)
        .add_attribute("action", "buy_nft")
        .add_attribute("collection", collection)
        .add_attribute("token_id", token_id)
        .add_attribute("buyer", info.sender)
        .add_attribute("seller_amount", seller_amount.to_string())
        .add_attribute("royalty_amount", royalty_amount.to_string())
        .add_attribute("success", "true"); // Ensure we always return a "success" attribute for `execute_buy`

    Ok(res)
}

/// Unlists an NFT from the marketplace.
/// If the seller of the listing is still the owner, only the seller can unlist their own listing.
/// If the seller of the listing is not the owner anymore, anyone can unlist the invalid listing.
///
/// # Errors
/// Returns `ListingNotFound` if no listing exists for the NFT.
/// Returns `OnlySellerCanUnlistListing` if sender is not the seller and the seller still owns the NFT.
pub fn execute_unlist(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    collection: String,
    token_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    // We do not check if the collection is whitelisted, as we should allow unlisting even if the collection has been removed
    let collection_addr = deps.api.addr_validate(&collection)?;

    // Retrieve the listing
    let listing = state::get_listing(deps.as_ref(), collection_addr.clone(), token_id.clone())?;

    // Check if the seller on the listing is still the owner
    let is_selling_owner = verify_nft_ownership(
        deps.as_ref(),
        listing.seller.clone(),
        collection.clone(),
        token_id.clone(),
    );

    // Check whether the marketplace is still approved.
    let marketplace_contract_address = env.contract.address.to_string();
    let approval = validate_marketplace_approval(
        deps.as_ref(),
        marketplace_contract_address.clone(),
        collection.clone(),
        token_id.clone(),
    );

    // In case the registered seller on the listing is not the owner anymore,
    // or the marketplace is not approved anymore
    // => We allow anyone to clean the state by removing listing
    //
    // In case the seller on the listing is still the owner
    // and the marketplace is still approved
    // => Only that owner can unlist.
    if is_selling_owner.is_ok() && approval.is_ok() && info.sender != listing.seller {
        return Err(ContractError::OnlySellerCanUnlistListing {});
    }

    state::remove_listing(
        &mut deps,
        collection_addr.clone(),
        token_id.clone(),
        listing.seller.clone(),
    )?;

    // Record the unlist event
    state::add_event(
        deps.storage,
        env.block.time,
        collection_addr.clone(),
        token_id.clone(),
        EventAction::Unlist,
        json!({
            "price": listing.price,
            "seller": listing.seller.to_string(),
        }),
    )?;

    Ok(Response::new()
        .add_attribute("action", "unlist_listing")
        .add_attribute("collection", collection)
        .add_attribute("token_id", token_id)
        .add_attribute("registered_seller", listing.seller)
        .add_attribute("sender", info.sender))
}

fn verify_nft_ownership(
    deps: Deps,
    addr_to_verify: Addr,
    collection: String,
    token_id: String,
) -> Result<(), ContractError> {
    let collection_addr = deps.api.addr_validate(&collection)?;
    let owner: OwnerOfResponse = deps.querier.query_wasm_smart(
        collection_addr,
        &HydroQueryMsg::OwnerOf {
            token_id,
            include_expired: Some(false),
        },
    )?;

    if owner.owner.as_str() != addr_to_verify.as_str() {
        return Err(ContractError::NftNotOwnedBySender {});
    }

    Ok(())
}

// This function queries Hydro contract to check whether the marketplace is approved for the token_id on the collection
// It returns an error if the collection is not a valid address or if it cannot find the Approval
fn validate_marketplace_approval(
    deps: Deps,
    marketplace_contract_address: String,
    collection: String,
    token_id: String,
) -> Result<(), ContractError> {
    let collection_addr = deps.api.addr_validate(&collection)?;

    // Hydro's Approval query returns an error if it cannot find the Approval
    deps.querier
        .query_wasm_smart(
            collection_addr.clone(),
            &HydroQueryMsg::Approval {
                token_id,
                spender: marketplace_contract_address,
                include_expired: Some(false),
            },
        )
        .map(|_: ApprovalResponse| ())
        .map_err(|_| ContractError::MarketplaceNotAllowedToTransferNft {})
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> Result<Binary, ContractError> {
    let binary = match msg {
        QueryMsg::Listing {
            collection,
            token_id,
        } => to_json_binary(&query_listing(deps, collection, token_id)?),
        QueryMsg::Listings { start_after, limit } => {
            to_json_binary(&query_listings(deps, start_after, limit)?)
        }
        QueryMsg::ListingsByOwner {
            owner,
            start_after,
            limit,
        } => to_json_binary(&query_listings_by_owner(deps, owner, start_after, limit)?),
        QueryMsg::ListingsByCollection {
            collection,
            start_after,
            limit,
        } => to_json_binary(&query_listings_by_collection(
            deps,
            collection,
            start_after,
            limit,
        )?),
        QueryMsg::WhitelistedCollections {} => {
            to_json_binary(&query_whitelisted_collections(deps)?)
        }
        QueryMsg::Events {
            collection,
            token_id,
            start_after,
            limit,
        } => to_json_binary(&query_events(
            deps,
            collection,
            token_id,
            start_after,
            limit,
        )?),
    }?;

    Ok(binary)
}

/// Queries marketplace events for a specific NFT.
/// Returns paginated list of events (list, buy, unlist) for the given collection and token.
///
/// # Errors
/// Returns an error if the collection address is invalid.
pub fn query_events(
    deps: Deps,
    collection: String,
    token_id: String,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> Result<EventsResponse, ContractError> {
    let collection_addr = deps.api.addr_validate(&collection)?;

    let events = state::get_events(deps, collection_addr, token_id, start_after, limit)?;

    Ok(EventsResponse { events })
}

/// Queries a specific NFT listing by collection and token ID.
/// Returns the listing details if it exists.
///
/// # Errors
/// Returns an error if the collection address is invalid.
/// Returns `ListingNotFound` if no listing exists for the NFT.
pub fn query_listing(
    deps: Deps,
    collection: String,
    token_id: String,
) -> Result<ListingResponse, ContractError> {
    let collection_addr = deps.api.addr_validate(&collection)?;

    let listing = state::get_listing(deps, collection_addr, token_id)?;

    Ok(ListingResponse { listing })
}

/// Queries all marketplace listings with pagination.
/// Returns a paginated list of all active listings.
///
/// # Errors
/// Returns an error if there are storage access issues.
pub fn query_listings(
    deps: Deps,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> Result<ListingsResponse, ContractError> {
    let listings = state::get_listings(deps, start_after, limit)?;

    Ok(ListingsResponse { listings })
}

/// Queries all listings owned by a specific address.
/// Returns a paginated list of listings for the given owner.
///
/// # Errors
/// Returns an error if the owner address is invalid or storage access fails.
pub fn query_listings_by_owner(
    deps: Deps,
    owner: String,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> Result<ListingsByOwnerResponse, ContractError> {
    let owner_addr = deps.api.addr_validate(&owner)?;

    let listings = state::get_listings_by_owner(deps, owner_addr, start_after, limit)?;

    Ok(ListingsByOwnerResponse { listings })
}

/// Queries all listings for a specific NFT collection.
/// Returns a paginated list of listings for the given collection.
///
/// # Errors
/// Returns an error if the collection address is invalid or storage access fails.
pub fn query_listings_by_collection(
    deps: Deps,
    collection: String,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> Result<ListingsByCollectionResponse, ContractError> {
    let collection_addr = deps.api.addr_validate(&collection)?;

    let listings = state::get_listings_by_collection(deps, collection_addr, start_after, limit)?;

    Ok(ListingsByCollectionResponse { listings })
}

/// Queries all whitelisted collections in the marketplace.
/// Returns the configuration for all collections that can be traded.
///
/// # Errors
/// Returns an error if storage access fails.
pub fn query_whitelisted_collections(
    deps: Deps,
) -> Result<WhitelistedCollectionsResponse, ContractError> {
    let collections: Vec<state::CollectionConfig> = state::get_whitelisted_collections(deps)?;

    Ok(WhitelistedCollectionsResponse { collections })
}
