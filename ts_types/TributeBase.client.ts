/**
* This file was automatically generated by @cosmwasm/ts-codegen@1.11.1.
* DO NOT MODIFY IT BY HAND. Instead, modify the source JSONSchema file,
* and run the @cosmwasm/ts-codegen generate command to regenerate this file.
*/

import { CosmWasmClient, SigningCosmWasmClient, ExecuteResult } from "@cosmjs/cosmwasm-stargate";
import { StdFee } from "@cosmjs/amino";
import { Addr, ConfigResponse, Config, ExecuteMsg, InstantiateMsg, Uint128, ProposalTributesResponse, Tribute, Coin, QueryMsg } from "./TributeBase.types";
export interface TributeBaseReadOnlyInterface {
  contractAddress: string;
  config: () => Promise<ConfigResponse>;
  proposalTributes: ({
    limit,
    proposalId,
    roundId,
    startFrom
  }: {
    limit: number;
    proposalId: number;
    roundId: number;
    startFrom: number;
  }) => Promise<ProposalTributesResponse>;
  historicalTributeClaims: ({
    limit,
    startFrom,
    userAddress
  }: {
    limit: number;
    startFrom: number;
    userAddress: string;
  }) => Promise<HistoricalTributeClaimsResponse>;
  roundTributes: ({
    limit,
    roundId,
    startFrom
  }: {
    limit: number;
    roundId: number;
    startFrom: number;
  }) => Promise<RoundTributesResponse>;
  outstandingTributeClaims: ({
    limit,
    roundId,
    startFrom,
    trancheId,
    userAddress
  }: {
    limit: number;
    roundId: number;
    startFrom: number;
    trancheId: number;
    userAddress: string;
  }) => Promise<OutstandingTributeClaimsResponse>;
}
export class TributeBaseQueryClient implements TributeBaseReadOnlyInterface {
  client: CosmWasmClient;
  contractAddress: string;
  constructor(client: CosmWasmClient, contractAddress: string) {
    this.client = client;
    this.contractAddress = contractAddress;
    this.config = this.config.bind(this);
    this.proposalTributes = this.proposalTributes.bind(this);
    this.historicalTributeClaims = this.historicalTributeClaims.bind(this);
    this.roundTributes = this.roundTributes.bind(this);
    this.outstandingTributeClaims = this.outstandingTributeClaims.bind(this);
  }
  config = async (): Promise<ConfigResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      config: {}
    });
  };
  proposalTributes = async ({
    limit,
    proposalId,
    roundId,
    startFrom
  }: {
    limit: number;
    proposalId: number;
    roundId: number;
    startFrom: number;
  }): Promise<ProposalTributesResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      proposal_tributes: {
        limit,
        proposal_id: proposalId,
        round_id: roundId,
        start_from: startFrom
      }
    });
  };
  historicalTributeClaims = async ({
    limit,
    startFrom,
    userAddress
  }: {
    limit: number;
    startFrom: number;
    userAddress: string;
  }): Promise<HistoricalTributeClaimsResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      historical_tribute_claims: {
        limit,
        start_from: startFrom,
        user_address: userAddress
      }
    });
  };
  roundTributes = async ({
    limit,
    roundId,
    startFrom
  }: {
    limit: number;
    roundId: number;
    startFrom: number;
  }): Promise<RoundTributesResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      round_tributes: {
        limit,
        round_id: roundId,
        start_from: startFrom
      }
    });
  };
  outstandingTributeClaims = async ({
    limit,
    roundId,
    startFrom,
    trancheId,
    userAddress
  }: {
    limit: number;
    roundId: number;
    startFrom: number;
    trancheId: number;
    userAddress: string;
  }): Promise<OutstandingTributeClaimsResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      outstanding_tribute_claims: {
        limit,
        round_id: roundId,
        start_from: startFrom,
        tranche_id: trancheId,
        user_address: userAddress
      }
    });
  };
}
export interface TributeBaseInterface extends TributeBaseReadOnlyInterface {
  contractAddress: string;
  sender: string;
  addTribute: ({
    proposalId,
    roundId,
    trancheId
  }: {
    proposalId: number;
    roundId: number;
    trancheId: number;
  }, fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  claimTribute: ({
    roundId,
    trancheId,
    tributeId,
    voterAddress
  }: {
    roundId: number;
    trancheId: number;
    tributeId: number;
    voterAddress: string;
  }, fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  refundTribute: ({
    proposalId,
    roundId,
    trancheId,
    tributeId
  }: {
    proposalId: number;
    roundId: number;
    trancheId: number;
    tributeId: number;
  }, fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
}
export class TributeBaseClient extends TributeBaseQueryClient implements TributeBaseInterface {
  client: SigningCosmWasmClient;
  sender: string;
  contractAddress: string;
  constructor(client: SigningCosmWasmClient, sender: string, contractAddress: string) {
    super(client, contractAddress);
    this.client = client;
    this.sender = sender;
    this.contractAddress = contractAddress;
    this.addTribute = this.addTribute.bind(this);
    this.claimTribute = this.claimTribute.bind(this);
    this.refundTribute = this.refundTribute.bind(this);
  }
  addTribute = async ({
    proposalId,
    roundId,
    trancheId
  }: {
    proposalId: number;
    roundId: number;
    trancheId: number;
  }, fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      add_tribute: {
        proposal_id: proposalId,
        round_id: roundId,
        tranche_id: trancheId
      }
    }, fee, memo, _funds);
  };
  claimTribute = async ({
    roundId,
    trancheId,
    tributeId,
    voterAddress
  }: {
    roundId: number;
    trancheId: number;
    tributeId: number;
    voterAddress: string;
  }, fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      claim_tribute: {
        round_id: roundId,
        tranche_id: trancheId,
        tribute_id: tributeId,
        voter_address: voterAddress
      }
    }, fee, memo, _funds);
  };
  refundTribute = async ({
    proposalId,
    roundId,
    trancheId,
    tributeId
  }: {
    proposalId: number;
    roundId: number;
    trancheId: number;
    tributeId: number;
  }, fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      refund_tribute: {
        proposal_id: proposalId,
        round_id: roundId,
        tranche_id: trancheId,
        tribute_id: tributeId
      }
    }, fee, memo, _funds);
  };
}