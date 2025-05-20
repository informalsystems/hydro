use cosmwasm_std::{Addr, Coin, Decimal, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize, Debug)]
pub struct State {
    // Address which is optionally set during initialization. If set, only this address will have permission rights to execute 'create_position' msg.
    pub project_owner: Option<Addr>,
    // Address which will receive replenished principal funds alongside pool rewards.
    pub principal_funds_owner: Addr,
    // Id of the existing cl pool on the Osmosis specified on initialization of the contract
    pub pool_id: u64,
    // Address which actually makes contract create the position.
    // This address receives counterparty tokens in case of 'end_round' execution and potential excessive principal amount.
    // Please note that if project_owner is set, the position_created_address and project_owner address will be the same.
    pub position_created_address: Option<Addr>,
    // Placeholder for saving the id of the position in the reply of 'create_position' execution.
    pub position_id: Option<u64>,
    // Denom which the contract wants to replenish.
    pub principal_denom: String,
    // Contract needs to store the information upfront whether principal token is in the first place in the pool (token0).
    pub principal_first: bool,
    // Potentially volatile denom
    pub counterparty_denom: String,
    // Saved after position is created - shows the principal amount position is created with (doesn't get decremented).
    pub initial_principal_amount: Uint128,
    // Saved after position is created - shows the counterparty amount position is created with (doesn't get decremented).
    pub initial_counterparty_amount: Uint128,
    // Current liquidity amount in the position. It is stored right after creating the position and is decremented in case of partial liquidations.
    pub liquidity_shares: Option<String>,
    // Helper variable only stored during the liquidation execution so that in the reply funds can be transferred to the liquidator address.
    // After funds are transferred, liquidator address is removed (so that this variable can serve partial liquidations).
    pub liquidator_address: Option<Addr>,
    // Defined during contract initialization - when the time ends - 'end_round' can be executed.
    pub round_end_time: Timestamp,
    // Duration of the potential auction set on initialization of the contract. If contract reaches the stage where auction is needed, this parameter will be utilized.
    pub auction_duration: u64,
    // Similarly,if auction is needed, auction_duration will be utilized to define auction_end_time.
    pub auction_end_time: Option<Timestamp>,
    // Tracks the amount of deposited principal amount during bidding (auction period).
    pub auction_principal_deposited: Uint128,
    // On the position creation, this parameter has the same value as initial_principal_amount, with the difference that this parameter is decremeneted as principal amount is being replenished.
    pub principal_to_replenish: Uint128,
    // Determined if auction is needed - this is the amount of counterparty tokens contract can spend to fulfill the auction.
    pub counterparty_to_give: Option<Uint128>,
    // Helper field needed for storing the information about how much rewards contract will receive in case of liquidation - so that those funds can be distributed immediatelly to principal_funds_owner.
    pub position_rewards: Option<Vec<Coin>>,
}

pub const STATE: Item<State> = Item::new("state");

// Helper part of the state used for sorting the bids by tokens_requested (counterparty)/ principal_deposited ratio as they come in.
// The bids are sorted in descending order, so that the best bid is always on the top.
// The idea is that only good enough bids are stored in the sorted bids.
// Example: If there are several bids, and a new bid comes in which is the best and will replenish the needed amount, all other bids are kicked out.
// The sorted bids are designed to contain only those bids which will be processed on 'resolve auction' action. (may not necessarily be the case).
pub const SORTED_BIDS: Item<Vec<(Addr, Decimal, Uint128)>> = Item::new("sorted_bids");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum BidStatus {
    Submitted,
    Processed,
    Refunded,
}

#[derive(Serialize, Deserialize, Debug, JsonSchema)]
pub struct Bid {
    pub bidder: Addr,
    pub principal_deposited: Uint128,
    pub tokens_requested: Uint128,
    // Since the bids can be partially fulfilled - the contract saves how much tokens are either fully/partially fulfilled, or refunded in case of bid not being one of the "winner" bids.
    pub tokens_fulfilled: Uint128,
    pub tokens_refunded: Uint128,
    // The bid is initally Submitted, but in case bid is not needed afterwards - it will be refunded and the status will be set as Refunded.
    // In case bid is considered as one of the 'winners' bid , it will be processed and the status will be Processed - but note that contract may not need the whole bid.
    pub status: BidStatus,
}
// The bids stored when bidders manage to create a bid in the auction period
pub const BIDS: Map<Addr, Bid> = Map::new("bids");
