use cosmwasm_std::{from_json, to_json_binary, Addr, Binary, StdError, StdResult};
#[cfg(feature = "cosmwasm_compat")]
use interface::compat::StdErrExt;
use interface::{
    inflow_control_center::{QueryMsg as ControlCenterQueryMsg, SubvaultsResponse},
    inflow_control_center_registry::{ControlCentersResponse, QueryMsg as RegistryQueryMsg},
    inflow_vault::{
        Config as InflowConfig, ConfigResponse as InflowConfigResponse, QueryMsg as InflowQueryMsg,
    },
};

pub fn control_center_subvaults_mock(
    subvaults: Vec<Addr>,
) -> impl Fn(&Binary) -> StdResult<Binary> + 'static {
    move |msg| match from_json(msg).unwrap() {
        ControlCenterQueryMsg::Subvaults {} => to_json_binary(&SubvaultsResponse {
            subvaults: subvaults.clone(),
        }),
        _ => Err(StdError::generic_err(
            "unsupported query type in control center mock",
        )),
    }
}

pub fn inflow_config_mock(config: InflowConfig) -> impl Fn(&Binary) -> StdResult<Binary> + 'static {
    move |msg| match from_json(msg).unwrap() {
        InflowQueryMsg::Config {} => to_json_binary(&InflowConfigResponse {
            config: config.clone(),
        }),
        _ => Err(StdError::generic_err(
            "unsupported query type in inflow vault mock",
        )),
    }
}

pub fn control_center_registry_mock(
    control_centers: Vec<Addr>,
) -> impl Fn(&Binary) -> StdResult<Binary> + 'static {
    move |msg| match from_json(msg).unwrap() {
        RegistryQueryMsg::ControlCenters {} => to_json_binary(&ControlCentersResponse {
            control_centers: control_centers.clone(),
        }),
        _ => Err(StdError::generic_err(
            "unsupported query type in registry mock",
        )),
    }
}
