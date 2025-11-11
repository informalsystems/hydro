use std::collections::HashMap;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_vec, Decimal, Deps, DepsMut, Env, Order, Reply, Response, StdError, StdResult, SubMsg,
    WasmMsg,
};
use interface::{
    hydro::TokenGroupRatioChange,
    lsm::ValidatorInfo,
    token_info_provider::{DenomInfoResponse, TokenInfoProviderQueryMsg, ValidatorsInfoResponse},
    utils::extract_response_msg_bytes_from_reply_msg,
};

use crate::{
    contract::compute_current_round_id,
    error::{new_generic_error, ContractError},
    lsm_integration::resolve_validator_from_denom,
    msg::{ReplyPayload, TokenInfoProviderInstantiateMsg},
    score_keeper::apply_token_groups_ratio_changes,
    state::{Constants, TOKEN_INFO_PROVIDERS},
    utils::load_current_constants,
};

pub const BASE_TOKEN_INFO_PROVIDER_ID: &str = "base_token_info_provider";

// This structure is a wrapper around supported token information providers and has the following responsibilities:
//  - Validation of token denoms provided by users trying to lock tokens in the contract.
//    The validate_denom() function returns the token group that the provided input denom belongs to,
//    or an error if it doesn't belong to any known group. Examples of token groups are:
//    stATOM, dATOM, cosmosvaloper{VALIDATOR_ADDRESS}, etc.
//  - Obtaining the ratio of the provided token group ID to the base token. The get_token_group_ratio()
//    function will return the ratio between the provided token group ID and the base token, or zero
//    if the token group ID isn't known at the given time. This can happen if (for LSM tokens) some validator
//    dropped out of the top N, or (for the derivatives) if locking of some derivative token was disabled
//    after the given token had already been locked in the contract by some users.
//  - Obtaining the instance of the TokenInfoProvider capable of handling LSM tokens, if the contract
//    supports locking of such tokens.
pub struct TokenManager {
    pub token_info_providers: Vec<TokenInfoProvider>,
}

impl TokenManager {
    pub fn new(deps: &Deps) -> Self {
        Self {
            token_info_providers: get_all_token_info_providers(deps),
        }
    }
}

impl TokenManager {
    pub fn validate_denom(
        &mut self,
        deps: &Deps,
        round_id: u64,
        denom: String,
    ) -> StdResult<String> {
        let mut errors = vec![];

        for provider in self.token_info_providers.iter_mut() {
            match provider.resolve_denom(deps, round_id, denom.clone()) {
                Ok(denom_group_id) => return Ok(denom_group_id),
                Err(err) => errors.push(err),
            }
        }

        // If there is only one token information provider, return the specific error.
        if errors.len() == 1 {
            return Err(errors.pop().unwrap());
        }

        Err(StdError::generic_err(format!(
            "Token with denom {denom} can not be locked in Hydro."
        )))
    }

    pub fn get_token_group_ratio(
        &mut self,
        deps: &Deps,
        round_id: u64,
        token_group_id: String,
    ) -> StdResult<Decimal> {
        for provider in self.token_info_providers.iter_mut() {
            if let Ok(token_ratio) =
                provider.get_token_group_ratio(deps, round_id, token_group_id.clone())
            {
                return Ok(token_ratio);
            }
        }

        Ok(Decimal::zero())
    }

    pub fn get_token_denom_ratio(&mut self, deps: &Deps, round_id: u64, denom: String) -> Decimal {
        let token_group_id = match self.validate_denom(deps, round_id, denom) {
            Err(_) => return Decimal::zero(),
            Ok(token_group_id) => token_group_id,
        };

        match self.get_token_group_ratio(deps, round_id, token_group_id) {
            Err(_) => Decimal::zero(),
            Ok(token_group_ratio) => token_group_ratio,
        }
    }

    pub fn get_lsm_token_info_provider(&self) -> Option<TokenInfoProviderLSM> {
        for token_info_provider in &self.token_info_providers {
            if let TokenInfoProvider::LSM(token_info_provider) = token_info_provider {
                return Some(token_info_provider.clone());
            }
        }

        None
    }

    pub fn get_base_token_info_provider(&self) -> Option<TokenInfoProviderBase> {
        for token_info_provider in &self.token_info_providers {
            if let TokenInfoProvider::Base(token_info_provider) = token_info_provider {
                return Some(token_info_provider.clone());
            }
        }

        None
    }
}

// This enum defines possible variants of token information providers. Instances of the enum are saved
// in the storage and loaded in transactions/queries execution when we need to validate provided token
// denoms or obtain their ratio to the base token. Having different variants (instead of just saving
// the contract address) allows us to perform different queries and handle caching of retrieved data
// in different ways, depending on the token information provider type. Note that CosmWasm can't store
// traits in the storage, so we need to use the enum.
#[cw_serde]
pub enum TokenInfoProvider {
    #[serde(rename = "lsm")]
    LSM(TokenInfoProviderLSM),
    Base(TokenInfoProviderBase),
    Derivative(TokenInfoProviderDerivative),
}

impl TokenInfoProvider {
    pub fn resolve_denom(
        &mut self,
        deps: &Deps,
        round_id: u64,
        denom: String,
    ) -> StdResult<String> {
        match self {
            TokenInfoProvider::LSM(provider) => provider.resolve_denom(deps, round_id, denom),
            TokenInfoProvider::Base(provider) => provider.resolve_denom(deps, round_id, denom),
            TokenInfoProvider::Derivative(provider) => {
                provider.resolve_denom(deps, round_id, denom)
            }
        }
    }

    pub fn get_token_group_ratio(
        &mut self,
        deps: &Deps,
        round_id: u64,
        token_group_id: String,
    ) -> StdResult<Decimal> {
        match self {
            TokenInfoProvider::LSM(provider) => {
                provider.get_token_group_ratio(deps, round_id, token_group_id)
            }
            TokenInfoProvider::Base(provider) => {
                provider.get_token_group_ratio(deps, round_id, token_group_id)
            }
            TokenInfoProvider::Derivative(provider) => {
                provider.get_token_group_ratio(deps, round_id, token_group_id)
            }
        }
    }

    pub fn get_all_token_group_ratios(
        &mut self,
        deps: &Deps,
        round_id: u64,
    ) -> StdResult<HashMap<String, Decimal>> {
        match self {
            TokenInfoProvider::LSM(provider) => provider.get_all_token_group_ratios(deps, round_id),
            TokenInfoProvider::Base(provider) => {
                provider.get_all_token_group_ratios(deps, round_id)
            }
            TokenInfoProvider::Derivative(provider) => {
                provider.get_all_token_group_ratios(deps, round_id)
            }
        }
    }
}

#[cw_serde]
pub struct TokenInfoProviderDerivative {
    pub contract: String,
    pub cache: HashMap<u64, DenomInfoResponse>,
}

impl TokenInfoProviderDerivative {
    pub fn resolve_denom(
        &mut self,
        deps: &Deps,
        round_id: u64,
        denom: String,
    ) -> StdResult<String> {
        let denom_info = self.get_denom_info_with_caching(deps, round_id)?;

        match denom_info.denom == denom {
            true => {
                if denom_info.ratio.is_zero() {
                    Err(StdError::generic_err(format!(
                        "Token ratio not available for round: {round_id}"
                    )))
                } else {
                    Ok(denom_info.token_group_id.clone())
                }
            }
            false => Err(StdError::generic_err(
                "Input denom doesn't match the expected derivative token denom",
            )),
        }
    }

    pub fn get_token_group_ratio(
        &mut self,
        deps: &Deps,
        round_id: u64,
        token_group_id: String,
    ) -> StdResult<Decimal> {
        let denom_info = self.get_denom_info_with_caching(deps, round_id)?;

        match denom_info.token_group_id == token_group_id {
            true => Ok(denom_info.ratio),
            false => Err(StdError::generic_err(
                "Input token group ID doesn't match expected token group ID.",
            )),
        }
    }

    pub fn get_all_token_group_ratios(
        &mut self,
        deps: &Deps,
        round_id: u64,
    ) -> StdResult<HashMap<String, Decimal>> {
        let denom_info = self.get_denom_info_with_caching(deps, round_id)?;

        Ok(HashMap::from([(
            denom_info.token_group_id,
            denom_info.ratio,
        )]))
    }

    fn get_denom_info_with_caching(
        &mut self,
        deps: &Deps,
        round_id: u64,
    ) -> StdResult<DenomInfoResponse> {
        Ok(match self.cache.get(&round_id) {
            Some(cache) => cache.clone(),
            None => {
                let denom_info_resp: DenomInfoResponse = deps.querier.query_wasm_smart(
                    self.contract.clone(),
                    &TokenInfoProviderQueryMsg::DenomInfo { round_id },
                )?;

                self.cache.insert(round_id, denom_info_resp.clone());

                denom_info_resp
            }
        })
    }
}

#[cw_serde]
pub struct TokenInfoProviderLSM {
    pub contract: String,
    // Validators cached per round ID
    pub cache: HashMap<u64, HashMap<String, ValidatorInfo>>,
    // We need to keep the channel ID in order to be able to resolve the LSM IBC denoms
    // within the Hydro contract. Only if the denom is identified as a valid LSM denom,
    // we will query the LSM Token Info Provider to fetch the round validators.
    pub hub_transfer_channel_id: String,
}

impl TokenInfoProviderLSM {
    // Returns OK if the denom is a valid IBC denom representing LSM
    // tokenized share transferred directly from the Cosmos Hub
    // of a validator that is also among the top max_validators validators
    // for the given round, and returns the address of that validator.
    pub fn resolve_denom(
        &mut self,
        deps: &Deps,
        round_id: u64,
        denom: String,
    ) -> StdResult<String> {
        let validator = resolve_validator_from_denom(deps, &self.hub_transfer_channel_id, denom)?;
        let round_validators = self.get_all_round_validators_with_caching(deps, round_id)?;

        round_validators
            .get(&validator)
            .map(|validator_info| validator_info.address.clone())
            .ok_or_else(|| {
                StdError::generic_err(format!(
                    "Validator {validator} is not present; possibly they are not part of the top N validators by delegated tokens",
                ))
        })
    }

    // Returns true if denom is a valid LSM IBC denom.
    // Note: it is purely checking the denom, and does not check whether the validator exists/is active
    pub fn is_lsm_denom(&self, deps: &Deps, denom: String) -> bool {
        let result = resolve_validator_from_denom(deps, &self.hub_transfer_channel_id, denom);
        result.is_ok()
    }

    pub fn get_token_group_ratio(
        &mut self,
        deps: &Deps,
        round_id: u64,
        token_group_id: String,
    ) -> StdResult<Decimal> {
        let round_validators = self.get_all_round_validators_with_caching(deps, round_id)?;

        round_validators
            .get(&token_group_id)
            .map(|validator_info| validator_info.power_ratio)
            .ok_or_else(|| {
                StdError::generic_err(format!(
                    "Input token group ID {token_group_id} doesn't match any of the round validators."
                ))
            })
    }

    pub fn get_all_token_group_ratios(
        &mut self,
        deps: &Deps,
        round_id: u64,
    ) -> StdResult<HashMap<String, Decimal>> {
        Ok(self
            .get_all_round_validators_with_caching(deps, round_id)?
            .values()
            .map(|validator_info| (validator_info.address.clone(), validator_info.power_ratio))
            .collect())
    }

    fn get_all_round_validators_with_caching(
        &mut self,
        deps: &Deps,
        round_id: u64,
    ) -> StdResult<HashMap<String, ValidatorInfo>> {
        let validators_info = match self.cache.get(&round_id) {
            Some(cache) => cache.clone(),
            None => {
                let validators_info: ValidatorsInfoResponse = deps.querier.query_wasm_smart(
                    self.contract.clone(),
                    &TokenInfoProviderQueryMsg::ValidatorsInfo { round_id },
                )?;

                self.cache
                    .insert(round_id, validators_info.validators.clone());

                validators_info.validators
            }
        };

        Ok(validators_info)
    }
}

#[cw_serde]
pub struct TokenInfoProviderBase {
    pub token_group_id: String,
    pub denom: String,
    pub ratio: Decimal,
}

impl TokenInfoProviderBase {
    // Returns OK if the denom is same as the denom of the base Hydro token (e.g. ATOM, OSMO, etc.).
    pub fn resolve_denom(
        &mut self,
        _deps: &Deps,
        _round_id: u64,
        denom: String,
    ) -> StdResult<String> {
        if self.denom != denom {
            return Err(StdError::generic_err(format!(
                "Mismatched denom: expected {}, got {}",
                self.denom, denom
            )));
        }
        Ok(self.token_group_id.clone())
    }

    pub fn get_token_group_ratio(
        &mut self,
        _deps: &Deps,
        _round_id: u64,
        token_group_id: String,
    ) -> StdResult<Decimal> {
        if self.token_group_id != token_group_id {
            return Err(StdError::generic_err(
                "Input token group ID doesn't match expected token group ID.",
            ));
        }
        Ok(self.ratio)
    }

    pub fn get_all_token_group_ratios(
        &mut self,
        _deps: &Deps,
        _round_id: u64,
    ) -> StdResult<HashMap<String, Decimal>> {
        Ok(HashMap::from_iter([(
            self.token_group_id.clone(),
            self.ratio,
        )]))
    }
}

// This function builds token info providers from the given list of instantiation messages.
// The function prepares SubMsgs that will instantiate the token info provider smart contracts.
// The function is used during the Hydro contract instantiation, as well as during the execute
// action that adds a new token info provider.
#[allow(clippy::type_complexity)]
pub fn add_token_info_providers(
    deps: &mut DepsMut,
    token_info_provider_msgs: Vec<TokenInfoProviderInstantiateMsg>,
) -> Result<(Vec<SubMsg>, Option<TokenInfoProvider>), ContractError> {
    let token_manager = TokenManager::new(&deps.as_ref());
    let mut token_info_provider_num = token_manager.token_info_providers.len();
    let mut found_lsm_provider = token_manager.get_lsm_token_info_provider().is_some();
    let mut found_base_provider = token_manager.get_base_token_info_provider().is_some();

    let mut submsgs = vec![];
    let mut base_provider = None;

    for token_info_provider_msg in token_info_provider_msgs {
        match token_info_provider_msg {
            TokenInfoProviderInstantiateMsg::LSM {
                code_id,
                msg,
                label,
                admin,
                hub_transfer_channel_id,
            } => {
                if found_lsm_provider {
                    return Err(new_generic_error(
                        "Only one LSM token info provider can be used.",
                    ));
                }

                let token_info_provider = TokenInfoProvider::LSM(TokenInfoProviderLSM {
                    contract: String::new(),
                    cache: HashMap::new(),
                    hub_transfer_channel_id,
                });

                let submsg: SubMsg = SubMsg::reply_on_success(
                    WasmMsg::Instantiate {
                        admin,
                        code_id,
                        msg,
                        funds: vec![],
                        label,
                    },
                    0,
                )
                .with_payload(to_json_vec(
                    &ReplyPayload::InstantiateTokenInfoProvider(token_info_provider),
                )?);

                submsgs.push(submsg);

                found_lsm_provider = true;
                token_info_provider_num += 1;
            }
            TokenInfoProviderInstantiateMsg::Base {
                token_group_id,
                denom,
            } => {
                if found_base_provider {
                    return Err(new_generic_error(
                        "Only one Base token info provider can be used.",
                    ));
                }

                let base_token_info_provider = TokenInfoProvider::Base(TokenInfoProviderBase {
                    token_group_id,
                    denom,
                    ratio: Decimal::one(),
                });

                TOKEN_INFO_PROVIDERS.save(
                    deps.storage,
                    BASE_TOKEN_INFO_PROVIDER_ID.to_string(),
                    &base_token_info_provider,
                )?;

                base_provider = Some(base_token_info_provider);
                found_base_provider = true;
                token_info_provider_num += 1;
            }
            TokenInfoProviderInstantiateMsg::TokenInfoProviderContract {
                code_id,
                msg,
                label,
                admin,
            } => {
                // Create token info provider with empty contract address that will be attached
                // as a SubMsg paylod and updated with newly instantiated contract address once
                // we receive the result of the instantiate SubMsg.
                let token_info_provider =
                    TokenInfoProvider::Derivative(TokenInfoProviderDerivative {
                        contract: String::new(),
                        cache: HashMap::new(),
                    });

                let submsg: SubMsg = SubMsg::reply_on_success(
                    WasmMsg::Instantiate {
                        admin,
                        code_id,
                        msg,
                        funds: vec![],
                        label,
                    },
                    0,
                )
                .with_payload(to_json_vec(
                    &ReplyPayload::InstantiateTokenInfoProvider(token_info_provider),
                )?);

                submsgs.push(submsg);
                token_info_provider_num += 1;
            }
        }
    }

    if token_info_provider_num == 0 {
        return Err(new_generic_error(
            "At least one token info provider must be specifed.",
        ));
    }

    Ok((submsgs, base_provider))
}

pub fn token_manager_handle_submsg_reply(
    mut deps: DepsMut,
    env: &Env,
    token_info_provider: TokenInfoProvider,
    msg: Reply,
) -> Result<Response, ContractError> {
    let response_bytes = &extract_response_msg_bytes_from_reply_msg(&msg)?;
    let instantiate_msg_response = cw_utils::parse_instantiate_response_data(response_bytes)
        .map_err(|e| StdError::generic_err(format!("failed to parse reply message: {e:?}")))?;

    let mut token_info_provider = match token_info_provider {
        TokenInfoProvider::LSM(mut token_info_provider) => {
            token_info_provider.contract = instantiate_msg_response.contract_address.clone();

            TokenInfoProvider::LSM(token_info_provider)
        }
        TokenInfoProvider::Derivative(mut token_info_provider) => {
            token_info_provider.contract = instantiate_msg_response.contract_address.clone();

            TokenInfoProvider::Derivative(token_info_provider)
        }
        TokenInfoProvider::Base(_) => {
            return Err(new_generic_error(
                "Base token info provider type not expected in reply().",
            ))
        }
    };

    TOKEN_INFO_PROVIDERS.save(
        deps.storage,
        instantiate_msg_response.contract_address,
        &token_info_provider,
    )?;

    let constants = load_current_constants(&deps.as_ref(), env)?;

    // This function gets executed both on contract instantiation and a new token info provider addition.
    // If the first round hasn't started yet, which can happen during contract instantiation, there are no
    // proposals and rounds whose powers should be updated. Also, the handle_token_info_provider_add_remove()
    // function tries to compute current round ID, which would error out if the first round hasn't started,
    // hence the check is introduced here.
    if env.block.time > constants.first_round_start {
        handle_token_info_provider_add_remove(
            &mut deps,
            env,
            &constants,
            &mut token_info_provider,
            |token_group| TokenGroupRatioChange {
                token_group_id: token_group.0.clone(),
                old_ratio: Decimal::zero(),
                new_ratio: *token_group.1,
            },
        )?;
    }

    Ok(Response::default())
}

pub fn handle_token_info_provider_add_remove<T>(
    deps: &mut DepsMut,
    env: &Env,
    constants: &Constants,
    token_info_provider: &mut TokenInfoProvider,
    map_to_token_ratio_change: T,
) -> StdResult<()>
where
    T: Fn((&String, &Decimal)) -> TokenGroupRatioChange,
{
    let current_round_id = compute_current_round_id(env, constants)?;

    let tokens_ratio_changes: Vec<TokenGroupRatioChange> = token_info_provider
        .get_all_token_group_ratios(&deps.as_ref(), current_round_id)?
        .iter()
        .map(map_to_token_ratio_change)
        .collect();

    apply_token_groups_ratio_changes(
        deps.storage,
        env.block.height,
        current_round_id,
        &tokens_ratio_changes,
    )?;

    Ok(())
}

fn get_all_token_info_providers(deps: &Deps) -> Vec<TokenInfoProvider> {
    TOKEN_INFO_PROVIDERS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|provider| match provider {
            Ok(provider) => Some(provider.1),
            Err(_) => None,
        })
        .collect::<Vec<TokenInfoProvider>>()
}
