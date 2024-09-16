/**
* This file was automatically generated by @cosmwasm/ts-codegen@1.11.1.
* DO NOT MODIFY IT BY HAND. Instead, modify the source JSONSchema file,
* and run the @cosmwasm/ts-codegen generate command to regenerate this file.
*/

import { CosmWasmClient, SigningCosmWasmClient, ExecuteResult } from "@cosmjs/cosmwasm-stargate";
import { StdFee } from "@cosmjs/amino";
import { Uint128, Timestamp, Uint64, AllUserLockupsResponse, LockEntryWithPower, LockEntry, Coin, ConstantsResponse, Constants, CurrentRoundResponse, ExecuteMsg, TrancheInfo, ExpiredUserLockupsResponse, InstantiateMsg, ProposalResponse, Proposal, QueryMsg, RoundEndResponse, RoundProposalsResponse, RoundTotalVotingPowerResponse, TopNProposalsResponse, TotalLockedTokensResponse, TranchesResponse, Tranche, Decimal, UserVoteResponse, VoteWithPower, UserVotingPowerResponse, Addr, WhitelistAdminsResponse, WhitelistResponse } from "./HydroBase.types";
export interface HydroBaseReadOnlyInterface {
  contractAddress: string;
  constants: () => Promise<ConstantsResponse>;
  tranches: () => Promise<TranchesResponse>;
  allUserLockups: ({
    address,
    limit,
    startFrom
  }: {
    address: string;
    limit: number;
    startFrom: number;
  }) => Promise<AllUserLockupsResponse>;
  expiredUserLockups: ({
    address,
    limit,
    startFrom
  }: {
    address: string;
    limit: number;
    startFrom: number;
  }) => Promise<ExpiredUserLockupsResponse>;
  userVotingPower: ({
    address
  }: {
    address: string;
  }) => Promise<UserVotingPowerResponse>;
  userVote: ({
    address,
    roundId,
    trancheId
  }: {
    address: string;
    roundId: number;
    trancheId: number;
  }) => Promise<UserVoteResponse>;
  currentRound: () => Promise<CurrentRoundResponse>;
  roundEnd: ({
    roundId
  }: {
    roundId: number;
  }) => Promise<RoundEndResponse>;
  roundTotalVotingPower: ({
    roundId
  }: {
    roundId: number;
  }) => Promise<RoundTotalVotingPowerResponse>;
  roundProposals: ({
    limit,
    roundId,
    startFrom,
    trancheId
  }: {
    limit: number;
    roundId: number;
    startFrom: number;
    trancheId: number;
  }) => Promise<RoundProposalsResponse>;
  proposal: ({
    proposalId,
    roundId,
    trancheId
  }: {
    proposalId: number;
    roundId: number;
    trancheId: number;
  }) => Promise<ProposalResponse>;
  topNProposals: ({
    numberOfProposals,
    roundId,
    trancheId
  }: {
    numberOfProposals: number;
    roundId: number;
    trancheId: number;
  }) => Promise<TopNProposalsResponse>;
  whitelist: () => Promise<WhitelistResponse>;
  whitelistAdmins: () => Promise<WhitelistAdminsResponse>;
  totalLockedTokens: () => Promise<TotalLockedTokensResponse>;
  registeredValidatorQueries: () => Promise<RegisteredValidatorQueriesResponse>;
  validatorPowerRatio: ({
    roundId,
    validator
  }: {
    roundId: number;
    validator: string;
  }) => Promise<ValidatorPowerRatioResponse>;
}
export class HydroBaseQueryClient implements HydroBaseReadOnlyInterface {
  client: CosmWasmClient;
  contractAddress: string;
  constructor(client: CosmWasmClient, contractAddress: string) {
    this.client = client;
    this.contractAddress = contractAddress;
    this.constants = this.constants.bind(this);
    this.tranches = this.tranches.bind(this);
    this.allUserLockups = this.allUserLockups.bind(this);
    this.expiredUserLockups = this.expiredUserLockups.bind(this);
    this.userVotingPower = this.userVotingPower.bind(this);
    this.userVote = this.userVote.bind(this);
    this.currentRound = this.currentRound.bind(this);
    this.roundEnd = this.roundEnd.bind(this);
    this.roundTotalVotingPower = this.roundTotalVotingPower.bind(this);
    this.roundProposals = this.roundProposals.bind(this);
    this.proposal = this.proposal.bind(this);
    this.topNProposals = this.topNProposals.bind(this);
    this.whitelist = this.whitelist.bind(this);
    this.whitelistAdmins = this.whitelistAdmins.bind(this);
    this.totalLockedTokens = this.totalLockedTokens.bind(this);
    this.registeredValidatorQueries = this.registeredValidatorQueries.bind(this);
    this.validatorPowerRatio = this.validatorPowerRatio.bind(this);
  }
  constants = async (): Promise<ConstantsResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      constants: {}
    });
  };
  tranches = async (): Promise<TranchesResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      tranches: {}
    });
  };
  allUserLockups = async ({
    address,
    limit,
    startFrom
  }: {
    address: string;
    limit: number;
    startFrom: number;
  }): Promise<AllUserLockupsResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      all_user_lockups: {
        address,
        limit,
        start_from: startFrom
      }
    });
  };
  expiredUserLockups = async ({
    address,
    limit,
    startFrom
  }: {
    address: string;
    limit: number;
    startFrom: number;
  }): Promise<ExpiredUserLockupsResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      expired_user_lockups: {
        address,
        limit,
        start_from: startFrom
      }
    });
  };
  userVotingPower = async ({
    address
  }: {
    address: string;
  }): Promise<UserVotingPowerResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      user_voting_power: {
        address
      }
    });
  };
  userVote = async ({
    address,
    roundId,
    trancheId
  }: {
    address: string;
    roundId: number;
    trancheId: number;
  }): Promise<UserVoteResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      user_vote: {
        address,
        round_id: roundId,
        tranche_id: trancheId
      }
    });
  };
  currentRound = async (): Promise<CurrentRoundResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      current_round: {}
    });
  };
  roundEnd = async ({
    roundId
  }: {
    roundId: number;
  }): Promise<RoundEndResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      round_end: {
        round_id: roundId
      }
    });
  };
  roundTotalVotingPower = async ({
    roundId
  }: {
    roundId: number;
  }): Promise<RoundTotalVotingPowerResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      round_total_voting_power: {
        round_id: roundId
      }
    });
  };
  roundProposals = async ({
    limit,
    roundId,
    startFrom,
    trancheId
  }: {
    limit: number;
    roundId: number;
    startFrom: number;
    trancheId: number;
  }): Promise<RoundProposalsResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      round_proposals: {
        limit,
        round_id: roundId,
        start_from: startFrom,
        tranche_id: trancheId
      }
    });
  };
  proposal = async ({
    proposalId,
    roundId,
    trancheId
  }: {
    proposalId: number;
    roundId: number;
    trancheId: number;
  }): Promise<ProposalResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      proposal: {
        proposal_id: proposalId,
        round_id: roundId,
        tranche_id: trancheId
      }
    });
  };
  topNProposals = async ({
    numberOfProposals,
    roundId,
    trancheId
  }: {
    numberOfProposals: number;
    roundId: number;
    trancheId: number;
  }): Promise<TopNProposalsResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      top_n_proposals: {
        number_of_proposals: numberOfProposals,
        round_id: roundId,
        tranche_id: trancheId
      }
    });
  };
  whitelist = async (): Promise<WhitelistResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      whitelist: {}
    });
  };
  whitelistAdmins = async (): Promise<WhitelistAdminsResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      whitelist_admins: {}
    });
  };
  totalLockedTokens = async (): Promise<TotalLockedTokensResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      total_locked_tokens: {}
    });
  };
  registeredValidatorQueries = async (): Promise<RegisteredValidatorQueriesResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      registered_validator_queries: {}
    });
  };
  validatorPowerRatio = async ({
    roundId,
    validator
  }: {
    roundId: number;
    validator: string;
  }): Promise<ValidatorPowerRatioResponse> => {
    return this.client.queryContractSmart(this.contractAddress, {
      validator_power_ratio: {
        round_id: roundId,
        validator
      }
    });
  };
}
export interface HydroBaseInterface extends HydroBaseReadOnlyInterface {
  contractAddress: string;
  sender: string;
  lockTokens: ({
    lockDuration
  }: {
    lockDuration: number;
  }, fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  refreshLockDuration: ({
    lockDuration,
    lockId
  }: {
    lockDuration: number;
    lockId: number;
  }, fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  unlockTokens: (fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  createProposal: ({
    description,
    title,
    trancheId
  }: {
    description: string;
    title: string;
    trancheId: number;
  }, fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  vote: ({
    proposalId,
    trancheId
  }: {
    proposalId: number;
    trancheId: number;
  }, fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  addAccountToWhitelist: ({
    address
  }: {
    address: string;
  }, fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  removeAccountFromWhitelist: ({
    address
  }: {
    address: string;
  }, fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  updateMaxLockedTokens: ({
    maxLockedTokens
  }: {
    maxLockedTokens: number;
  }, fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  pause: (fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  addTranche: ({
    tranche
  }: {
    tranche: TrancheInfo;
  }, fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  editTranche: ({
    trancheId,
    trancheMetadata,
    trancheName
  }: {
    trancheId: number;
    trancheMetadata?: string;
    trancheName?: string;
  }, fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
  createIcqsForValidators: ({
    validators
  }: {
    validators: string[];
  }, fee?: number | StdFee | "auto", memo?: string, _funds?: Coin[]) => Promise<ExecuteResult>;
}
export class HydroBaseClient extends HydroBaseQueryClient implements HydroBaseInterface {
  client: SigningCosmWasmClient;
  sender: string;
  contractAddress: string;
  constructor(client: SigningCosmWasmClient, sender: string, contractAddress: string) {
    super(client, contractAddress);
    this.client = client;
    this.sender = sender;
    this.contractAddress = contractAddress;
    this.lockTokens = this.lockTokens.bind(this);
    this.refreshLockDuration = this.refreshLockDuration.bind(this);
    this.unlockTokens = this.unlockTokens.bind(this);
    this.createProposal = this.createProposal.bind(this);
    this.vote = this.vote.bind(this);
    this.addAccountToWhitelist = this.addAccountToWhitelist.bind(this);
    this.removeAccountFromWhitelist = this.removeAccountFromWhitelist.bind(this);
    this.updateMaxLockedTokens = this.updateMaxLockedTokens.bind(this);
    this.pause = this.pause.bind(this);
    this.addTranche = this.addTranche.bind(this);
    this.editTranche = this.editTranche.bind(this);
    this.createIcqsForValidators = this.createIcqsForValidators.bind(this);
  }
  lockTokens = async ({
    lockDuration
  }: {
    lockDuration: number;
  }, fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      lock_tokens: {
        lock_duration: lockDuration
      }
    }, fee, memo, _funds);
  };
  refreshLockDuration = async ({
    lockDuration,
    lockId
  }: {
    lockDuration: number;
    lockId: number;
  }, fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      refresh_lock_duration: {
        lock_duration: lockDuration,
        lock_id: lockId
      }
    }, fee, memo, _funds);
  };
  unlockTokens = async (fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      unlock_tokens: {}
    }, fee, memo, _funds);
  };
  createProposal = async ({
    description,
    title,
    trancheId
  }: {
    description: string;
    title: string;
    trancheId: number;
  }, fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      create_proposal: {
        description,
        title,
        tranche_id: trancheId
      }
    }, fee, memo, _funds);
  };
  vote = async ({
    proposalId,
    trancheId
  }: {
    proposalId: number;
    trancheId: number;
  }, fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      vote: {
        proposal_id: proposalId,
        tranche_id: trancheId
      }
    }, fee, memo, _funds);
  };
  addAccountToWhitelist = async ({
    address
  }: {
    address: string;
  }, fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      add_account_to_whitelist: {
        address
      }
    }, fee, memo, _funds);
  };
  removeAccountFromWhitelist = async ({
    address
  }: {
    address: string;
  }, fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      remove_account_from_whitelist: {
        address
      }
    }, fee, memo, _funds);
  };
  updateMaxLockedTokens = async ({
    maxLockedTokens
  }: {
    maxLockedTokens: number;
  }, fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      update_max_locked_tokens: {
        max_locked_tokens: maxLockedTokens
      }
    }, fee, memo, _funds);
  };
  pause = async (fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      pause: {}
    }, fee, memo, _funds);
  };
  addTranche = async ({
    tranche
  }: {
    tranche: TrancheInfo;
  }, fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      add_tranche: {
        tranche
      }
    }, fee, memo, _funds);
  };
  editTranche = async ({
    trancheId,
    trancheMetadata,
    trancheName
  }: {
    trancheId: number;
    trancheMetadata?: string;
    trancheName?: string;
  }, fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      edit_tranche: {
        tranche_id: trancheId,
        tranche_metadata: trancheMetadata,
        tranche_name: trancheName
      }
    }, fee, memo, _funds);
  };
  createIcqsForValidators = async ({
    validators
  }: {
    validators: string[];
  }, fee: number | StdFee | "auto" = "auto", memo?: string, _funds?: Coin[]): Promise<ExecuteResult> => {
    return await this.client.execute(this.sender, this.contractAddress, {
      create_icqs_for_validators: {
        validators
      }
    }, fee, memo, _funds);
  };
}