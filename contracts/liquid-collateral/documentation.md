# Contract storage

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

# Execute methods

## Create position with reply
 - based on the passed position arguments, method is sending MsgCreatePosition to the cl module on Osmosis
 - in the reply contract is updating the state with the position information.

## Liquidate position with reply
 - method checks whether principal amount in the position is zero - which needs to be the case in order to allow liqudation
 - the percentage of liquidation amount is calculated based on the principal funds liquidator sent to the contract (can be full or partial)
 - amount is immediatelly transferred to the principal_funds owner (so contract doesn't need to hold anything)
 - MsgWithdrawPosition on Osmosis module is called
 - in the reply the counterparty amount which was pulled from the pool is sent to the liquidator address
 - in case of full liquidation - all rewards are being sent to principal_funds_owner

## End round with reply
 - is only executed if round has ended
 - full position withdraw is executed (MsgWithdrawPosition with all liquidity amount)
 - in the reply:
   - all rewards are sent to principal_funds_owner
   - if there are enough (equal or more) principal amount than needed for replenish:
     - all fetched counterparty amount is sent to position_created_address (or project owner)
     - potential excessive principal amount is sent to position_created_address
     - exact amount needed to be replenished is sent to principal_funds_owner
   - in case there were not enough principal amount for replenish:
     - send whatever principal amount is fetched to principal_funds_owner
     - decrement principal amount needed 
     - update counteparty amount available in the auction
     - start the auction

## End round bid
 - can only be executed if auction is in progress
 - bidder sends desired principal amount to the contract and request some amount of counterparty token
 - bidder must replenish at least 1% of principal
 - bidder cannot request more counterparty than contract has available
 - bid is being saved in bids and in sorted bids 
 - in case there are already bids that replenishes the whole principal amount - the new bid will need to be better than at least one bid
 - if one or more bids are kicked out from sorted bids - bidders will be refunded and correct status of the bid will be saved

## Resolve auction
 - after auction time elapses this method can be executed by anyone
 - sorted bids are taken and the iteration goes backwards
 - contract is taking as much as principal amount possible and is giving the bidder the counterparty tokens
 - in case some bid is partially fulfilled - the contract is refunding the bidder unused principal amount
 - all replenished principal amount contract is sending to the principal_funds_owner
 - in case all needed principals are replenished - the iteration stops and all unspent counterparty are sent to position_created_address




