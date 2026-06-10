use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Order,
    Response, StdResult, Uint128,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;

use crate::{
    error::ContractError,
    msg::{AllPairsResponse, ConfigResponse, ExecuteMsg, InstantiateMsg, PairResponse, QueryMsg},
    state::{ADMIN, PAIRS},
};

const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const DEFAULT_LIMIT: u32 = 30;
const MAX_LIMIT: u32 = 100;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let admin = deps.api.addr_validate(&msg.admin)?;
    ADMIN.save(deps.storage, &admin)?;

    for pair in &msg.pairs {
        PAIRS.save(
            deps.storage,
            pair.neutron_shares_denom.as_str(),
            &pair.cosmos_hub_shares_denom,
        )?;
    }

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("admin", admin))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Convert {} => convert(deps, env, info),
        ExecuteMsg::AddPair {
            neutron_shares_denom,
            cosmos_hub_shares_denom,
        } => add_pair(deps, info, neutron_shares_denom, cosmos_hub_shares_denom),
        ExecuteMsg::RemovePair {
            neutron_shares_denom,
        } => remove_pair(deps, info, neutron_shares_denom),
    }
}

fn convert(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    if info.funds.len() != 1 {
        return Err(ContractError::InvalidFunds);
    }
    let sent = &info.funds[0];

    let cosmos_hub_denom = PAIRS
        .may_load(deps.storage, sent.denom.as_str())?
        .ok_or_else(|| ContractError::PairNotFound {
            denom: sent.denom.clone(),
        })?;

    let contract_balance = deps
        .querier
        .query_balance(&env.contract.address, &cosmos_hub_denom)
        .unwrap_or(Coin {
            denom: cosmos_hub_denom.clone(),
            amount: Uint128::zero(),
        });

    if contract_balance.amount < sent.amount {
        return Err(ContractError::InsufficientBalance {
            available: contract_balance.amount,
            required: sent.amount,
        });
    }

    Ok(Response::new()
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin {
                denom: cosmos_hub_denom.clone(),
                amount: sent.amount,
            }],
        })
        .add_attribute("action", "convert")
        .add_attribute("sender", &info.sender)
        .add_attribute("neutron_denom", &sent.denom)
        .add_attribute("cosmos_hub_denom", cosmos_hub_denom)
        .add_attribute("amount", sent.amount))
}

fn add_pair(
    deps: DepsMut,
    info: MessageInfo,
    neutron_shares_denom: String,
    cosmos_hub_shares_denom: String,
) -> Result<Response, ContractError> {
    let admin = ADMIN.load(deps.storage)?;
    if info.sender != admin {
        return Err(ContractError::Unauthorized);
    }

    PAIRS.save(
        deps.storage,
        neutron_shares_denom.as_str(),
        &cosmos_hub_shares_denom,
    )?;

    Ok(Response::new()
        .add_attribute("action", "add_pair")
        .add_attribute("neutron_denom", neutron_shares_denom)
        .add_attribute("cosmos_hub_denom", cosmos_hub_shares_denom))
}

fn remove_pair(
    deps: DepsMut,
    info: MessageInfo,
    neutron_shares_denom: String,
) -> Result<Response, ContractError> {
    let admin = ADMIN.load(deps.storage)?;
    if info.sender != admin {
        return Err(ContractError::Unauthorized);
    }

    PAIRS.remove(deps.storage, neutron_shares_denom.as_str());

    Ok(Response::new()
        .add_attribute("action", "remove_pair")
        .add_attribute("neutron_denom", neutron_shares_denom))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::Pair { neutron_denom } => to_json_binary(&query_pair(deps, neutron_denom)?),
        QueryMsg::AllPairs { start_after, limit } => {
            to_json_binary(&query_all_pairs(deps, start_after, limit)?)
        }
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let admin = ADMIN.load(deps.storage)?;
    Ok(ConfigResponse {
        admin: admin.to_string(),
    })
}

fn query_pair(deps: Deps, neutron_denom: String) -> StdResult<Option<PairResponse>> {
    let cosmos_hub_denom = PAIRS.may_load(deps.storage, neutron_denom.as_str())?;
    Ok(
        cosmos_hub_denom.map(|cosmos_hub_shares_denom| PairResponse {
            neutron_shares_denom: neutron_denom,
            cosmos_hub_shares_denom,
        }),
    )
}

fn query_all_pairs(
    deps: Deps,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<AllPairsResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let start = start_after.as_deref().map(Bound::exclusive);

    let pairs = PAIRS
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|item| {
            item.map(
                |(neutron_shares_denom, cosmos_hub_shares_denom)| PairResponse {
                    neutron_shares_denom,
                    cosmos_hub_shares_denom,
                },
            )
        })
        .collect::<StdResult<Vec<_>>>()?;

    Ok(AllPairsResponse { pairs })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::ConversionPair;
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env, MockApi};
    use cosmwasm_std::{coins, from_json};

    const ADMIN: &str = "admin";
    const USER: &str = "user1";
    const NEUTRON_DENOM: &str =
        "ibc/C744236911B9CAA806DCDD730C9EBA323CB53B822B2EBD77BF977412B2E64DA1";
    const HUB_DENOM: &str =
        "factory/cosmos1qg5ega6dykkxc307y25pecuufrjkxkaggkkxh7nad0vhyhtuhw3s6ufdm4/inflow_uatom";

    fn api() -> MockApi {
        MockApi::default()
    }

    fn do_instantiate(deps: DepsMut) {
        let admin_addr = api().addr_make(ADMIN);
        let msg = InstantiateMsg {
            admin: admin_addr.to_string(),
            pairs: vec![ConversionPair {
                neutron_shares_denom: NEUTRON_DENOM.to_string(),
                cosmos_hub_shares_denom: HUB_DENOM.to_string(),
            }],
        };
        instantiate(deps, mock_env(), message_info(&admin_addr, &[]), msg).unwrap();
    }

    #[test]
    fn test_instantiate_registers_pairs() {
        let mut deps = mock_dependencies();
        do_instantiate(deps.as_mut());

        let res: Option<PairResponse> = from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::Pair {
                    neutron_denom: NEUTRON_DENOM.to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        let pair = res.unwrap();
        assert_eq!(pair.neutron_shares_denom, NEUTRON_DENOM);
        assert_eq!(pair.cosmos_hub_shares_denom, HUB_DENOM);
    }

    #[test]
    fn test_convert_unknown_denom_fails() {
        let mut deps = mock_dependencies();
        do_instantiate(deps.as_mut());
        let user = api().addr_make(USER);

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&user, &coins(100, "ibc/UNKNOWN")),
            ExecuteMsg::Convert {},
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::PairNotFound { .. }));
    }

    #[test]
    fn test_convert_no_funds_fails() {
        let mut deps = mock_dependencies();
        do_instantiate(deps.as_mut());
        let user = api().addr_make(USER);

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&user, &[]),
            ExecuteMsg::Convert {},
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InvalidFunds));
    }

    #[test]
    fn test_convert_multiple_funds_fails() {
        let mut deps = mock_dependencies();
        do_instantiate(deps.as_mut());
        let user = api().addr_make(USER);

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(
                &user,
                &[
                    Coin::new(100u128, NEUTRON_DENOM),
                    Coin::new(50u128, "uatom"),
                ],
            ),
            ExecuteMsg::Convert {},
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InvalidFunds));
    }

    #[test]
    fn test_convert_insufficient_balance_fails() {
        let mut deps = mock_dependencies();
        do_instantiate(deps.as_mut());
        let user = api().addr_make(USER);

        // Contract has zero Hub shares — conversion should fail
        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&user, &coins(100, NEUTRON_DENOM)),
            ExecuteMsg::Convert {},
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InsufficientBalance { .. }));
    }

    #[test]
    fn test_add_pair_unauthorized() {
        let mut deps = mock_dependencies();
        do_instantiate(deps.as_mut());
        let user = api().addr_make(USER);

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&user, &[]),
            ExecuteMsg::AddPair {
                neutron_shares_denom: "ibc/NEW".to_string(),
                cosmos_hub_shares_denom: "factory/x/new".to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized));
    }

    #[test]
    fn test_add_and_remove_pair() {
        let mut deps = mock_dependencies();
        do_instantiate(deps.as_mut());
        let admin = api().addr_make(ADMIN);

        execute(
            deps.as_mut(),
            mock_env(),
            message_info(&admin, &[]),
            ExecuteMsg::AddPair {
                neutron_shares_denom: "ibc/NEW".to_string(),
                cosmos_hub_shares_denom: "factory/x/new".to_string(),
            },
        )
        .unwrap();

        let res: Option<PairResponse> = from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::Pair {
                    neutron_denom: "ibc/NEW".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert!(res.is_some());

        execute(
            deps.as_mut(),
            mock_env(),
            message_info(&admin, &[]),
            ExecuteMsg::RemovePair {
                neutron_shares_denom: "ibc/NEW".to_string(),
            },
        )
        .unwrap();

        let res: Option<PairResponse> = from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::Pair {
                    neutron_denom: "ibc/NEW".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert!(res.is_none());
    }

    #[test]
    fn test_query_all_pairs() {
        let mut deps = mock_dependencies();
        do_instantiate(deps.as_mut());

        let res: AllPairsResponse = from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::AllPairs {
                    start_after: None,
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(res.pairs.len(), 1);
        assert_eq!(res.pairs[0].neutron_shares_denom, NEUTRON_DENOM);
    }
}
