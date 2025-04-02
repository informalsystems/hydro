/**
* This file was automatically generated by @cosmwasm/ts-codegen@1.12.0.
* DO NOT MODIFY IT BY HAND. Instead, modify the source JSONSchema file,
* and run the @cosmwasm/ts-codegen generate command to regenerate this file.
*/

import { CosmWasmClient } from "@cosmjs/cosmwasm-stargate";
import { Addr, ConfigResponse, Config, Decimal, DenomInfoResponse, ExecuteMsg, InstantiateMsg, QueryMsg } from "./STTokenInfoProviderBase.types";
export interface STTokenInfoProviderBaseReadOnlyInterface {
  contractAddress: string;
  config: () => Promise<ConfigResponse>;
  denomInfo: ({
    roundId
  }: {
    roundId: number;
  }) => Promise<DenomInfoResponse>;
}
export class STTokenInfoProviderBaseQueryClient implements STTokenInfoProviderBaseReadOnlyInterface {
  client: CosmWasmClient;
  contractAddress: string;
  constructor(client: CosmWasmClient, contractAddress: string) {
    this.client = client;
    this.contractAddress = contractAddress;
    this.config = this.config.bind(this);
    this.denomInfo = this.denomInfo.bind(this);
  }
  config = async (): Promise<ConfigResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      config: {}
    });
  };
  denomInfo = async ({
    roundId
  }: {
    roundId: number;
  }): Promise<DenomInfoResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      denom_info: {
        round_id: roundId
      }
    });
  };
}