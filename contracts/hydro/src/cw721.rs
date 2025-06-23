use crate::{
    contract::{compute_current_round_id, compute_round_end},
    error::ContractError,
    msg::Cw721ReceiveMsg,
    query::{
        AllNftInfoResponse, ApprovalResponse, ApprovalsResponse, CollectionInfoResponse,
        NftInfoResponse, NumTokensResponse, OperatorsResponse, OwnerOfResponse, TokensResponse,
    },
    state::{
        Approval, LockEntryV2, LOCKS_MAP_V2, LOCK_ID, NFT_APPROVALS, NFT_OPERATORS,
        TOKEN_INFO_PROVIDERS, TRANCHE_MAP, USER_LOCKS, USER_LOCKS_FOR_CLAIM,
    },
    token_manager::{TokenInfoProvider, TokenManager, LSM_TOKEN_INFO_PROVIDER_ID},
    utils::{load_current_constants, to_lockup_with_power, to_lockup_with_tranche_infos},
};

use cosmwasm_std::{
    Addr, Binary, BlockInfo, Deps, DepsMut, Env, MessageInfo, Order, Response, StdResult, Storage,
};
use cw_utils::Expiration;
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

use cw_storage_plus::Bound;

const DEFAULT_QUERY_LIMIT: u32 = 10;
const MAX_QUERY_LIMIT: u32 = 100;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid token_id")]
    InvalidTokenId,
    #[error("recipient is not a contract")]
    NonContractRecipient,
    #[error("invalid spender address")]
    InvalidSpenderAddress,
    #[error("invalid operator address")]
    InvalidOperatorAddress,
    #[error("expiration already expired")]
    ExpirationAlreadyExpired,
    #[error("invalid start_after")]
    InvalidStartAfter,
    #[error("no approval found")]
    NoApprovalFound,
    #[error("approval expired")]
    ApprovalExpired,
    #[error("cannot transfer lsm lockups")]
    LSMNotTransferrable,
    #[error("cannot approve lsm lockups")]
    LSMNotApprovable,
    #[error("cannot revoke lsm lockups")]
    LSMNotRevokable,
    #[error("lsm token info provider is not lsm")]
    LSMTokenInfoProviderNotLSM,
    #[error("cannot transfer tokens to oneself")]
    ForbiddenTransferToOneself,
}

/// This transfers ownership of the token to recipient account.
/// This is designed to send to an address controlled by a private key and does not trigger any actions on the recipient if it is a contract.
/// Requires token_id to point to a valid token, and env.sender to be the owner of it, or have an allowance to transfer it.
pub fn handle_execute_transfer(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    recipient: String,
    token_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let recipient_addr = deps.api.addr_validate(&recipient)?;
    let lock_id = token_id.parse().map_err(|_| Error::InvalidTokenId)?;
    let lock_entry = LOCKS_MAP_V2.load(deps.storage, lock_id)?;

    // The check of the ownership or allowance to transfer is done within transfer function
    transfer(deps, &env, &info, recipient_addr, lock_entry)?;

    Ok(Response::new()
        .add_attribute("action", "transfer_nft")
        .add_attribute("from", info.sender)
        .add_attribute("to", recipient)
        .add_attribute("token_id", token_id))
}

/// This transfers ownership of the token to contract account.
/// The contract field must be an address controlled by a smart contract, which implements the CW721Receiver interface.
/// The msg will be passed to the recipient contract, along with the token_id.
pub fn handle_execute_send_nft(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    contract: String,
    token_id: String,
    msg: Binary,
) -> Result<Response<NeutronMsg>, ContractError> {
    let recipient_addr = deps.api.addr_validate(&contract)?;
    let lock_id = token_id.parse().map_err(|_| Error::InvalidTokenId)?;
    let lock_entry = LOCKS_MAP_V2.load(deps.storage, lock_id)?;

    // verify that recipient is a contract
    deps.querier
        .query_wasm_contract_info(&recipient_addr)
        .map_err(|_| Error::NonContractRecipient)?;

    // The check of the ownership or allowance to transfer is done within transfer function
    transfer(deps, &env, &info, recipient_addr, lock_entry)?;

    // Create `ReceiveNft`
    let receive_msg = Cw721ReceiveMsg {
        sender: info.sender.to_string(), // user who called send_nft
        token_id: token_id.clone(),
        msg,
    };

    Ok(Response::new()
        .add_message(receive_msg.into_cosmos_msg(contract.clone())?)
        .add_attribute("action", "send_nft")
        .add_attribute("from", info.sender)
        .add_attribute("to", contract)
        .add_attribute("token_id", token_id))
}

/// Transfer a lockup to a new owner
/// This function is used when the lockup is transferred to a new owner by the current owner or by an approved address.
/// It updates the owner of the lockup and the list of locks for the old and new owners.
/// It also clears existing approvals for the lockup.
fn transfer(
    deps: DepsMut<'_, NeutronQuery>,
    env: &Env,
    info: &MessageInfo,
    recipient: Addr,
    lock_entry: LockEntryV2,
) -> Result<(), ContractError> {
    // Check that the sender is the owner or has approval to transfer
    if !can_user_transfer(deps.storage, &info.sender, &lock_entry, &env.block)? {
        return Err(ContractError::Unauthorized {});
    }

    // Error out if the lockup denom is LSM. This is temporary until we allow LSM lockups to be transferred
    if is_denom_lsm(&deps, lock_entry.funds.denom)? {
        return Err(Error::LSMNotTransferrable.into());
    }

    // Clear approvals for the lock ID
    clear_nft_approvals(deps.storage, lock_entry.lock_id)?;

    let old_owner_addr = lock_entry.owner;
    let new_owner_addr = recipient;

    if old_owner_addr == new_owner_addr {
        return Err(Error::ForbiddenTransferToOneself.into());
    }

    // Update the owner of the lockup
    LOCKS_MAP_V2.update(
        deps.storage,
        lock_entry.lock_id,
        env.block.height,
        |lock_entry| {
            let mut lock_entry = lock_entry.expect("transferred lock entry must exist");
            lock_entry.owner = new_owner_addr.clone();
            StdResult::Ok(lock_entry)
        },
    )?;

    // Remove the lock entry from the old owner
    USER_LOCKS.update(
        deps.storage,
        old_owner_addr.clone(),
        env.block.height,
        |current_locks| {
            let mut current_locks = current_locks.expect("old owner must have at least 1 lock");
            current_locks.retain(|lock_id| lock_id != &lock_entry.lock_id);
            StdResult::Ok(current_locks)
        },
    )?;

    USER_LOCKS_FOR_CLAIM.update(deps.storage, old_owner_addr.clone(), |current_locks| {
        let mut current_locks = current_locks.expect("old owner must have at least 1 lock");
        current_locks.retain(|lock_id| lock_id != &lock_entry.lock_id);
        StdResult::Ok(current_locks)
    })?;

    // Add the lock entry to the new owner
    USER_LOCKS.update(
        deps.storage,
        new_owner_addr.clone(),
        env.block.height,
        |current_locks| {
            let mut locks = current_locks.unwrap_or_default();
            locks.push(lock_entry.lock_id);
            StdResult::Ok(locks)
        },
    )?;

    USER_LOCKS_FOR_CLAIM.update(deps.storage, new_owner_addr.clone(), |current_locks| {
        let mut locks = current_locks.unwrap_or_default();
        locks.push(lock_entry.lock_id);
        StdResult::Ok(locks)
    })?;

    Ok(())
}

pub fn clear_nft_approvals(storage: &mut dyn Storage, lock_id: u64) -> Result<(), ContractError> {
    let keys_to_remove = NFT_APPROVALS
        .prefix(lock_id)
        .keys(storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    for spender_addr in keys_to_remove {
        NFT_APPROVALS.remove(storage, (lock_id, spender_addr));
    }

    Ok(())
}

/// Grants permission to spender to transfer or send the given token.
/// This can only be performed when env.sender is the owner of the given token_id or an operator.
/// There can be multiple spender accounts per token, and they are cleared once the token is transferred or sent.
pub fn handle_execute_approve(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    spender: String,
    expires: Option<Expiration>,
    token_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Checks the spender address is valid
    let spender_addr = deps
        .api
        .addr_validate(&spender)
        .map_err(|_| Error::InvalidSpenderAddress)?;

    // Checks the token_id is valid
    let lock_id = token_id.parse().map_err(|_| Error::InvalidTokenId)?;

    // Check that we can create an approval (owner or operator)
    let lock_entry = LOCKS_MAP_V2.load(deps.storage, lock_id)?;
    if !can_user_create_approval(deps.storage, &info.sender, &lock_entry, &env.block)? {
        return Err(ContractError::Unauthorized {});
    }

    // Error out if the lockup denom is LSM. This is temporary until we allow LSM lockups to be transferred
    if is_denom_lsm(&deps, lock_entry.funds.denom)? {
        return Err(Error::LSMNotApprovable.into());
    }

    let expires = expires.unwrap_or_default(); // default is Never

    if expires.is_expired(&env.block) {
        return Err(Error::ExpirationAlreadyExpired.into());
    }

    // Stores Approval
    let approval = Approval {
        spender: spender.clone(),
        expires,
    };
    NFT_APPROVALS.save(deps.storage, (lock_id, spender_addr), &approval)?;

    Ok(Response::new()
        .add_attribute("action", "approve")
        .add_attribute("spender", spender)
        .add_attribute("token_id", token_id))
}

/// This revokes a previously granted permission to transfer the given token_id.
/// This can only be granted when env.sender is the owner of the given token_id or an operator.
pub fn handle_execute_revoke(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    spender: String,
    token_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Checks the spender address is valid
    let spender_addr = deps
        .api
        .addr_validate(&spender)
        .map_err(|_| Error::InvalidSpenderAddress)?;

    // Checks the token_id is valid
    let lock_id = token_id.parse().map_err(|_| Error::InvalidTokenId)?;

    // Check that we can revoke an approval (owner or operator)
    let lock_entry = LOCKS_MAP_V2.load(deps.storage, lock_id)?;
    if !can_user_create_approval(deps.storage, &info.sender, &lock_entry, &env.block)? {
        return Err(ContractError::Unauthorized {});
    }

    // Error out if the lockup denom is LSM. This is temporary until we allow LSM lockups to be transferred
    if is_denom_lsm(&deps, lock_entry.funds.denom)? {
        return Err(Error::LSMNotRevokable.into());
    }

    // Removes approval
    NFT_APPROVALS.remove(deps.storage, (lock_id, spender_addr));

    Ok(Response::new()
        .add_attribute("action", "revoke")
        .add_attribute("spender", spender)
        .add_attribute("token_id", token_id))
}

/// Grant operator permission to transfer, send or create/revoke Approvals on all tokens owned by env.sender.
/// This approval is tied to the owner, not the tokens and applies to any future token that the owner receives as well.
pub fn handle_execute_approve_all(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    operator: String,
    expires: Option<Expiration>,
) -> Result<Response<NeutronMsg>, ContractError> {
    let operator_addr = deps
        .api
        .addr_validate(&operator)
        .map_err(|_| Error::InvalidOperatorAddress)?;

    // Determine expiration value
    let expires = expires.unwrap_or_default(); // default is Never

    if expires.is_expired(&env.block) {
        return Err(Error::ExpirationAlreadyExpired.into());
    }

    // Store operator approval for the owner
    NFT_OPERATORS.save(deps.storage, (info.sender.clone(), operator_addr), &expires)?;

    Ok(Response::new()
        .add_attribute("action", "approve_all")
        .add_attribute("owner", info.sender)
        .add_attribute("operator", operator))
}

// Revoke a previous ApproveAll permission granted to the given operator.
pub fn handle_execute_revoke_all(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    operator: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    let operator_addr = deps
        .api
        .addr_validate(&operator)
        .map_err(|_| Error::InvalidOperatorAddress)?;

    NFT_OPERATORS.remove(deps.storage, (info.sender.clone(), operator_addr));

    Ok(Response::new()
        .add_attribute("action", "revoke_all")
        .add_attribute("owner", info.sender)
        .add_attribute("operator", operator))
}

/// Returns the owner of the given token, as well as anyone with approval on this particular token.
/// If the token is unknown, returns an error.
/// If include_expired is set (to true), show expired approvals in the results, otherwise, ignore them.
pub fn query_owner_of(
    deps: Deps<NeutronQuery>,
    env: Env,
    token_id: String,
    include_expired: Option<bool>,
) -> Result<OwnerOfResponse, ContractError> {
    let lock_id = token_id.parse().map_err(|_| Error::InvalidTokenId)?;
    let lockup = LOCKS_MAP_V2.load(deps.storage, lock_id)?;
    let include_expired = include_expired.unwrap_or(false);

    let mut approvals = vec![];

    for kv_res in NFT_APPROVALS
        .prefix(lock_id)
        .range(deps.storage, None, None, Order::Ascending)
    {
        let (_, approval) = kv_res?;

        if include_expired || !approval.expires.is_expired(&env.block) {
            approvals.push(approval)
        }
    }

    Ok(OwnerOfResponse {
        owner: lockup.owner.to_string(),
        approvals,
    })
}

/// Return an approval of spender about the given token_id.
/// If include_expired is set (to true), show expired approval in the results, otherwise, ignore them.
pub fn query_approval(
    deps: Deps<NeutronQuery>,
    env: Env,
    token_id: String,
    spender: String,
    include_expired: Option<bool>,
) -> Result<ApprovalResponse, ContractError> {
    let spender_addr = deps.api.addr_validate(&spender)?;
    let lock_id = token_id.parse().map_err(|_| Error::InvalidTokenId)?;
    let lockup = LOCKS_MAP_V2.load(deps.storage, lock_id)?;

    // Owner is always approved
    if lockup.owner == spender_addr {
        return Ok(ApprovalResponse {
            approval: Approval {
                spender: lockup.owner.to_string(),
                expires: Expiration::Never {},
            },
        });
    }

    let include_expired = include_expired.unwrap_or(false);
    let Some(approval) = NFT_APPROVALS.may_load(deps.storage, (lock_id, spender_addr))? else {
        return Err(Error::NoApprovalFound.into());
    };

    if !include_expired && approval.expires.is_expired(&env.block) {
        return Err(Error::ApprovalExpired.into());
    }

    Ok(ApprovalResponse { approval })
}

/// Return all approvals that apply on the given token_id.
/// If include_expired is set (to true), show expired approvals in the results, otherwise, ignore them.
pub fn query_approvals(
    deps: Deps<NeutronQuery>,
    env: Env,
    token_id: String,
    include_expired: Option<bool>,
) -> Result<ApprovalsResponse, ContractError> {
    let include_expired = include_expired.unwrap_or(false);
    let lock_id = token_id.parse().map_err(|_| Error::InvalidTokenId)?;

    // Check that the token_id is valid
    LOCKS_MAP_V2.load(deps.storage, lock_id)?;

    let mut approvals = vec![];

    for kv_res in NFT_APPROVALS
        .prefix(lock_id)
        .range(deps.storage, None, None, Order::Ascending)
    {
        let (_, approval) = kv_res?;

        if include_expired || !approval.expires.is_expired(&env.block) {
            approvals.push(approval)
        }
    }

    Ok(ApprovalsResponse { approvals })
}

/// List operators that can access all of the owner's tokens.
/// If include_expired is set (to true), show expired operators in the results, otherwise, ignore them.
/// If start_after is set, then it returns the first limit operators after the given one.
pub fn query_all_operators(
    deps: Deps<NeutronQuery>,
    env: Env,
    owner: String,
    include_expired: Option<bool>,
    start_after: Option<String>,
    limit: Option<u32>,
) -> Result<OperatorsResponse, ContractError> {
    let owner_addr = deps.api.addr_validate(&owner)?;
    let include_expired = include_expired.unwrap_or(false);
    let limit = limit.unwrap_or(DEFAULT_QUERY_LIMIT).min(MAX_QUERY_LIMIT) as usize;

    // Determine start bound for pagination
    let start = start_after
        .map(|s| deps.api.addr_validate(&s))
        .transpose()?
        .map(Bound::exclusive);

    let mut operators = vec![];

    for kv_res in NFT_OPERATORS
        .prefix(owner_addr)
        .range(deps.storage, start, None, cosmwasm_std::Order::Ascending)
        .take(limit)
    {
        let (operator_addr, expiration) = kv_res?;

        if include_expired || !expiration.is_expired(&env.block) {
            operators.push(Approval {
                spender: operator_addr.into_string(),
                expires: expiration,
            })
        }
    }

    Ok(OperatorsResponse { operators })
}

/// Returns the number of tokens issued so far.
/// Note: This is not the same as the total number of tokens in existence,
/// as some tokens (lockups) may have been burned (unlocked by users).
pub fn query_num_tokens(deps: Deps<NeutronQuery>) -> Result<NumTokensResponse, ContractError> {
    let count = LOCK_ID.load(deps.storage)?;
    Ok(NumTokensResponse { count })
}

/// Returns top-level cw721 metadata about the contract. Namely, name and symbol.
pub fn query_collection_info(
    deps: Deps<NeutronQuery>,
    env: Env,
) -> Result<CollectionInfoResponse, ContractError> {
    let constants = load_current_constants(&deps, &env)?;

    Ok(constants.cw721_collection_info)
}

/// Returns metadata about one particular token.
/// The metadata extension is of type LockupWithPerTrancheInfo
pub fn query_nft_info(
    deps: Deps<NeutronQuery>,
    env: Env,
    token_id: String,
) -> Result<NftInfoResponse, ContractError> {
    let lock_id = token_id.parse().map_err(|_| Error::InvalidTokenId)?;
    let lockup = LOCKS_MAP_V2.load(deps.storage, lock_id)?;

    let constants = load_current_constants(&deps, &env)?;
    let current_round_id = compute_current_round_id(&env, &constants)?;
    let round_end = compute_round_end(&constants, current_round_id)?;
    let mut token_manager = TokenManager::new(&deps);

    let lockup_with_power = to_lockup_with_power(
        &deps,
        &constants,
        &mut token_manager,
        current_round_id,
        round_end,
        lockup,
    );

    let tranche_ids = TRANCHE_MAP
        .keys(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    let lockup_with_tranche_info = to_lockup_with_tranche_infos(
        &deps,
        &constants,
        &tranche_ids,
        lockup_with_power,
        current_round_id,
    )?;

    Ok(NftInfoResponse {
        token_uri: None,
        extension: lockup_with_tranche_info,
    })
}

/// Returns the result of both `NftInfo` and `OwnerOf` as one query as an optimization for clients.
/// If include_expired is set (to true), shows expired approvals in the results, otherwise, ignore them.
pub fn query_all_nft_info(
    deps: Deps<NeutronQuery>,
    env: Env,
    token_id: String,
    include_expired: Option<bool>,
) -> Result<AllNftInfoResponse, ContractError> {
    let nft_info = query_nft_info(deps, env.clone(), token_id.clone())?;
    let owner_of = query_owner_of(deps, env, token_id, include_expired)?;

    Ok(AllNftInfoResponse {
        access: owner_of,
        info: nft_info,
    })
}

// Lists token_ids owned by a given owner, ordered lexicographically by token_id, [] if no tokens.
// If start_after is unset, the query returns the first results.
// If start_after is set, then it returns the first limit tokens after the given one.
// If start_after is set and is invalid (i.e. not parsable to a number), the query returns an error.
pub fn query_tokens(
    deps: Deps<NeutronQuery>,
    owner: String,
    start_after: Option<String>,
    limit: Option<u32>,
) -> Result<TokensResponse, ContractError> {
    let address = deps.api.addr_validate(&owner)?;

    let limit = limit.unwrap_or(DEFAULT_QUERY_LIMIT).min(MAX_QUERY_LIMIT) as usize;

    let start_after: Option<u64> = start_after
        .map(|s| s.parse())
        .transpose()
        .map_err(|_| Error::InvalidStartAfter)?;

    let mut tokens = USER_LOCKS.load(deps.storage, address)?;
    tokens.sort_unstable();

    let is_after_start = |lock_id: u64| match start_after {
        Some(threshold) => lock_id > threshold,
        None => true,
    };

    let tokens: Vec<String> = tokens
        .into_iter()
        .filter(|&lock_id| is_after_start(lock_id))
        .take(limit)
        .map(|lock_id| lock_id.to_string())
        .collect();

    Ok(TokensResponse { tokens })
}

/// Lists token_ids controlled by the contract, ordered lexicographically by token_id.
pub fn query_all_tokens(
    deps: Deps<NeutronQuery>,
    _env: Env,
    start_after: Option<String>,
    limit: Option<u32>,
) -> Result<TokensResponse, ContractError> {
    let limit = limit.unwrap_or(DEFAULT_QUERY_LIMIT).min(MAX_QUERY_LIMIT) as usize;

    let start_after: Option<u64> = start_after
        .map(|s| s.parse())
        .transpose()
        .map_err(|_| Error::InvalidStartAfter)?;

    let start_bound = start_after.map(Bound::exclusive);

    // Order::Ascending returns ordered keys, thanks to big-endian storage of u64 keys
    // see https://github.com/CosmWasm/cw-storage-plus/blob/main/src/int_key.rs
    let tokens: Vec<String> = LOCKS_MAP_V2
        .keys(deps.storage, start_bound, None, Order::Ascending)
        .take(limit)
        .map(|res| res.map(|lock_id| lock_id.to_string()))
        .collect::<StdResult<_>>()?;

    Ok(TokensResponse { tokens })
}

/// Returns true if the user has a valid per-token approval, false otherwise.
fn has_valid_token_approval(
    storage: &dyn Storage,
    user_addr: &Addr,
    lock_id: u64,
    block: &BlockInfo,
) -> Result<bool, ContractError> {
    NFT_APPROVALS
        .may_load(storage, (lock_id, user_addr.clone()))?
        .map_or(Ok(false), |approval| {
            Ok(!approval.expires.is_expired(block))
        })
}

/// Returns true if the user is a valid operator for the owner, false otherwise.
fn has_valid_operator_approval(
    storage: &dyn Storage,
    owner: &Addr,
    user_addr: &Addr,
    block: &BlockInfo,
) -> Result<bool, ContractError> {
    NFT_OPERATORS
        .may_load(storage, (owner.clone(), user_addr.clone()))?
        .map_or(Ok(false), |expiration| Ok(!expiration.is_expired(block)))
}

/// Returns true if the user is allowed to transfer the Lock Entry, false otherwise.
/// (i.e. is the owner, is explicitly approved for this token, or is an operator for the owner)
fn can_user_transfer(
    storage: &dyn Storage,
    user_addr: &Addr,
    lock_entry: &LockEntryV2,
    block: &BlockInfo,
) -> Result<bool, ContractError> {
    Ok(lock_entry.owner == user_addr
        || has_valid_token_approval(storage, user_addr, lock_entry.lock_id, block)?
        || has_valid_operator_approval(storage, &lock_entry.owner, user_addr, block)?)
}

/// Returns true if the user is allowed to create an approval on the NFT (lock entry)
/// (i.e. is the owner, or is an operator for the owner)
fn can_user_create_approval(
    storage: &dyn Storage,
    user_addr: &Addr,
    lock_entry: &LockEntryV2,
    block: &BlockInfo,
) -> Result<bool, ContractError> {
    Ok(lock_entry.owner == user_addr
        || has_valid_operator_approval(storage, &lock_entry.owner, user_addr, block)?)
}

/// Returns true if the denom is LSM, false otherwise.
fn is_denom_lsm(deps: &DepsMut<'_, NeutronQuery>, denom: String) -> Result<bool, ContractError> {
    let lsm_info_provider =
        TOKEN_INFO_PROVIDERS.may_load(deps.storage, LSM_TOKEN_INFO_PROVIDER_ID.to_string())?;

    // If the contract has an LSM token info provider, check if the token is LSM
    if let Some(provider) = lsm_info_provider {
        let lsm_info_provider = match provider {
            TokenInfoProvider::LSM(lsm) => lsm,
            _ => {
                return Err(Error::LSMTokenInfoProviderNotLSM.into()); // Should never happen
            }
        };

        if lsm_info_provider.is_lsm_denom(&deps.as_ref(), denom) {
            return Ok(true);
        }
    }

    Ok(false)
}
