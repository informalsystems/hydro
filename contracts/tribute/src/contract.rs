use std::vec;

use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Decimal, Deps, DepsMut, Env,
    MessageInfo, Order, Response, StdError, StdResult, Uint128,
};
use cw2::set_contract_version;
use hydro::msg::LiquidityDeployment;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg};
use crate::query::{
    ConfigResponse, HistoricalTributeClaimsResponse, OutstandingTributeClaimsResponse,
    ProposalTributesResponse, QueryMsg, RoundTributesResponse, TributeClaim,
};
use crate::state::{
    Config, Tribute, CONFIG, ID_TO_TRIBUTE_MAP, TRIBUTE_CLAIMED_LOCKS, TRIBUTE_CLAIMS, TRIBUTE_ID,
    TRIBUTE_MAP,
};
use hydro::query::{
    CurrentRoundResponse, LiquidityDeploymentResponse, ProposalResponse, QueryMsg as HydroQueryMsg,
    UserVotedLocksResponse,
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

    // Get all user's locks that voted for this proposal
    let voted_locks_resp = query_user_voted_locks(
        &deps.as_ref(),
        &config.hydro_contract,
        round_id,
        tranche_id,
        voter.clone().to_string(),
    )?;

    // Find locks that voted for the proposal associated with this tribute
    let (_, prop_locks) = voted_locks_resp
        .voted_locks
        .into_iter()
        .find(|(prop_id, _)| *prop_id == tribute.proposal_id)
        .ok_or_else(|| {
            StdError::generic_err("User didn't vote for the proposal this tribute belongs to")
        })?;

    // Filter out locks that have already claimed this tribute
    let mut unclaimed_locks = Vec::new();
    let mut unclaimed_voting_power = Decimal::zero();

    for lock_info in prop_locks {
        if !TRIBUTE_CLAIMED_LOCKS.has(deps.storage, (tribute_id, lock_info.lock_id)) {
            unclaimed_voting_power += lock_info.power;
            unclaimed_locks.push(lock_info.lock_id);
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
    let percentage_fraction = match user_voting_power
        .checked_div(Decimal::from_ratio(total_proposal_power, Uint128::one()))
    {
        Ok(percentage_fraction) => percentage_fraction,
        Err(_) => {
            return Err(ContractError::Std(StdError::generic_err(
                "Failed to compute users voting power percentage",
            )));
        }
    };
    let amount = match Decimal::from_ratio(tribute_funds.amount, Uint128::one())
        .checked_mul(percentage_fraction)
    {
        Ok(amount) => amount,
        Err(_) => {
            return Err(ContractError::Std(StdError::generic_err(
                "Failed to compute users tribute share",
            )));
        }
    }
    // to_uint_floor() is used so that, due to the precision, contract doesn't transfer by 1 token more
    // to some users, which would leave the last users trying to claim the tribute unable to do so
    // This also implies that some dust amount of tokens could be left on the contract after everyone
    // claiming their portion of the tribute
    .to_uint_floor();
    let sent_coin = Coin {
        denom: tribute_funds.denom,
        amount,
    };
    Ok(sent_coin)
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
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

pub fn query_proposal_tributes(
    deps: Deps,
    round_id: u64,
    proposal_id: u64,
    start_from: u32,
    limit: u32,
) -> StdResult<ProposalTributesResponse> {
    let tributes = TRIBUTE_MAP
        .prefix((round_id, proposal_id))
        .range(deps.storage, None, None, Order::Ascending)
        .map(|l| l.unwrap().1)
        .skip(start_from as usize)
        .take(limit as usize)
        .map(|tribute_id| ID_TO_TRIBUTE_MAP.load(deps.storage, tribute_id).unwrap())
        .collect();

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
    round_id: u64,
    tranche_id: u64,
    address: String,
) -> Result<UserVotedLocksResponse, ContractError> {
    let user_voted_locks: UserVotedLocksResponse = deps.querier.query_wasm_smart(
        hydro_contract,
        &HydroQueryMsg::UserVotedLocks {
            round_id,
            tranche_id,
            address,
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
    Ok(RoundTributesResponse {
        tributes: TRIBUTE_MAP
            .sub_prefix(round_id)
            .range(deps.storage, None, None, Order::Ascending)
            .skip(start_from as usize)
            .take(limit as usize)
            .filter_map(|l| {
                if l.is_err() {
                    // log an error and skip this entry
                    deps.api
                        .debug(format!("Error reading tribute: {:?}", l).as_str());
                    return None;
                }
                let (_, tribute_id) = l.unwrap();
                let tribute = ID_TO_TRIBUTE_MAP.load(deps.storage, tribute_id).unwrap();
                Some(tribute)
            })
            .collect(),
    })
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
        round_id,
        tranche_id,
        address.to_string(),
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

        // Find which locks haven't claimed yet and sum their power
        let mut lock_ids_set = std::collections::HashSet::new();

        for lock_info in &lock_infos {
            lock_ids_set.insert(lock_info.lock_id);
        }

        // get all tributes for this proposal
        let tributes = TRIBUTE_MAP
            .prefix((round_id, proposal_id))
            .range(deps.storage, None, None, Order::Ascending)
            .filter(|l| l.is_ok())
            .filter_map(|l| {
                let (_, tribute_id) = l.unwrap();
                ID_TO_TRIBUTE_MAP
                    .may_load(deps.storage, tribute_id)
                    .unwrap_or(None)
            })
            .collect::<Vec<Tribute>>();

        // For each tribute, compute the claimable amount based on unclaimed locks
        for tribute in tributes {
            // For this tribute, check which locks haven't claimed yet
            let mut tribute_unclaimed_power = Decimal::zero();

            for lock_info in &lock_infos {
                // Check if this lock has already claimed this tribute
                if !TRIBUTE_CLAIMED_LOCKS.has(deps.storage, (tribute.tribute_id, lock_info.lock_id))
                {
                    tribute_unclaimed_power += lock_info.power;
                }
            }

            // Skip if no unclaimed power for this tribute
            if tribute_unclaimed_power == Decimal::zero() {
                continue;
            }

            // Calculate the claim amount
            match calculate_voter_claim_amount(
                tribute.funds.clone(),
                tribute_unclaimed_power,
                proposal.power,
            ) {
                Ok(sent_coin) => {
                    claims.push(TributeClaim {
                        round_id: tribute.round_id,
                        tranche_id: tribute.tranche_id,
                        proposal_id: tribute.proposal_id,
                        tribute_id: tribute.tribute_id,
                        amount: sent_coin,
                    });
                }
                Err(err) => {
                    // log an error and skip this entry
                    deps.api
                        .debug(format!("Error calculating voter claim amount: {:?}", err).as_str());
                }
            }
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
