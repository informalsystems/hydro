use cosmwasm_std::{Addr, Coin, Decimal, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/*
## State
- project_owner: address which is optionally set during initialization. If set, only this address will have permission rights to execute 'create_position' msg.
- principal_funds_owner: address which will receive replenished principal funds alongside pool rewards.
- pool_id: id of the existing cl pool on the Osmosis specified on initialization of the contract.
- position_created_address: address which actually makes contract create the position. This address receives counterparty tokens in case of 'end_round' execution and potential excessive principal amount. Please note that if project_owner is set, the position_created_address and project_owner address will be the same.
- position_id: placeholder for saving the id of the position in the reply of 'create_position' execution
- principal_denom: denom which the contract wants to replenish
- principal_first: contract needs to store the information whether principal token is in the first place in the pool (token0)
- counterparty_denom: 'WOBBLE' denom
- initial_principal_amount: saved after position is created - shows with which principal amount position is created (doesn't get decremented)
- initial_counterparty_amount: similar as initial_principal_amount
- liquidity_shares: current liquidity amount in the position. It is stored right after creating the position and is decremented in case of partial liquidations.
- liquidator_address: helper variable only stored during the liquidation execution so that in the reply funds can be transferred to the liquidator address. After funds are transferred, liquidator address is removed (so that this variable can serve partial liquidations)
- round_end_time: defined during contract initialization - when the time ends - 'end_round' can be executed
- auction_duration: duration of the potential auction set on initialization of the contract. If contract reaches the stage where auction is needed, this parameter will be utilized
- auction_end_time: similarly,if auction is needed, auction_duration will be utilized to define auction_end_time
- auction_principal_deposited: tracks the amount of deposited principal amount during bidding
- principal_to_replenish: on the position creation, this parameter has the same value as initial_principal_amount, with the difference that this parameter is decremeneted as principal amount is being replenished
- counterparty_to_give: determined if auction is needed - this is the amount of counterparty tokens contract can spend to fulfill the auction
- position_rewards: helper field needed for storing the information about how much rewards contract will receive in case of liquidation - so that those funds can be distributed immediatelly to principal_funds_owner
*/

#[derive(Serialize, Deserialize, Debug)]
pub struct State {
    pub project_owner: Option<Addr>,
    pub principal_funds_owner: Addr,
    pub pool_id: u64,
    pub position_created_address: Option<Addr>,
    pub position_id: Option<u64>,
    pub principal_denom: String,
    pub principal_first: bool, // Flag to track in which position is the principal
    pub counterparty_denom: String,
    pub initial_principal_amount: Uint128,
    pub initial_counterparty_amount: Uint128,
    pub liquidity_shares: Option<String>,
    pub liquidator_address: Option<Addr>,
    pub round_end_time: Timestamp,
    pub auction_duration: u64,
    pub auction_end_time: Option<Timestamp>,
    pub auction_principal_deposited: Uint128,
    pub principal_to_replenish: Uint128,
    pub counterparty_to_give: Option<Uint128>,
    pub position_rewards: Option<Vec<Coin>>,
}

pub const STATE: Item<State> = Item::new("state");

pub const SORTED_BIDS: Item<Vec<(Addr, Decimal, Uint128)>> = Item::new("sorted_bids");

/*
## Bids
 - official bids stored when bidder manages to create a bid in the auction period
 - Bid fields: bidder (address), principal_deposited, tokens_requested (counterparty), tokens_fulfilled (counterparty), tokens_refunded(principal) and bid status.
 - Bid status can be: Submitted, Processed and Refunded.
 - since the bids can be partially fulfilled - the contract saves how much tokens are either fully/partially fulfilled, or refunded in case of bid not being one of the "winner" bids.
 - The bid is initally Submitted, but in case bid is not needed afterwards - it will be refunded and the status will be set as Refunded. In case bid is considered as one of the 'winners' bid , it will be processed and the status will be Processed - but note that contract may not need the whole bid.

## Sorted bids
 - helper part of the state used for sorting the bids as they come in.
 - the idea is that only good enough bids are stored in the sorted bids
 - example: if there are several bids, and a new bid comes in which is the best and will replenish the needed amount, all other bids are kicked out
 - the sorted bids are designed to contain only those bids which will be processed on 'resolve auction' action.
*/

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
    pub tokens_fulfilled: Uint128,
    pub tokens_refunded: Uint128,
    pub status: BidStatus,
}

pub const BIDS: Map<Addr, Bid> = Map::new("bids");
