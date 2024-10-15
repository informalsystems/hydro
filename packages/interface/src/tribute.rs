use cw_orch::interface;

pub use tribute::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg};
pub use tribute::query::QueryMsg;

pub const CONTRACT_ID: &str = "tribute_contract";

#[interface(InstantiateMsg, ExecuteMsg, QueryMsg, MigrateMsg, id = CONTRACT_ID)]
pub struct Tribute;

#[cfg(not(target_arch = "wasm32"))]
use cw_orch::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
impl<Chain: CwEnv> Uploadable for Tribute<Chain> {
    // Return the path to the wasm file
    fn wasm(_chain: &ChainInfoOwned) -> WasmPath {
        artifacts_dir_from_workspace!()
            .find_wasm_path("tribute")
            .unwrap()
    }
    // Return a CosmWasm contract wrapper
    fn wrapper() -> Box<dyn MockContract<Empty>> {
        Box::new(
            ContractWrapper::new_with_empty(
                tribute::contract::execute,
                tribute::contract::instantiate,
                tribute::contract::query,
            )
            .with_migrate(tribute::migrate::migrate),
        )
    }
}
