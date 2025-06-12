use cosmwasm_schema::{cw_serde, QueryResponses};

use crate::state::{CollectionConfig, Event, Listing};

#[cw_serde]
pub struct ListingResponse {
    pub listing: Listing,
}

#[cw_serde]
pub struct ListingsResponse {
    pub listings: Vec<Listing>,
}

#[cw_serde]
pub struct ListingsByOwnerResponse {
    pub listings: Vec<Listing>,
}

#[cw_serde]
pub struct ListingsByCollectionResponse {
    pub listings: Vec<Listing>,
}

#[cw_serde]
pub struct WhitelistedCollectionsResponse {
    pub collections: Vec<CollectionConfig>,
}

#[cw_serde]
pub struct EventsResponse {
    pub events: Vec<Event>,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ListingResponse)]
    Listing {
        collection: String,
        token_id: String,
    },
    #[returns(ListingsResponse)]
    Listings {
        start_after: Option<u64>,
        limit: Option<u32>,
    },
    #[returns(ListingsByOwnerResponse)]
    ListingsByOwner {
        owner: String,
        start_after: Option<u64>,
        limit: Option<u32>,
    },
    #[returns(ListingsByCollectionResponse)]
    ListingsByCollection {
        collection: String,
        start_after: Option<u64>,
        limit: Option<u32>,
    },
    #[returns(WhitelistedCollectionsResponse)]
    WhitelistedCollections {},
    #[returns(EventsResponse)]
    Events {
        collection: String,
        token_id: String,
        start_after: Option<u64>,
        limit: Option<u32>,
    },
}
