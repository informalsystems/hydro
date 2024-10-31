use crate::testing_mocks::{mock_dependencies, no_op_grpc_query_mock};
use crate::{
    contract::{execute, instantiate},
    msg::ExecuteMsg,
};
use cosmwasm_std::testing::mock_env;
use cosmwasm_std::Uint128;

#[cfg(test)]
mod tests {
    use cosmwasm_std::coin;

    use crate::{
        msg::LiquidityDeployment,
        state::{Proposal, LIQUIDITY_DEPLOYMENTS_MAP, PROPOSAL_MAP},
        testing::{get_address_as_str, get_default_instantiate_msg, get_message_info},
    };

    use super::*;

    #[derive(Debug)]
    struct AddLiquidityDeploymentTestCase {
        round_id: u64,
        tranche_id: u64,
        proposal_id: u64,
        sender: String,
        expect_error: bool,
    }

    #[test]
    fn test_add_remove_liquidity_deployment() {
        let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
        let admin_address = get_address_as_str(&deps.api, "admin");
        let info = get_message_info(&deps.api, "admin", &[]);
        let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
        instantiate_msg.whitelist_admins = vec![admin_address.clone()];
        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg);
        assert!(res.is_ok(), "{:?}", res);

        // Add a proposal to the store
        let proposal_id = 1;

        let proposal = Proposal {
            round_id: 0,
            tranche_id: 1,
            proposal_id,
            power: Uint128::zero(),
            percentage: Uint128::zero(),
            title: "proposal1".to_string(),
            description: "description1".to_string(),
            minimum_atom_liquidity_request: Uint128::zero(),
            bid_duration: 1,
        };
        PROPOSAL_MAP
            .save(deps.as_mut().storage, (0, 1, proposal_id), &proposal)
            .unwrap();

        // Define test cases
        let test_cases = vec![
            AddLiquidityDeploymentTestCase {
                round_id: 0,
                tranche_id: 1,
                proposal_id,
                sender: "admin".to_string(),
                expect_error: false,
            },
            AddLiquidityDeploymentTestCase {
                round_id: 1,
                tranche_id: 1,
                proposal_id,
                sender: "admin".to_string(),
                expect_error: true, // Round has not started yet
            },
            AddLiquidityDeploymentTestCase {
                round_id: 0,
                tranche_id: 2,
                proposal_id,
                sender: "admin".to_string(),
                expect_error: true, // Tranche does not exist
            },
            AddLiquidityDeploymentTestCase {
                round_id: 0,
                tranche_id: 0,
                proposal_id: 2,
                sender: "admin".to_string(),
                expect_error: true, // Proposal does not exist
            },
            AddLiquidityDeploymentTestCase {
                round_id: 0,
                tranche_id: 1,
                proposal_id,
                sender: "non_admin".to_string(),
                expect_error: true, // Sender is not an admin
            },
        ];

        for case in test_cases {
            // Add or remove the sender from the whitelist admins list
            let info = get_message_info(&deps.api, &case.sender, &[]);
            let add_liquidity_msg = ExecuteMsg::AddLiquidityDeployment {
                round_id: case.round_id,
                tranche_id: case.tranche_id,
                proposal_id: case.proposal_id,
                destinations: vec!["destination1".to_string()],
                deployed_funds: vec![coin(100, "token")],
                funds_before_deployment: vec![coin(200, "token")],
                total_rounds: 10,
                remaining_rounds: 5,
            };

            let res = execute(deps.as_mut(), env.clone(), info.clone(), add_liquidity_msg);

            if case.expect_error {
                assert!(res.is_err(), "Expected error for case: {:#?}", case);
            } else {
                assert!(
                    res.is_ok(),
                    "Expected success for case: {:#?}, error: {:?}",
                    case,
                    res.err()
                );
            }
        }
    }

    #[derive(Debug)]
    struct RemoveLiquidityDeploymentTestCase {
        round_id: u64,
        tranche_id: u64,
        proposal_id: u64,
        sender: String,
        expect_error: bool,
    }

    #[test]
    fn test_remove_liquidity_deployment() {
        let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
        let admin_address = get_address_as_str(&deps.api, "admin");
        let info = get_message_info(&deps.api, "admin", &[]);
        let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
        instantiate_msg.whitelist_admins = vec![admin_address.clone()];
        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg);
        assert!(res.is_ok(), "{:?}", res);

        // Add a proposal and a liquidity deployment to the store
        let proposal_id = 1;
        let round_id = 0;
        let tranche_id = 1;

        let proposal = Proposal {
            round_id,
            tranche_id,
            proposal_id,
            power: Uint128::zero(),
            percentage: Uint128::zero(),
            title: "proposal1".to_string(),
            description: "description1".to_string(),
            bid_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        };
        PROPOSAL_MAP
            .save(
                deps.as_mut().storage,
                (round_id, tranche_id, proposal_id),
                &proposal,
            )
            .unwrap();

        let liquidity_deployment = LiquidityDeployment {
            round_id,
            tranche_id,
            proposal_id,
            destinations: vec!["destination1".to_string()],
            deployed_funds: vec![coin(100, "token")],
            funds_before_deployment: vec![coin(200, "token")],
            total_rounds: 10,
            remaining_rounds: 5,
        };
        LIQUIDITY_DEPLOYMENTS_MAP
            .save(
                deps.as_mut().storage,
                (round_id, tranche_id, proposal_id),
                &liquidity_deployment,
            )
            .unwrap();

        // Define test cases
        let test_cases = vec![
            RemoveLiquidityDeploymentTestCase {
                round_id,
                tranche_id,
                proposal_id,
                sender: "admin".to_string(),
                expect_error: false,
            },
            RemoveLiquidityDeploymentTestCase {
                round_id,
                tranche_id,
                proposal_id: 2,
                sender: "admin".to_string(),
                expect_error: true, // Deployment does not exist
            },
            RemoveLiquidityDeploymentTestCase {
                round_id,
                tranche_id: 2,
                proposal_id,
                sender: "admin".to_string(),
                expect_error: true, // Deployment does not exist
            },
            RemoveLiquidityDeploymentTestCase {
                round_id,
                tranche_id,
                proposal_id,
                sender: "non_admin".to_string(),
                expect_error: true, // Sender is not an admin
            },
        ];

        for case in test_cases {
            // Remove the sender from the whitelist admins list if necessary
            let info = get_message_info(&deps.api, &case.sender, &[]);
            let remove_liquidity_msg = ExecuteMsg::RemoveLiquidityDeployment {
                round_id: case.round_id,
                tranche_id: case.tranche_id,
                proposal_id: case.proposal_id,
            };

            let res = execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                remove_liquidity_msg,
            );

            if case.expect_error {
                assert!(res.is_err(), "Expected error for case: {:#?}", case);
            } else {
                assert!(
                    res.is_ok(),
                    "Expected success for case: {:#?}, error: {:?}",
                    case,
                    res.err()
                );
            }
        }
    }
}
