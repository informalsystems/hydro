use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo,
    Order, Response, StdError, StdResult,
};

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg};
use crate::query::QueryMsg;
use crate::state::{Config, Tribute, CONFIG, TRIBUTE_CLAIMS, TRIBUTE_ID, TRIBUTE_MAP};
use atom_wars::{Proposal, QueryMsg as AtomWarsQueryMsg, Vote};

pub const DEFAULT_MAX_ENTRIES: usize = 100;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let config = Config {
        atom_wars_contract: deps.api.addr_validate(&msg.atom_wars_contract)?,
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
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::AddTribute {
            tranche_id,
            proposal_id,
        } => add_tribute(deps, env, info, tranche_id, proposal_id),
        ExecuteMsg::ClaimTribute {
            round_id,
            tranche_id,
            tribute_id,
        } => claim_tribute(deps, env, info, round_id, tranche_id, tribute_id),
        ExecuteMsg::RefundTribute {
            round_id,
            tranche_id,
            proposal_id,
            tribute_id,
        } => refund_tribute(
            deps,
            env,
            info,
            round_id,
            proposal_id,
            tranche_id,
            tribute_id,
        ),
    }
}

fn add_tribute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<Response, ContractError> {
    let atom_wars_contract = CONFIG.load(deps.storage)?.atom_wars_contract;
    let current_round_id = query_current_round_id(&deps, &atom_wars_contract)?;

    // Check that the proposal exists
    query_proposal(
        &deps,
        &atom_wars_contract,
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

// ClaimTribute(round_id, tranche_id, prop_id):
//     Check that the round is ended
//     Check that the prop was among the top N proposals for this tranche/round
//     Look up sender's vote for the round
//     Check that the sender voted for the prop
//     Check that the sender has not already claimed the tribute
//     Divide sender's vote power by total power voting for the prop to figure out their percentage
//     Use the sender's percentage to send them the right portion of the tribute
//     Mark on the sender's vote that they claimed the tribute
fn claim_tribute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    round_id: u64,
    tranche_id: u64,
    tribute_id: u64,
) -> Result<Response, ContractError> {
    // Check that the sender has not already claimed the tribute using the TRIBUTE_CLAIMS map
    if TRIBUTE_CLAIMS.may_load(deps.storage, (info.sender.clone(), tribute_id))? == Some(true) {
        return Err(ContractError::Std(StdError::generic_err(
            "Sender has already claimed the tribute",
        )));
    }

    // Check that the round is ended
    let config = CONFIG.load(deps.storage)?;
    let current_round_id = query_current_round_id(&deps, &config.atom_wars_contract)?;

    if round_id >= current_round_id {
        return Err(ContractError::Std(StdError::generic_err(
            "Round has not ended yet",
        )));
    }

    // Look up sender's vote for the round, error if it cannot be found
    let vote = query_user_vote(
        &deps,
        &config.atom_wars_contract,
        round_id,
        tranche_id,
        info.sender.clone().to_string(),
    )?;

    // Check that the sender voted for one of the top N proposals
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

    // Divide sender's vote power by the prop's power to figure out their percentage
    let percentage_fraction = (vote.power, proposal.power);
    // checked_mul_floor() is used so that, due to the precision, contract doesn't transfer by 1 token more
    // to some users, which would leave the last users trying to claim the tribute unable to do so
    // This also implies that some dust amount of tokens could be left on the contract after everyone
    // claiming their portion of the tribute
    let amount = match tribute.funds.amount.checked_mul_floor(percentage_fraction) {
        Ok(amount) => amount,
        Err(_) => {
            return Err(ContractError::Std(StdError::generic_err(
                "Failed to compute users tribute share",
            )));
        }
    };

    // Mark in the TRIBUTE_CLAIMS that the sender has claimed this tribute
    TRIBUTE_CLAIMS.save(deps.storage, (info.sender.clone(), tribute_id), &true)?;

    // Send the tribute to the sender
    Ok(Response::new()
        .add_attribute("action", "claim_tribute")
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin {
                denom: tribute.funds.denom,
                amount,
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
    _env: Env,
    info: MessageInfo,
    round_id: u64,
    proposal_id: u64,
    tranche_id: u64,
    tribute_id: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Check that the round is ended by checking that the round_id is less than the current round
    let current_round_id = query_current_round_id(&deps, &config.atom_wars_contract)?;
    if round_id >= current_round_id {
        return Err(ContractError::Std(StdError::generic_err(
            "Round has not ended yet",
        )));
    }

    if let Some(_) = get_top_n_proposal(&deps, &config, round_id, tranche_id, proposal_id)? {
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
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::ProposalTributes {
            round_id,
            tranche_id,
            proposal_id,
        } => to_json_binary(&query_proposal_tributes(
            deps,
            round_id,
            tranche_id,
            proposal_id,
        )),
    }
}

pub fn query_proposal_tributes(
    deps: Deps,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> Vec<Tribute> {
    TRIBUTE_MAP
        .prefix(((round_id, tranche_id), proposal_id))
        .range(deps.storage, None, None, Order::Ascending)
        .map(|l| l.unwrap().1)
        .take(DEFAULT_MAX_ENTRIES)
        .collect()
}

fn query_current_round_id(deps: &DepsMut, atom_wars_contract: &Addr) -> Result<u64, ContractError> {
    let current_round_id: u64 = deps
        .querier
        .query_wasm_smart(atom_wars_contract, &AtomWarsQueryMsg::CurrentRound {})?;

    Ok(current_round_id)
}

fn query_proposal(
    deps: &DepsMut,
    atom_wars_contract: &Addr,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<Proposal, ContractError> {
    let proposal: Proposal = deps.querier.query_wasm_smart(
        atom_wars_contract,
        &AtomWarsQueryMsg::Proposal {
            round_id,
            tranche_id,
            proposal_id,
        },
    )?;

    Ok(proposal)
}

fn query_user_vote(
    deps: &DepsMut,
    atom_wars_contract: &Addr,
    round_id: u64,
    tranche_id: u64,
    address: String,
) -> Result<Vote, ContractError> {
    Ok(deps.querier.query_wasm_smart(
        atom_wars_contract,
        &AtomWarsQueryMsg::UserVote {
            round_id,
            tranche_id,
            address,
        },
    )?)
}

fn get_top_n_proposal(
    deps: &DepsMut,
    config: &Config,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<Option<Proposal>, ContractError> {
    let proposals: Vec<Proposal> = deps.querier.query_wasm_smart(
        &config.atom_wars_contract,
        &AtomWarsQueryMsg::TopNProposals {
            round_id,
            tranche_id,
            number_of_proposals: config.top_n_props_count as usize,
        },
    )?;

    for proposal in proposals {
        if proposal.proposal_id == proposal_id {
            return Ok(Some(proposal));
        }
    }

    Ok(None)
}
