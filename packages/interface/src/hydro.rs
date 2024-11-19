use cw_orch::interface;

use hydro::migration::v2_0_1::MigrateMsgV2_0_1;
pub use hydro::msg::{ExecuteMsg, InstantiateMsg};
pub use hydro::query::QueryMsg;

pub const CONTRACT_ID: &str = "hydro_contract";

#[interface(InstantiateMsg, ExecuteMsg, QueryMsg, MigrateMsgV2_0_1, id = CONTRACT_ID)]
pub struct Hydro;

#[cfg(not(target_arch = "wasm32"))]
use cw_orch::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
impl<Chain: CwEnv> Uploadable for Hydro<Chain> {
    // Return the path to the wasm file
    fn wasm(_chain: &ChainInfoOwned) -> WasmPath {
        artifacts_dir_from_workspace!()
            .find_wasm_path("hydro")
            .unwrap()
    }
}
