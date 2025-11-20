use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Response,
    StdResult, Uint128,
};
use cw2::set_contract_version;

use crate::{
    error::ContractError,
    msg::{ExecuteMsg, InstantiateMsg, QueryMsg, StateResponse},
    state::{ActionState, State, STATE},
};

const CONTRACT_NAME: &str = "crates.io:proxy";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    if msg.admins.is_empty() {
        return Err(ContractError::NoAdmins {});
    }

    let admins = msg
        .admins
        .iter()
        .map(|addr| deps.api.addr_validate(addr))
        .collect::<StdResult<Vec<Addr>>>()?;

    let state = State {
        admins,
        last_action: ActionState::default(),
    };

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    STATE.save(deps.storage, &state)?;

    Ok(Response::new().add_attribute("action", "instantiate_proxy"))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::ForwardToInflow {} => forward_to_inflow(deps),
        ExecuteMsg::WithdrawReceiptTokens { address, amount } => {
            withdraw_receipt_tokens(deps, info, address, amount)
        }
        ExecuteMsg::WithdrawFunds { address, amount } => {
            withdraw_funds(deps, info, address, amount)
        }
    }
}

fn forward_to_inflow(deps: DepsMut) -> Result<Response, ContractError> {
    STATE.update(deps.storage, |mut state| -> Result<_, ContractError> {
        state.last_action = ActionState::Forwarded;
        Ok(state)
    })?;

    Ok(Response::new().add_attribute("action", "forward_to_inflow"))
}

fn withdraw_receipt_tokens(
    deps: DepsMut,
    info: MessageInfo,
    address: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    process_withdrawal(deps, info, address, amount, ActionKind::ReceiptTokens)
}

fn withdraw_funds(
    deps: DepsMut,
    info: MessageInfo,
    address: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    process_withdrawal(deps, info, address, amount, ActionKind::Funds)
}

enum ActionKind {
    ReceiptTokens,
    Funds,
}

fn process_withdrawal(
    deps: DepsMut,
    info: MessageInfo,
    address: String,
    amount: Uint128,
    kind: ActionKind,
) -> Result<Response, ContractError> {
    let recipient = deps.api.addr_validate(&address)?;

    let mut state = STATE.load(deps.storage)?;
    ensure_admin(&state, &info.sender)?;

    state.last_action = match kind {
        ActionKind::ReceiptTokens => ActionState::WithdrawReceiptTokens {
            recipient: recipient.clone(),
            amount,
        },
        ActionKind::Funds => ActionState::WithdrawFunds {
            recipient: recipient.clone(),
            amount,
        },
    };

    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_attribute(
            "action",
            match kind {
                ActionKind::ReceiptTokens => "withdraw_receipt_tokens",
                ActionKind::Funds => "withdraw_funds",
            },
        )
        .add_attribute("recipient", recipient)
        .add_attribute("amount", amount))
}

fn ensure_admin(state: &State, sender: &Addr) -> Result<(), ContractError> {
    if state.admins.iter().any(|addr| addr == sender) {
        Ok(())
    } else {
        Err(ContractError::Unauthorized {})
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::State {} => {
            let state = STATE.load(deps.storage)?;
            to_json_binary(&StateResponse { state })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Addr, MessageInfo,
    };

    const CREATOR: &str = "cosmwasm1wtqa75mkgwgncx8v4dep5aygmnq7gspaufggc5ev3u68et43qxmsqy5haw";
    const ADMIN1: &str = "cosmwasm1g807u64s6uvk3daw4k4h778h850put0qdny3llp3xn43y5dar0hqfdcpt4";
    const ADMIN2: &str = "cosmwasm195ay4pn6v07zenrafuhm5mnkklsj7kxa7gaz9djc9gjmkp0ehayszlp362";

    fn message(sender: &Addr) -> MessageInfo {
        MessageInfo {
            sender: sender.clone(),
            funds: vec![],
        }
    }

    fn instantiate_contract(deps: DepsMut) -> (Addr, Addr) {
        let creator = Addr::unchecked(CREATOR);
        let admin1 = Addr::unchecked(ADMIN1);
        let admin2 = Addr::unchecked(ADMIN2);
        let msg = InstantiateMsg {
            admins: vec![admin1.to_string(), admin2.to_string()],
        };
        instantiate(deps, mock_env(), message(&creator), msg).unwrap();
        (admin1, admin2)
    }

    #[test]
    fn cannot_instantiate_without_admins() {
        let mut deps = mock_dependencies();
        let creator = Addr::unchecked(CREATOR);
        let err = instantiate(
            deps.as_mut(),
            mock_env(),
            message(&creator),
            InstantiateMsg { admins: vec![] },
        )
        .unwrap_err();

        assert_eq!(err, ContractError::NoAdmins {});
    }

    #[test]
    fn forward_updates_state() {
        let mut deps = mock_dependencies();
        instantiate_contract(deps.as_mut());

        let actor = Addr::unchecked(CREATOR);
        execute(
            deps.as_mut(),
            mock_env(),
            message(&actor),
            ExecuteMsg::ForwardToInflow {},
        )
        .unwrap();

        let state = STATE.load(&deps.storage).unwrap();
        assert!(matches!(state.last_action, ActionState::Forwarded));
    }

    #[test]
    fn withdraw_requires_admin() {
        let mut deps = mock_dependencies();
        let (admin1, _) = instantiate_contract(deps.as_mut());

        let not_admin = Addr::unchecked(CREATOR);
        let err = execute(
            deps.as_mut(),
            mock_env(),
            message(&not_admin),
            ExecuteMsg::WithdrawFunds {
                address: ADMIN1.to_string(),
                amount: Uint128::new(10),
            },
        )
        .unwrap_err();
        assert_eq!(err, ContractError::Unauthorized {});

        execute(
            deps.as_mut(),
            mock_env(),
            message(&admin1),
            ExecuteMsg::WithdrawReceiptTokens {
                address: ADMIN2.to_string(),
                amount: Uint128::new(20),
            },
        )
        .unwrap();

        let state = STATE.load(&deps.storage).unwrap();
        match state.last_action {
            ActionState::WithdrawReceiptTokens { amount, .. } => {
                assert_eq!(amount, Uint128::new(20));
            }
            _ => panic!("unexpected action stored"),
        }
    }
}
