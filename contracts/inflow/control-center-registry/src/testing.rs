use cosmwasm_std::{
    from_json,
    testing::{message_info, mock_dependencies, mock_env},
    Addr,
};

use crate::{
    error::ContractError,
    execute, instantiate,
    msg::{ConfigResponse, ControlCentersResponse},
    query, ExecuteMsg, InstantiateMsg, QueryMsg,
};

#[test]
fn instantiate_validates_addresses_and_exposes_config() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let admin = deps.api.addr_make("admin");
    let center = deps.api.addr_make("center");
    let info = message_info(&admin, &[]);

    for msg in [
        InstantiateMsg {
            admin: "invalid".into(),
            control_centers: vec![],
        },
        InstantiateMsg {
            admin: admin.to_string(),
            control_centers: vec!["invalid".into()],
        },
    ] {
        assert!(matches!(
            instantiate(deps.as_mut(), env.clone(), info.clone(), msg),
            Err(ContractError::Std(_))
        ));
    }

    instantiate(
        deps.as_mut(),
        env.clone(),
        info,
        InstantiateMsg {
            admin: admin.to_string(),
            control_centers: vec![center.to_string(), center.to_string()],
        },
    )
    .unwrap();

    let config: ConfigResponse =
        from_json(query(deps.as_ref(), env.clone(), QueryMsg::Config {}).unwrap()).unwrap();
    assert_eq!(config.admin, admin);
    let centers: ControlCentersResponse =
        from_json(query(deps.as_ref(), env, QueryMsg::ControlCenters {}).unwrap()).unwrap();
    assert_eq!(centers.control_centers, vec![center]);
}

#[test]
fn only_admin_can_add_and_remove_control_centers() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let admin = deps.api.addr_make("admin");
    let creator = deps.api.addr_make("creator");
    let center = deps.api.addr_make("center");
    instantiate(
        deps.as_mut(),
        env.clone(),
        message_info(&creator, &[]),
        InstantiateMsg {
            admin: admin.to_string(),
            control_centers: vec![center.to_string()],
        },
    )
    .unwrap();

    for msg in [
        ExecuteMsg::UpdateAdmin {
            new_admin: creator.to_string(),
        },
        ExecuteMsg::AddControlCenter {
            address: deps.api.addr_make("another_center").to_string(),
        },
        ExecuteMsg::RemoveControlCenter {
            address: center.to_string(),
        },
    ] {
        assert!(matches!(
            execute(deps.as_mut(), env.clone(), message_info(&creator, &[]), msg),
            Err(ContractError::Unauthorized {})
        ));
    }

    let centers: ControlCentersResponse =
        from_json(query(deps.as_ref(), env, QueryMsg::ControlCenters {}).unwrap()).unwrap();
    assert_eq!(centers.control_centers, vec![center]);
}

#[test]
fn admin_rotation_validates_address_and_transfers_permissions() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let admin = deps.api.addr_make("admin");
    let new_admin = deps.api.addr_make("new_admin");
    let center = deps.api.addr_make("center");
    instantiate(
        deps.as_mut(),
        env.clone(),
        message_info(&admin, &[]),
        InstantiateMsg {
            admin: admin.to_string(),
            control_centers: vec![center.to_string()],
        },
    )
    .unwrap();

    assert!(matches!(
        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&admin, &[]),
            ExecuteMsg::UpdateAdmin {
                new_admin: "invalid".into(),
            },
        ),
        Err(ContractError::Std(_))
    ));
    let config: ConfigResponse =
        from_json(query(deps.as_ref(), env.clone(), QueryMsg::Config {}).unwrap()).unwrap();
    assert_eq!(config.admin, admin);

    let response = execute(
        deps.as_mut(),
        env.clone(),
        message_info(&admin, &[]),
        ExecuteMsg::UpdateAdmin {
            new_admin: new_admin.to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        response.attributes,
        vec![
            cosmwasm_std::attr("action", "update_admin"),
            cosmwasm_std::attr("old_admin", &admin),
            cosmwasm_std::attr("new_admin", &new_admin),
        ]
    );
    let config: ConfigResponse =
        from_json(query(deps.as_ref(), env.clone(), QueryMsg::Config {}).unwrap()).unwrap();
    assert_eq!(config.admin, new_admin);
    let centers: ControlCentersResponse =
        from_json(query(deps.as_ref(), env.clone(), QueryMsg::ControlCenters {}).unwrap()).unwrap();
    assert_eq!(centers.control_centers, vec![center.clone()]);

    for msg in [
        ExecuteMsg::UpdateAdmin {
            new_admin: admin.to_string(),
        },
        ExecuteMsg::AddControlCenter {
            address: center.to_string(),
        },
        ExecuteMsg::RemoveControlCenter {
            address: center.to_string(),
        },
    ] {
        assert!(matches!(
            execute(deps.as_mut(), env.clone(), message_info(&admin, &[]), msg),
            Err(ContractError::Unauthorized {})
        ));
    }

    for msg in [
        ExecuteMsg::RemoveControlCenter {
            address: center.to_string(),
        },
        ExecuteMsg::AddControlCenter {
            address: center.to_string(),
        },
        ExecuteMsg::UpdateAdmin {
            new_admin: admin.to_string(),
        },
    ] {
        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&new_admin, &[]),
            msg,
        )
        .unwrap();
    }
}

#[test]
fn admin_can_add_and_remove_including_last_control_center() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let admin = deps.api.addr_make("admin");
    let info = message_info(&admin, &[]);
    let center = deps.api.addr_make("center");
    instantiate(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        InstantiateMsg {
            admin: admin.to_string(),
            control_centers: vec![],
        },
    )
    .unwrap();

    for msg in [
        ExecuteMsg::AddControlCenter {
            address: "invalid".into(),
        },
        ExecuteMsg::RemoveControlCenter {
            address: "invalid".into(),
        },
    ] {
        assert!(matches!(
            execute(deps.as_mut(), env.clone(), info.clone(), msg),
            Err(ContractError::Std(_))
        ));
    }

    let add = ExecuteMsg::AddControlCenter {
        address: center.to_string(),
    };
    execute(deps.as_mut(), env.clone(), info.clone(), add.clone()).unwrap();
    assert!(matches!(
        execute(deps.as_mut(), env.clone(), info.clone(), add),
        Err(ContractError::AlreadyRegistered { .. })
    ));
    let centers: ControlCentersResponse =
        from_json(query(deps.as_ref(), env.clone(), QueryMsg::ControlCenters {}).unwrap()).unwrap();
    assert_eq!(centers.control_centers, vec![center.clone()]);

    let remove = ExecuteMsg::RemoveControlCenter {
        address: center.to_string(),
    };
    execute(deps.as_mut(), env.clone(), info.clone(), remove.clone()).unwrap();
    assert!(matches!(
        execute(deps.as_mut(), env.clone(), info, remove),
        Err(ContractError::NotRegistered { .. })
    ));
    let centers: ControlCentersResponse =
        from_json(query(deps.as_ref(), env, QueryMsg::ControlCenters {}).unwrap()).unwrap();
    assert_eq!(centers.control_centers, Vec::<Addr>::new());
}
