use std::vec;

use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Coins, Decimal, Deps, DepsMut, Env,
    MessageInfo, Order, Response, StdError, StdResult, Uint128,
};
use cw2::set_contract_version;
use hydro::msg::LiquidityDeployment;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg};
use crate::query::{
    ConfigResponse, HistoricalTributeClaimsResponse, OutstandingLockupClaimableCoinsResponse,
    OutstandingTributeClaimsResponse, ProposalTributesResponse, QueryMsg, RoundTributesResponse,
    TributeClaim,
};
use crate::state::{
    Config, Tribute, CONFIG, ID_TO_TRIBUTE_MAP, TRIBUTE_CLAIMED_LOCKS, TRIBUTE_CLAIMS, TRIBUTE_ID,
    TRIBUTE_MAP,
};
use hydro::query::{
    CurrentRoundResponse, LiquidityDeploymentResponse, LockVotesHistoryResponse, ProposalResponse,
    QueryMsg as HydroQueryMsg, UserVotedLocksResponse,
};
use hydro::state::Proposal;

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const DEFAULT_MAX_ENTRIES: usize = 100;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    let config = Config {
        hydro_contract: deps.api.addr_validate(&msg.hydro_contract)?,
    };

    CONFIG.save(deps.storage, &config)?;
    TRIBUTE_ID.save(deps.storage, &0)?;

    Ok(Response::new()
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender.clone()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::AddTribute {
            round_id,
            tranche_id,
            proposal_id,
        } => add_tribute(deps, env, info, round_id, tranche_id, proposal_id),
        ExecuteMsg::ClaimTribute {
            round_id,
            tranche_id,
            tribute_id,
            voter_address,
        } => claim_tribute(deps, info, round_id, tranche_id, tribute_id, voter_address),
        ExecuteMsg::RefundTribute {
            round_id,
            tranche_id,
            proposal_id,
            tribute_id,
        } => refund_tribute(deps, info, round_id, proposal_id, tranche_id, tribute_id),
    }
}

fn add_tribute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<Response, ContractError> {
    let hydro_contract = CONFIG.load(deps.storage)?.hydro_contract;

    // Check that the proposal exists
    query_proposal(&deps, &hydro_contract, round_id, tranche_id, proposal_id)?;

    // Check that the sender has sent funds
    if info.funds.is_empty() {
        return Err(ContractError::Std(StdError::generic_err(
            "Must send funds to add tribute",
        )));
    }

    // Check that the sender has only sent one type of coin for the tribute
    if info.funds.len() != 1 {
        return Err(ContractError::Std(StdError::generic_err(
            "Must send exactly one coin",
        )));
    }

    // Create tribute in TributeMap
    let tribute_id = TRIBUTE_ID.load(deps.storage)?;
    TRIBUTE_ID.save(deps.storage, &(tribute_id + 1))?;
    let tribute = Tribute {
        round_id,
        tranche_id,
        proposal_id,
        tribute_id,
        funds: info.funds[0].clone(),
        depositor: info.sender.clone(),
        refunded: false,
        creation_time: env.block.time,
        creation_round: query_current_round_id(&deps, &hydro_contract)?,
    };
    TRIBUTE_MAP.save(
        deps.storage,
        (round_id, proposal_id, tribute_id),
        &tribute_id,
    )?;
    ID_TO_TRIBUTE_MAP.save(deps.storage, tribute_id, &tribute)?;

    Ok(Response::new()
        .add_attribute("action", "add_tribute")
        .add_attribute("depositor", info.sender.clone())
        .add_attribute("round_id", round_id.to_string())
        .add_attribute("tranche_id", tranche_id.to_string())
        .add_attribute("proposal_id", proposal_id.to_string())
        .add_attribute("tribute_id", tribute_id.to_string())
        .add_attribute("funds", info.funds[0].to_string()))
}

// ClaimTribute(round_id, tranche_id, prop_id, tribute_id, voter_address):
//     Check that the voter has not already claimed the tribute
//     Check that the round is ended
//     Check that there was a deployment entered for the proposal, and that the proposal received a non-zero amount of funds
//     Look up voter's vote for the round
//     Check that the voter voted for the prop
//     Divide voter's vote power by total power voting for the prop to figure out their percentage
//     Use the voter's percentage to send them the right portion of the tribute
//     Mark on the voter's vote that they claimed the tribute
fn claim_tribute(
    deps: DepsMut,
    info: MessageInfo,
    round_id: u64,
    tranche_id: u64,
    tribute_id: u64,
    voter_address: String,
) -> Result<Response, ContractError> {
    let voter = deps.api.addr_validate(&voter_address)?;

    // Check that the round is ended
    let config = CONFIG.load(deps.storage)?;
    let current_round_id = query_current_round_id(&deps, &config.hydro_contract)?;

    if round_id >= current_round_id {
        return Err(ContractError::Std(StdError::generic_err(
            "Round has not ended yet",
        )));
    }

    let tribute = ID_TO_TRIBUTE_MAP.load(deps.storage, tribute_id)?;

    // Get all user's locks that voted for this specific proposal
    let user_voted_locks = query_user_voted_locks(
        &deps.as_ref(),
        &config.hydro_contract,
        voter.clone().to_string(),
        round_id,
        tranche_id,
        Some(tribute.proposal_id),
    )?;

    // Extract the locks for the specific proposal (should only be one entry)
    let prop_locks = user_voted_locks
        .voted_locks
        .into_iter()
        .find(|(prop_id, _)| *prop_id == tribute.proposal_id)
        .map(|(_, locks)| locks)
        .unwrap_or_default();

    // Filter out locks that have already claimed this tribute
    let mut unclaimed_locks = Vec::new();
    let mut unclaimed_voting_power = Decimal::zero();

    for lock_info in prop_locks {
        if !TRIBUTE_CLAIMED_LOCKS.has(deps.storage, (tribute_id, lock_info.lock_id)) {
            unclaimed_voting_power += lock_info.vote_power;

            // During locks split/merge, we are inserting 0-power votes for the past rounds for newly created locks.
            // These locks should not be allowed to claim previous rounds tributes. Furthermore, users could repeat
            // the split/merge actions many times, which could result in some lock being marked here as if it has claimed a
            // specific tribute, and later a user could merge such lock with a new one that voted on a different proposal.
            // If this happens, when the second claim attempt is made, we would mark the same lock as if it has claimed both
            // tributes, which isn't the case.
            if !lock_info.vote_power.is_zero() {
                unclaimed_locks.push(lock_info.lock_id);
            }
        }
    }

    // If there are no unclaimed locks, return an error
    if unclaimed_locks.is_empty() {
        return Err(ContractError::Std(StdError::generic_err(
            "Nothing to claim - all locks have already claimed this tribute",
        )));
    }

    // make sure that tributes for this proposal are claimable
    get_proposal_tributes_info(
        &deps.as_ref(),
        &config,
        round_id,
        tranche_id,
        tribute.proposal_id,
    )?
    .are_tributes_claimable()?;

    let proposal = get_proposal(
        &deps.as_ref(),
        &config,
        round_id,
        tranche_id,
        tribute.proposal_id,
    )?;

    // Calculate claim amount based on unclaimed voting power
    let sent_coin =
        calculate_voter_claim_amount(tribute.funds, unclaimed_voting_power, proposal.power)?;

    // Update TRIBUTE_CLAIMS for this tribute - add to existing amount if present
    let previous_claim = TRIBUTE_CLAIMS.may_load(deps.storage, (voter.clone(), tribute_id))?;
    let updated_claim = match previous_claim {
        Some(previous_coin) => {
            // Make sure the denom matches
            if previous_coin.denom != sent_coin.denom {
                return Err(ContractError::Std(StdError::generic_err(format!(
                    "Mismatched denominations: previous claim was {}, new claim is {}",
                    previous_coin.denom, sent_coin.denom
                ))));
            }

            // Add the new amount to the previous amount
            Coin {
                denom: previous_coin.denom.clone(),
                amount: previous_coin.amount + sent_coin.amount,
            }
        }
        None => sent_coin.clone(),
    };

    // Save the updated claim
    TRIBUTE_CLAIMS.save(deps.storage, (voter.clone(), tribute_id), &updated_claim)?;

    // Mark each unclaimed lock as having claimed this tribute
    for lock_id in unclaimed_locks {
        TRIBUTE_CLAIMED_LOCKS.save(deps.storage, (tribute_id, lock_id), &true)?;
    }

    // Send the tribute to the voter
    Ok(Response::new()
        .add_attribute("action", "claim_tribute")
        .add_attribute("sender", info.sender)
        .add_attribute("round_id", round_id.to_string())
        .add_attribute("tranche_id", tranche_id.to_string())
        .add_attribute("proposal_id", proposal.proposal_id.to_string())
        .add_attribute("tribute_id", tribute_id.to_string())
        .add_attribute("tribute_receiver", voter.clone())
        .add_attribute("tribute_amount", sent_coin.to_string())
        .add_message(BankMsg::Send {
            to_address: voter.to_string(),
            amount: vec![sent_coin],
        }))
}

pub fn calculate_voter_claim_amount(
    tribute_funds: Coin,
    user_voting_power: Decimal,
    total_proposal_power: Uint128,
) -> Result<Coin, ContractError> {
    let amount = Decimal::from_ratio(tribute_funds.amount, Uint128::one())
        .checked_mul(user_voting_power)
        .map_err(|_| StdError::generic_err("Failed to compute numerator for tribute calculation"))?
        .checked_div(Decimal::from_ratio(total_proposal_power, Uint128::one()))
        .map_err(|_| StdError::generic_err("Failed to compute users tribute share"))?
        .to_uint_floor();

    Ok(Coin {
        denom: tribute_funds.denom,
        amount,
    })
}

// RefundTribute(round_id, tranche_id, prop_id, tribute_id):
//     Check that the round is ended
//     Check that the prop lost
//     Check that the sender is the depositor of the tribute
//     Check that the sender has not already refunded the tribute
//     Send the tribute back to the sender
fn refund_tribute(
    deps: DepsMut,
    info: MessageInfo,
    round_id: u64,
    proposal_id: u64,
    tranche_id: u64,
    tribute_id: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Check that the round is ended by checking that the round_id is less than the current round
    let current_round_id = query_current_round_id(&deps, &config.hydro_contract)?;
    if round_id >= current_round_id {
        return Err(ContractError::Std(StdError::generic_err(
            "Round has not ended yet",
        )));
    }

    get_proposal_tributes_info(&deps.as_ref(), &config, round_id, tranche_id, proposal_id)?
        .are_tributes_refundable()?;

    // Load the tribute
    let mut tribute = ID_TO_TRIBUTE_MAP.load(deps.storage, tribute_id)?;

    // Check that the sender is the depositor of the tribute
    if tribute.depositor != info.sender {
        return Err(ContractError::Std(StdError::generic_err(
            "Sender is not the depositor of the tribute",
        )));
    }

    // Check that the sender has not already refunded the tribute
    if tribute.refunded {
        return Err(ContractError::Std(StdError::generic_err(
            "Sender has already refunded the tribute",
        )));
    }

    // Mark the tribute as refunded
    tribute.refunded = true;
    ID_TO_TRIBUTE_MAP.save(deps.storage, tribute_id, &tribute)?;

    // Send the tribute back to the sender
    Ok(Response::new()
        .add_attribute("action", "refund_tribute")
        .add_attribute("sender", info.sender.to_string())
        .add_attribute("round_id", round_id.to_string())
        .add_attribute("tranche_id", tranche_id.to_string())
        .add_attribute("proposal_id", proposal_id.to_string())
        .add_attribute("tribute_id", tribute_id.to_string())
        .add_attribute("refunded_amount", tribute.funds.to_string())
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![tribute.funds],
        }))
}

// Holds information about a proposal: whether the proposal had a liquidity deployment entered,
// and whether that deployment was for a non-zero amount of funds.
struct ProposalTributesInfo {
    pub had_deployment_entered: bool,
    pub received_nonzero_funds: bool,
}

impl ProposalTributesInfo {
    fn are_tributes_claimable(&self) -> Result<(), ContractError> {
        if !self.had_deployment_entered {
            return Err(ContractError::Std(StdError::generic_err(
                "Tribute not claimable: Proposal did not have a liquidity deployment entered",
            )));
        }

        if !self.received_nonzero_funds {
            return Err(ContractError::Std(StdError::generic_err(
                "Tribute not claimable: Proposal did not receive a non-zero liquidity deployment",
            )));
        }

        Ok(())
    }

    fn are_tributes_refundable(&self) -> Result<(), ContractError> {
        if !self.had_deployment_entered {
            return Err(ContractError::Std(StdError::generic_err(
                "Can't refund tribute for proposal that didn't have a liquidity deployment entered",
            )));
        }

        if self.received_nonzero_funds {
            return Err(ContractError::Std(StdError::generic_err(
                "Can't refund tribute for proposal that received a non-zero liquidity deployment",
            )));
        }

        Ok(())
    }
}

// This function will return an info struct that holds information about the proposal.
// The info struct will contain information about whether tributes on this proposal are refundable, claimable, or neither.
fn get_proposal_tributes_info(
    deps: &Deps,
    config: &Config,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<ProposalTributesInfo, ContractError> {
    let mut info = ProposalTributesInfo {
        had_deployment_entered: false,
        received_nonzero_funds: false,
    };

    // get the liquidity deployments for this proposal
    let liquidity_deployment_res =
        get_liquidity_deployment(deps, config, round_id, tranche_id, proposal_id);

    if let Ok(liquidity_deployment) = liquidity_deployment_res {
        info.had_deployment_entered = true;
        info.received_nonzero_funds = liquidity_deployment.has_nonzero_funds();
    }

    Ok(info)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::ProposalTributes {
            round_id,
            proposal_id,
            start_from,
            limit,
        } => to_json_binary(&query_proposal_tributes(
            deps,
            round_id,
            proposal_id,
            start_from,
            limit,
        )?),
        QueryMsg::HistoricalTributeClaims {
            user_address,
            start_from,
            limit,
        } => to_json_binary(&query_historical_tribute_claims(
            &deps,
            user_address,
            start_from,
            limit,
        )?),
        QueryMsg::RoundTributes {
            round_id,
            start_from,
            limit,
        } => to_json_binary(&query_round_tributes(&deps, round_id, start_from, limit)?),
        QueryMsg::OutstandingTributeClaims {
            user_address,
            round_id,
            tranche_id,
        } => to_json_binary(&query_outstanding_tribute_claims(
            &deps,
            user_address,
            round_id,
            tranche_id,
        )?),
        QueryMsg::OutstandingLockupClaimableCoins { lock_id } => {
            to_json_binary(&query_outstanding_lockup_claimable_coins(&deps, lock_id)?)
        }
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

/// Generic helper to process tribute iterators with pagination
fn process_tribute_iterator<K, I>(
    deps: &Deps,
    iterator: I,
    start_from: Option<u32>,
    limit: Option<u32>,
) -> StdResult<Vec<Tribute>>
where
    I: Iterator<Item = StdResult<(K, u64)>>,
    K: std::fmt::Debug,
{
    iterator
        .skip(start_from.unwrap_or(0) as usize)
        .take(limit.unwrap_or(u32::MAX) as usize)
        .filter_map(Result::ok)
        .map(|(_, tribute_id)| ID_TO_TRIBUTE_MAP.load(deps.storage, tribute_id))
        .collect()
}

/// Helper function to retrieve the tributes from a proposal
fn get_proposal_tributes(
    deps: &Deps,
    round_id: u64,
    proposal_id: u64,
    start_from: Option<u32>,
    limit: Option<u32>,
) -> StdResult<Vec<Tribute>> {
    let iterator = TRIBUTE_MAP.prefix((round_id, proposal_id)).range(
        deps.storage,
        None,
        None,
        Order::Ascending,
    );

    process_tribute_iterator(deps, iterator, start_from, limit)
}

/// Helper function to retrieve the tributes from a round
fn get_round_tributes(
    deps: &Deps,
    round_id: u64,
    start_from: Option<u32>,
    limit: Option<u32>,
) -> StdResult<Vec<Tribute>> {
    let iterator =
        TRIBUTE_MAP
            .sub_prefix(round_id)
            .range(deps.storage, None, None, Order::Ascending);

    process_tribute_iterator(deps, iterator, start_from, limit)
}

pub fn query_proposal_tributes(
    deps: Deps,
    round_id: u64,
    proposal_id: u64,
    start_from: u32,
    limit: u32,
) -> StdResult<ProposalTributesResponse> {
    let tributes =
        get_proposal_tributes(&deps, round_id, proposal_id, Some(start_from), Some(limit))?;

    Ok(ProposalTributesResponse { tributes })
}

fn query_current_round_id(deps: &DepsMut, hydro_contract: &Addr) -> Result<u64, ContractError> {
    let current_round_resp: CurrentRoundResponse = deps
        .querier
        .query_wasm_smart(hydro_contract, &HydroQueryMsg::CurrentRound {})?;

    Ok(current_round_resp.round_id)
}

fn query_proposal(
    deps: &DepsMut,
    hydro_contract: &Addr,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<Proposal, ContractError> {
    let proposal_resp: ProposalResponse = deps.querier.query_wasm_smart(
        hydro_contract,
        &HydroQueryMsg::Proposal {
            round_id,
            tranche_id,
            proposal_id,
        },
    )?;

    Ok(proposal_resp.proposal)
}

pub fn query_user_voted_locks(
    deps: &Deps,
    hydro_contract: &Addr,
    user_address: String,
    round_id: u64,
    tranche_id: u64,
    proposal_id: Option<u64>,
) -> Result<UserVotedLocksResponse, ContractError> {
    let user_voted_locks: UserVotedLocksResponse = deps.querier.query_wasm_smart(
        hydro_contract,
        &HydroQueryMsg::UserVotedLocks {
            user_address,
            round_id,
            tranche_id,
            proposal_id,
        },
    )?;

    Ok(user_voted_locks)
}

pub fn query_historical_tribute_claims(
    deps: &Deps,
    address: String,
    start_from: u32,
    limit: u32,
) -> StdResult<HistoricalTributeClaimsResponse> {
    // go through all TRIBUTE_CLAIMS for the address
    let address = deps.api.addr_validate(&address)?;

    Ok(HistoricalTributeClaimsResponse {
        claims: TRIBUTE_CLAIMS
            .prefix(address)
            .range(deps.storage, None, None, Order::Ascending)
            .skip(start_from as usize)
            .take(limit as usize)
            .filter_map(|l| {
                if l.is_err() {
                    // log an error and skip this entry
                    deps.api.debug("Error reading tribute claim");
                    return None;
                }
                let (tribute_id, amount) = l.unwrap();
                let tribute = ID_TO_TRIBUTE_MAP.load(deps.storage, tribute_id).unwrap();
                Some(TributeClaim {
                    round_id: tribute.round_id,
                    tranche_id: tribute.tranche_id,
                    proposal_id: tribute.proposal_id,
                    tribute_id,
                    amount,
                })
            })
            .collect(),
    })
}

pub fn query_round_tributes(
    deps: &Deps,
    round_id: u64,
    start_from: u32,
    limit: u32,
) -> StdResult<RoundTributesResponse> {
    let tributes = get_round_tributes(deps, round_id, Some(start_from), Some(limit))?;
    Ok(RoundTributesResponse { tributes })
}

// This goes through all the tributes for a certain round and tranche,
// then checks whether the given user address can claim them.
// If the user has not claimed the tribute yet, the amount that the user would receive when claiming is
// computed, and the tribute is added to the list of tributes that the user can claim.
pub fn query_outstanding_tribute_claims(
    deps: &Deps,
    address: String,
    round_id: u64,
    tranche_id: u64,
) -> StdResult<OutstandingTributeClaimsResponse> {
    let address = deps.api.addr_validate(&address)?;
    let config = CONFIG.load(deps.storage)?;

    // get user voted locks for this round and tranche
    let user_voted_locks = query_user_voted_locks(
        deps,
        &config.hydro_contract,
        address.to_string(),
        round_id,
        tranche_id,
        None,
    )
    .map_err(|err| StdError::generic_err(format!("Failed to get user voted locks: {}", err)))?;

    let mut claims = vec![];

    // Process each proposal the user voted for
    for (proposal_id, lock_infos) in user_voted_locks.voted_locks {
        // Check if tributes for this proposal are claimable
        if get_proposal_tributes_info(deps, &config, round_id, tranche_id, proposal_id)
            .map_err(|err| StdError::generic_err(format!("Failed to get proposal info: {}", err)))?
            .are_tributes_claimable()
            .is_err()
        {
            continue;
        }

        let proposal = get_proposal(deps, &config, round_id, tranche_id, proposal_id)
            .map_err(|err| StdError::generic_err(format!("Failed to get proposal: {}", err)))?;

        // get all tributes for this proposal
        let tributes = get_proposal_tributes(deps, round_id, proposal_id, None, None)?;

        // For each tribute, compute the claimable amount based on unclaimed locks
        for tribute in tributes {
            // For this tribute, check which locks haven't claimed yet
            let mut tribute_unclaimed_power = Decimal::zero();

            for lock_info in &lock_infos {
                // Check if this lock has already claimed this tribute
                if !TRIBUTE_CLAIMED_LOCKS.has(deps.storage, (tribute.tribute_id, lock_info.lock_id))
                {
                    tribute_unclaimed_power += lock_info.vote_power;
                }
            }

            // Skip if no unclaimed power for this tribute
            if tribute_unclaimed_power == Decimal::zero() {
                continue;
            }

            let Ok(sent_coin) = calculate_voter_claim_amount(
                tribute.funds.clone(),
                tribute_unclaimed_power,
                proposal.power,
            ) else {
                // skip if claim amount calculation fails
                continue;
            };

            claims.push(TributeClaim {
                round_id: tribute.round_id,
                tranche_id: tribute.tranche_id,
                proposal_id: tribute.proposal_id,
                tribute_id: tribute.tribute_id,
                amount: sent_coin,
            });
        }
    }

    Ok(OutstandingTributeClaimsResponse { claims })
}

fn get_proposal(
    deps: &Deps,
    config: &Config,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<Proposal, ContractError> {
    let proposal_resp: ProposalResponse = deps.querier.query_wasm_smart(
        &config.hydro_contract,
        &HydroQueryMsg::Proposal {
            round_id,
            tranche_id,
            proposal_id,
        },
    )?;

    Ok(proposal_resp.proposal)
}

fn get_liquidity_deployment(
    deps: &Deps,
    config: &Config,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<LiquidityDeployment, ContractError> {
    let liquidity_deployment_resp: LiquidityDeploymentResponse = deps
        .querier
        .query_wasm_smart(
            &config.hydro_contract,
            &HydroQueryMsg::LiquidityDeployment {
                round_id,
                tranche_id,
                proposal_id,
            },
        )
        .map_err(|err| {
            StdError::generic_err(format!(
                "No liquidity deployment was entered yet for proposal. Error: {:?}",
                err
            ))
        })?;

    Ok(liquidity_deployment_resp.liquidity_deployment)
}

fn query_lock_votes_history(
    deps: &Deps,
    config: &Config,
    lock_id: u64,
) -> Result<LockVotesHistoryResponse, ContractError> {
    let lock_votes_history = deps.querier.query_wasm_smart(
        &config.hydro_contract,
        &HydroQueryMsg::LockVotesHistory {
            lock_id,
            start_from_round_id: None,
            stop_at_round_id: None,
            tranche_id: None,
        },
    )?;

    Ok(lock_votes_history)
}

/// Query outstanding tribute claims for a specific lock across all rounds and tranches
/// This is more efficient than iterating through all tributes - it first calls LockVotesHistory
/// from Hydro to get the list of (round_id, proposal_id) pairs that this lock voted for,
/// then looks up tributes only for those specific round/proposal combinations.
pub fn query_outstanding_lockup_claimable_coins(
    deps: &Deps,
    lock_id: u64,
) -> StdResult<OutstandingLockupClaimableCoinsResponse> {
    let config = CONFIG.load(deps.storage)?;

    // Step 1: Get the lock's voting history from Hydro contract
    let lock_votes_history = query_lock_votes_history(deps, &config, lock_id).map_err(|err| {
        StdError::generic_err(format!("Failed to get lock votes history: {}", err))
    })?;

    let mut claimable_coins = Coins::default();

    // Step 2: For each vote in the history, check for tributes
    for vote_entry in lock_votes_history.vote_history {
        // Check if tributes for this proposal are claimable
        if get_proposal_tributes_info(
            deps,
            &config,
            vote_entry.round_id,
            vote_entry.tranche_id,
            vote_entry.proposal_id,
        )
        .map_err(|err| StdError::generic_err(format!("Failed to get proposal info: {}", err)))?
        .are_tributes_claimable()
        .is_err()
        {
            continue; // Skip if tributes are not claimable yet
        }

        let proposal = get_proposal(
            deps,
            &config,
            vote_entry.round_id,
            vote_entry.tranche_id,
            vote_entry.proposal_id,
        )
        .map_err(|err| StdError::generic_err(format!("Failed to get proposal: {}", err)))?;

        // get all tributes for this proposal
        let tributes = get_proposal_tributes(
            deps,
            vote_entry.round_id,
            vote_entry.proposal_id,
            None,
            None,
        )?;

        // For each tribute, check if this lock has already claimed it
        for tribute in tributes {
            // Skip if this lock has already claimed this tribute
            if TRIBUTE_CLAIMED_LOCKS.has(deps.storage, (tribute.tribute_id, lock_id)) {
                continue;
            }

            let Ok(claimable_coin) = calculate_voter_claim_amount(
                tribute.funds.clone(),
                vote_entry.vote_power,
                proposal.power,
            ) else {
                // skip if claim amount calculation fails
                continue;
            };

            claimable_coins.add(claimable_coin)?;
        }
    }

    let coins = claimable_coins.into_vec();

    Ok(OutstandingLockupClaimableCoinsResponse { coins })
}
