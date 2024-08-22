use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Decimal, Deps, DepsMut, Env,
    MessageInfo, Order, Response, StdError, StdResult, Uint128,
};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg};
use crate::query::{ConfigResponse, ProposalTributesResponse, QueryMsg};
use crate::state::{Config, Tribute, CONFIG, TRIBUTE_CLAIMS, TRIBUTE_ID, TRIBUTE_MAP};
use hydro::query::{
    CurrentRoundResponse, ProposalResponse, QueryMsg as HydroQueryMsg, TopNProposalsResponse,
    UserVoteResponse,
};
use hydro::state::{Proposal, Vote, VoteWithPower};

/// Contract name that is used for migration.
const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

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
        top_n_props_count: msg.top_n_props_count,
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
    if TRIBUTE_CLAIMS.may_load(deps.storage, (voter.clone(), tribute_id))? == Some(true) {
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
    let proposal = match get_top_n_proposal(&deps, &config, round_id, tranche_id, vote.prop_id)? {
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

    let amount = match Decimal::from_ratio(tribute.funds.amount, Uint128::one())
        .checked_mul(percentage_fraction)
    {
        Ok(amount) => amount,
        Err(_) => {
            return Err(ContractError::Std(StdError::generic_err(
                "Failed to compute users tribute share",
            )));
        }
    };

    // Mark in the TRIBUTE_CLAIMS that the voter has claimed this tribute
    TRIBUTE_CLAIMS.save(deps.storage, (voter.clone(), tribute_id), &true)?;

    // Send the tribute to the voter
    Ok(Response::new()
        .add_attribute("action", "claim_tribute")
        .add_message(BankMsg::Send {
            to_address: voter.to_string(),
            amount: vec![Coin {
                denom: tribute.funds.denom,
                // to_uint_floor() is used so that, due to the precision, contract doesn't transfer by 1 token more
                // to some users, which would leave the last users trying to claim the tribute unable to do so
                // This also implies that some dust amount of tokens could be left on the contract after everyone
                // claiming their portion of the tribute
                amount: amount.to_uint_floor(),
            }],
        }))
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

    if get_top_n_proposal(&deps, &config, round_id, tranche_id, proposal_id)?.is_some() {
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

fn get_top_n_proposal(
    deps: &DepsMut,
    config: &Config,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<Option<Proposal>, ContractError> {
    let proposals_resp: TopNProposalsResponse = deps.querier.query_wasm_smart(
        &config.hydro_contract,
        &HydroQueryMsg::TopNProposals {
            round_id,
            tranche_id,
            number_of_proposals: config.top_n_props_count as usize,
        },
    )?;

    for proposal in proposals_resp.proposals {
        if proposal.proposal_id == proposal_id {
            return Ok(Some(proposal));
        }
    }

    Ok(None)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}
