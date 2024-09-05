use std::vec;

use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Decimal, Deps, DepsMut, Env, IbcMsg,
    IbcTimeout, MessageInfo, Order, Reply, Response, StdError, StdResult, Uint128,
};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg};
use crate::query::{ConfigResponse, ProposalTributesResponse, QueryMsg, TributeClaim};
use crate::state::{
    Config, Tribute, COMMUNITY_POOL_CLAIMS, CONFIG, ID_TO_TRIBUTE_MAP, TRIBUTE_CLAIMS, TRIBUTE_ID,
    TRIBUTE_MAP,
};
use hydro::query::{
    CurrentRoundResponse, ProposalResponse, QueryMsg as HydroQueryMsg, TopNProposalsResponse,
    UserVoteResponse,
};
use hydro::state::{Proposal, VoteWithPower};

/// Contract name that is used for migration.
const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const DEFAULT_MAX_ENTRIES: usize = 100;

// Use 1 week as IBC timeout by default
pub const IBC_TIMEOUT_DURATION_IN_SECONDS: u64 = 60 * 60 * 24 * 7;

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
        top_n_props_count: msg.top_n_props_count,
        community_pool_config: msg.community_pool_config,
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
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::AddTribute {
            tranche_id,
            proposal_id,
        } => add_tribute(deps, info, tranche_id, proposal_id),
        ExecuteMsg::ClaimTribute {
            round_id,
            tranche_id,
            tribute_id,
            voter_address,
        } => claim_tribute(deps, round_id, tranche_id, tribute_id, voter_address),
        ExecuteMsg::RefundTribute {
            round_id,
            tranche_id,
            proposal_id,
            tribute_id,
        } => refund_tribute(deps, info, round_id, proposal_id, tranche_id, tribute_id),
        ExecuteMsg::ClaimCommunityPoolTribute {
            round_id,
            tranche_id,
        } => claim_tribute_for_community_pool(deps, _env, round_id, tranche_id),
    }
}

fn add_tribute(
    deps: DepsMut,
    info: MessageInfo,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<Response, ContractError> {
    let hydro_contract = CONFIG.load(deps.storage)?.hydro_contract;
    let current_round_id = query_current_round_id(&deps, &hydro_contract)?;

    // Check that the proposal exists
    query_proposal(
        &deps,
        &hydro_contract,
        current_round_id,
        tranche_id,
        proposal_id,
    )?;

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
        round_id: current_round_id,
        tranche_id,
        proposal_id,
        tribute_id,
        funds: info.funds[0].clone(),
        depositor: info.sender.clone(),
        refunded: false,
    };
    TRIBUTE_MAP.save(
        deps.storage,
        ((current_round_id, tranche_id), proposal_id, tribute_id),
        &tribute,
    )?;
    ID_TO_TRIBUTE_MAP.save(deps.storage, tribute_id, &tribute)?;

    Ok(Response::new()
        .add_attribute("action", "add_tribute")
        .add_attribute("depositor", info.sender.clone())
        .add_attribute("round_id", current_round_id.to_string())
        .add_attribute("tranche_id", tranche_id.to_string())
        .add_attribute("proposal_id", proposal_id.to_string())
        .add_attribute("funds", info.funds[0].to_string()))
}

// ClaimTribute(round_id, tranche_id, prop_id, tribute_id, voter_address):
//     Check that the voter has not already claimed the tribute
//     Check that the round is ended
//     Check that the prop was among the top N proposals for this tranche/round
//     Look up voter's vote for the round
//     Check that the voter voted for the prop
//     Divide voter's vote power by total power voting for the prop to figure out their percentage
//     Use the voter's percentage to send them the right portion of the tribute
//     Mark on the voter's vote that they claimed the tribute
fn claim_tribute(
    deps: DepsMut,
    round_id: u64,
    tranche_id: u64,
    tribute_id: u64,
    voter_address: String,
) -> Result<Response, ContractError> {
    let voter = deps.api.addr_validate(&voter_address)?;

    // Check that the voter has not already claimed the tribute using the TRIBUTE_CLAIMS map
    let claim = TRIBUTE_CLAIMS.may_load(deps.storage, (voter.clone(), tribute_id))?;
    if claim.is_some() && claim.unwrap().0 {
        return Err(ContractError::Std(StdError::generic_err(
            "User has already claimed the tribute",
        )));
    }

    // Check that the round is ended
    let config = CONFIG.load(deps.storage)?;
    let current_round_id = query_current_round_id(&deps, &config.hydro_contract)?;

    if round_id >= current_round_id {
        return Err(ContractError::Std(StdError::generic_err(
            "Round has not ended yet",
        )));
    }

    // Look up voter's vote for the round, error if it cannot be found
    let vote = query_user_vote(
        &deps,
        &config.hydro_contract,
        round_id,
        tranche_id,
        voter.clone().to_string(),
    )?;

    // Check that the voter voted for one of the top N proposals
    let proposal =
        match get_top_n_proposal(&deps.as_ref(), &config, round_id, tranche_id, vote.prop_id)? {
            Some(prop) => prop,
            None => {
                return Err(ContractError::Std(StdError::generic_err(
                    "User voted for proposal outside of top N proposals",
                )))
            }
        };

    // Load the tribute and use the percentage to figure out how much of the tribute to send them
    let tribute = TRIBUTE_MAP.load(
        deps.storage,
        ((round_id, tranche_id), vote.prop_id, tribute_id),
    )?;

    // adjust the tribute to get only the portion that should be distributed
    // to the voters
    let voters_share = get_voters_tribute_share(&config, tribute.clone().funds)?;

    let percentage_fraction = match vote
        .power
        .checked_div(Decimal::from_ratio(proposal.power, Uint128::one()))
    {
        Ok(percentage_fraction) => percentage_fraction,
        Err(_) => {
            return Err(ContractError::Std(StdError::generic_err(
                "Failed to compute users voting power percentage",
            )));
        }
    };

    let amount =
        match Decimal::from_ratio(voters_share, Uint128::one()).checked_mul(percentage_fraction) {
            Ok(amount) => amount,
            Err(_) => {
                return Err(ContractError::Std(StdError::generic_err(
                    "Failed to compute users tribute share",
                )));
            }
        }
        .to_uint_floor();

    let sent_coin = Coin {
        denom: tribute.funds.denom,
        amount,
    };

    // Mark in the TRIBUTE_CLAIMS that the voter has claimed this tribute
    TRIBUTE_CLAIMS.save(
        deps.storage,
        (voter.clone(), tribute_id),
        &(true, sent_coin.clone()),
    )?;

    // Send the tribute to the voter
    Ok(Response::new()
        .add_attribute("action", "claim_tribute")
        .add_message(BankMsg::Send {
            to_address: voter.to_string(),
            amount: vec![sent_coin],
        }))
}

// For each proposal in the top N proposals for the given round and tranche:
// Trigger a send of the community pool tax share to the community pool address
pub fn claim_tribute_for_community_pool(
    deps: DepsMut,
    env: Env,
    round_id: u64,
    tranche_id: u64,
) -> Result<Response, ContractError> {
    // Load the config
    let config = CONFIG.load(deps.storage)?;

    let current_round_id = query_current_round_id(&deps, &config.hydro_contract)?;
    if round_id >= current_round_id {
        return Err(ContractError::Std(StdError::generic_err(
            "Round has not ended yet",
        )));
    }

    // Load the top N proposals
    let proposals_resp: Vec<Proposal> =
        get_top_n_proposals(&deps.as_ref(), &config, round_id, tranche_id)?;

    // For each proposal in the top N proposals, send the community pool tax share to the community pool address
    let mut res = Response::new();

    // count how many tributes were claimed for the community pool to add to the response
    let mut claimed_tributes_count = 0;
    for proposal in proposals_resp {
        // iterate over all tributes for this proposal
        let tributes = TRIBUTE_MAP
            .prefix(((round_id, tranche_id), proposal.proposal_id))
            .range(deps.storage, None, None, Order::Ascending)
            .map(|l| l.unwrap().1)
            .collect::<Vec<Tribute>>();

        for tribute in tributes {
            // continue if this tribute was already claimed for the community pool
            if COMMUNITY_POOL_CLAIMS
                .may_load(deps.storage, tribute.tribute_id)?
                .unwrap_or(false)
            {
                continue;
            }

            // Calculate the community pool share
            let community_pool_share =
                get_community_pool_tribute_share(&config, tribute.clone().funds).unwrap();

            let send_coin = Coin {
                denom: tribute.funds.denom,
                amount: community_pool_share,
            };

            // Send the community pool share to the community pool address
            res = res.add_message(IbcMsg::Transfer {
                channel_id: config.community_pool_config.clone().channel_id,
                to_address: config.community_pool_config.clone().bucket_address,
                amount: send_coin,
                timeout: IbcTimeout::with_timestamp(
                    env.block.time.plus_seconds(IBC_TIMEOUT_DURATION_IN_SECONDS),
                ),
                memo: None,
            });

            // mark the tribute as claimed by the community pool
            COMMUNITY_POOL_CLAIMS.save(deps.storage, tribute.tribute_id, &true)?;
            claimed_tributes_count += 1;
        }
    }
    Ok(res
        .add_attribute("action", "claim_tribute_for_community_pool")
        .add_attribute("round_id", round_id.to_string())
        .add_attribute("tranche_id", tranche_id.to_string())
        .add_attribute("claimed_tributes_count", claimed_tributes_count.to_string()))
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

    if get_top_n_proposal(&deps.as_ref(), &config, round_id, tranche_id, proposal_id)?.is_some() {
        return Err(ContractError::Std(StdError::generic_err(
            "Can't refund top N proposal",
        )));
    }

    // Load the tribute
    let mut tribute = TRIBUTE_MAP.load(
        deps.storage,
        ((round_id, tranche_id), proposal_id, tribute_id),
    )?;

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
    TRIBUTE_MAP.save(
        deps.storage,
        ((round_id, tranche_id), proposal_id, tribute_id),
        &tribute,
    )?;

    // Send the tribute back to the sender
    Ok(Response::new()
        .add_attribute("action", "refund_tribute")
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![tribute.funds],
        }))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::ProposalTributes {
            round_id,
            tranche_id,
            proposal_id,
            start_from,
            limit,
        } => to_json_binary(&query_proposal_tributes(
            deps,
            round_id,
            tranche_id,
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
    tranche_id: u64,
    proposal_id: u64,
    start_from: u32,
    limit: u32,
) -> StdResult<ProposalTributesResponse> {
    let tributes = TRIBUTE_MAP
        .prefix(((round_id, tranche_id), proposal_id))
        .range(deps.storage, None, None, Order::Ascending)
        .map(|l| l.unwrap().1)
        .skip(start_from as usize)
        .take(limit as usize)
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

fn query_user_vote(
    deps: &DepsMut,
    hydro_contract: &Addr,
    round_id: u64,
    tranche_id: u64,
    address: String,
) -> Result<VoteWithPower, ContractError> {
    let user_vote_resp: UserVoteResponse = deps.querier.query_wasm_smart(
        hydro_contract,
        &HydroQueryMsg::UserVote {
            round_id,
            tranche_id,
            address,
        },
    )?;

    let vote = user_vote_resp.vote;

    Ok(vote)
}

pub fn query_historical_tribute_claims(
    deps: &Deps,
    address: String,
    start_from: u32,
    limit: u32,
) -> StdResult<Vec<TributeClaim>> {
    // go through all TRIBUTE_CLAIMS for the address
    let address = deps.api.addr_validate(&address)?;
    Ok(TRIBUTE_CLAIMS
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
            let (tribute_id, (claimed, amount)) = l.unwrap();
            if !claimed {
                deps.api.debug("Unclaimed tribute in the claims database");
                return None;
            }
            let tribute = ID_TO_TRIBUTE_MAP.load(deps.storage, tribute_id).unwrap();
            Some(TributeClaim {
                round_id: tribute.round_id,
                tranche_id: tribute.tranche_id,
                proposal_id: tribute.proposal_id,
                tribute_id,
                amount,
            })
        })
        .collect())
}

fn get_top_n_proposal(
    deps: &Deps,
    config: &Config,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<Option<Proposal>, ContractError> {
    let top_n_proposals = get_top_n_proposals(deps, config, round_id, tranche_id)?;

    for proposal in top_n_proposals {
        if proposal.proposal_id == proposal_id {
            return Ok(Some(proposal));
        }
    }

    Ok(None)
}

fn get_top_n_proposals(
    deps: &Deps,
    config: &Config,
    round_id: u64,
    tranche_id: u64,
) -> Result<Vec<Proposal>, ContractError> {
    let proposals_resp: TopNProposalsResponse = deps.querier.query_wasm_smart(
        &config.hydro_contract,
        &HydroQueryMsg::TopNProposals {
            round_id,
            tranche_id,
            number_of_proposals: config.top_n_props_count as usize,
        },
    )?;

    Ok(proposals_resp.proposals)
}

// Given a funds amount, calculate how much of it should go to the community pool
pub fn get_community_pool_tribute_share(
    config: &Config,
    funds: Coin,
) -> Result<Uint128, ContractError> {
    // Calculate the community pool share
    let community_pool_share = Decimal::from_ratio(funds.amount, Uint128::one())
        .checked_mul(config.community_pool_config.tax_percent)
        .map_err(|_| {
            ContractError::Std(StdError::generic_err(
                "Failed to calculate community pool share due to overflow",
            ))
        })?
        // round down here to avoid claiming more than the tax amount
        .to_uint_floor();

    Ok(community_pool_share)
}

// Given a funds amount, calculate how much of it should go to the voters
pub fn get_voters_tribute_share(config: &Config, funds: Coin) -> Result<Uint128, ContractError> {
    // Calculate the voters share
    let voters_share = funds
        .amount
        .checked_sub(get_community_pool_tribute_share(config, funds)?)
        .map_err(|_| {
            ContractError::Std(StdError::generic_err(
                "Failed to calculate voters share due to overflow",
            ))
        })?;

    Ok(voters_share)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}

// TODO: figure out build issue that we have if we don't define all this functions in both contracts
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, _msg: Reply) -> Result<Response, ContractError> {
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}
