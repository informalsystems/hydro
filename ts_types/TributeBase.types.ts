/**
* This file was automatically generated by @cosmwasm/ts-codegen@1.11.1.
* DO NOT MODIFY IT BY HAND. Instead, modify the source JSONSchema file,
* and run the @cosmwasm/ts-codegen generate command to regenerate this file.
*/

export type Decimal = string;
export type Addr = string;
export interface ConfigResponse {
  config: Config;
}
export interface Config {
  community_pool_config: CommunityPoolTaxConfig;
  hydro_contract: Addr;
  top_n_props_count: number;
}
export interface CommunityPoolTaxConfig {
  bucket_address: string;
  channel_id: string;
  tax_percent: Decimal;
}
export type ExecuteMsg = {
  add_tribute: {
    proposal_id: number;
    tranche_id: number;
  };
} | {
  claim_tribute: {
    round_id: number;
    tranche_id: number;
    tribute_id: number;
    voter_address: string;
  };
} | {
  refund_tribute: {
    proposal_id: number;
    round_id: number;
    tranche_id: number;
    tribute_id: number;
  };
} | {
  claim_community_pool_tribute: {
    round_id: number;
    tranche_id: number;
  };
};
export interface InstantiateMsg {
  community_pool_config: CommunityPoolTaxConfig;
  hydro_contract: string;
  top_n_props_count: number;
}
export type Uint128 = string;
export interface ProposalTributesResponse {
  tributes: Tribute[];
}
export interface Tribute {
  depositor: Addr;
  funds: Coin;
  proposal_id: number;
  refunded: boolean;
  round_id: number;
  tranche_id: number;
  tribute_id: number;
}
export interface Coin {
  amount: Uint128;
  denom: string;
}
export type QueryMsg = {
  config: {};
} | {
  proposal_tributes: {
    limit: number;
    proposal_id: number;
    round_id: number;
    start_from: number;
    tranche_id: number;
  };
};