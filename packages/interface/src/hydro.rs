use cw_orch::interface;

pub use hydro::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg};
pub use hydro::query::QueryMsg;

pub const CONTRACT_ID: &str = "hydro_contract";

#[interface(InstantiateMsg, ExecuteMsg, QueryMsg, MigrateMsg, id = CONTRACT_ID)]
pub struct Hydro;

#[cfg(not(target_arch = "wasm32"))]
use cw_orch::prelude::*;
use neutron_sdk::bindings::msg::NeutronMsg;

#[cfg(not(target_arch = "wasm32"))]
impl<Chain: CwEnv> Uploadable for Hydro<Chain> {
    // Return the path to the wasm file
    fn wasm(_chain: &ChainInfoOwned) -> WasmPath {
        artifacts_dir_from_workspace!()
            .find_wasm_path("hydro")
            .unwrap()
    }
    // Return a CosmWasm contract wrapper
    fn wrapper() -> Box<dyn MockContract<NeutronMsg>> {
        Box::new(
            ContractWrapper::new_with_empty(
                hydro::contract::execute,
                hydro::contract::instantiate,
                hydro::contract::query,
            )
            .with_migrate(hydro::contract::migrate),
        )
    }
}
