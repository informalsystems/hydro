use cosmwasm_std::{
    to_json_binary, to_json_vec, Addr, DepsMut, Reply, Response, StdError, StdResult, SubMsg,
    Uint128, WasmMsg,
};
use interface::{
    gatekeeper::{ExecuteLockTokensMsg, ExecuteMsg as GatekeeperExecuteMsg},
    utils::extract_response_msg_bytes_from_reply_msg,
};
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

use crate::{
    error::{new_generic_error, ContractError},
    msg::{InstantiateContractMsg, LockTokensProof, ReplyPayload},
    state::GATEKEEPER,
    utils::LockingInfo,
};

// Given the provided inputs, builds a SubMsg to instantiate the Gatekeeper
// contract during the instantiation of the Hydro contract.
pub fn build_init_gatekeeper_msg(
    gatekeeper_init_info: &Option<InstantiateContractMsg>,
) -> StdResult<Option<SubMsg<NeutronMsg>>> {
    match gatekeeper_init_info {
        Some(gatekeeper_init_info) => {
            let submsg: SubMsg<NeutronMsg> = SubMsg::reply_on_success(
                WasmMsg::Instantiate {
                    code_id: gatekeeper_init_info.code_id,
                    msg: gatekeeper_init_info.msg.clone(),
                    label: gatekeeper_init_info.label.clone(),
                    admin: gatekeeper_init_info.admin.clone(),
                    funds: vec![],
                },
                0,
            )
            .with_payload(to_json_vec(&ReplyPayload::InstantiateGatekeeper)?);

            Ok(Some(submsg))
        }
        None => Ok(None),
    }
}

// Handles the Reply of the Gatekeeper contract instantiation SubMsg by
// saving the Gatekeeper contract address into the Hydro contract state.
pub fn gatekeeper_handle_submsg_reply(
    deps: DepsMut<NeutronQuery>,
    msg: Reply,
) -> Result<Response<NeutronMsg>, ContractError> {
    let bytes = &extract_response_msg_bytes_from_reply_msg(&msg)?;
    let instantiate_msg_response = cw_utils::parse_instantiate_response_data(bytes)
        .map_err(|e| StdError::generic_err(format!("failed to parse reply message: {e:?}")))?;

    GATEKEEPER.save(deps.storage, &instantiate_msg_response.contract_address)?;

    Ok(Response::default())
}

// Build the SubMsg that will be executed against the Gatekeeper contract in order to
// verify if the user is eligible to lock the given number of tokens. The SubMsg will
// not be created in case there is no Gatekeeper address stored in the Hydro contract,
// or user is trying to lock tokens only in known users cap.
pub fn build_gatekeeper_lock_tokens_msg(
    deps: &DepsMut<NeutronQuery>,
    user_address: &Addr,
    locking_info: &LockingInfo,
    proof: &Option<LockTokensProof>,
) -> Result<Option<SubMsg<NeutronMsg>>, ContractError> {
    // If there is no Gatekeeper don't build the SubMsg
    let gatekeeper = match GATEKEEPER.may_load(deps.storage)? {
        None => return Ok(None),
        Some(gatekeeper) => gatekeeper,
    };

    // If user is not trying to lock anything in the public cap, no need to verify against the Gatekeeper
    let amount_to_lock = match locking_info.lock_in_public_cap {
        None => return Ok(None),
        Some(amount_to_lock) => Uint128::new(amount_to_lock),
    };

    let proof = match proof {
        None => {
            return Err(new_generic_error(
                "Proof must be provided in order to lock tokens.",
            ))
        }
        Some(proof) => proof,
    };

    let lock_tokens_msg = GatekeeperExecuteMsg::LockTokens(ExecuteLockTokensMsg {
        user_address: user_address.to_string(),
        amount_to_lock,
        maximum_amount: proof.maximum_amount,
        proof: proof.proof.clone(),
        sig_info: proof.sig_info.clone(),
    });

    let wasm_execute_msg = WasmMsg::Execute {
        contract_addr: gatekeeper,
        msg: to_json_binary(&lock_tokens_msg)?,
        funds: vec![],
    };

    Ok(Some(SubMsg::reply_never(wasm_execute_msg)))
}
