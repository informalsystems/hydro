use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error(transparent)]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Address {address} is not a contract")]
    NotAContract { address: String },

    #[error("Listing already exists")]
    ListingAlreadyExists {},

    #[error("Collection {collection} is not whitelisted")]
    CollectionNotWhitelisted { collection: String },

    #[error("Denom {denom} not accepted by {collection}")]
    DenomNotAccepted { denom: String, collection: String },

    #[error("Payment mismatch")]
    PaymentMismatch {},

    #[error("Only seller can unlist this listing")]
    OnlySellerCanUnlistListing {},

    #[error("Revoke the Marketplace Approval before unlisting this listing")]
    RevokeApprovalBeforeUnlist {},

    #[error("Only seller can update price")]
    OnlySellerCanUpdatePrice {},

    #[error("NFT not owned by sender")]
    NftNotOwnedBySender {},

    #[error("No new admin proposed")]
    NoNewAdminProposed {},

    #[error("{} is not the new admin (should be {})", caller, new_admin)]
    NotNewAdmin { caller: String, new_admin: String },

    #[error("This token is not for sale")]
    ListingNotFound {},

    #[error("Marketplace is not allowed to transfer this NFT")]
    MarketplaceNotAllowedToTransferNft {},

    #[error("Royalty fee must be between 0 and {} bps", max_fee_bps)]
    InvalidRoyaltyFee { max_fee_bps: u16 },

    #[error("sell_denoms cannot be empty")]
    EmptySellDenoms {},

    #[error("Invalid denomination: {denom}")]
    InvalidDenom { denom: String },

    #[error("Price cannot be zero")]
    ZeroPrice {},
}
