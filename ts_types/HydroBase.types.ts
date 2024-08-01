/**
* This file was automatically generated by @cosmwasm/ts-codegen@1.11.1.
* DO NOT MODIFY IT BY HAND. Instead, modify the source JSONSchema file,
* and run the @cosmwasm/ts-codegen generate command to regenerate this file.
*/

export type Uint128 = string;
export type Timestamp = Uint64;
export type Uint64 = string;
export interface AllUserLockupsResponse {
  lockups: LockEntry[];
}
export interface LockEntry {
  funds: Coin;
  lock_end: Timestamp;
  lock_start: Timestamp;
}
export interface Coin {
  amount: Uint128;
  denom: string;
  [k: string]: unknown;
}
export interface ConstantsResponse {
  constants: Constants;
}
export interface Constants {
  denom: string;
  first_round_start: Timestamp;
  lock_epoch_length: number;
  max_locked_tokens: number;
  paused: boolean;
  round_length: number;
}
export interface CurrentRoundResponse {
  round_id: number;
}
export type ExecuteMsg = {
  lock_tokens: {
    lock_duration: number;
    [k: string]: unknown;
  };
} | {
  refresh_lock_duration: {
    lock_duration: number;
    lock_id: number;
    [k: string]: unknown;
  };
} | {
  unlock_tokens: {
    [k: string]: unknown;
  };
} | {
  create_proposal: {
    covenant_params: CovenantParams;
    description: string;
    title: string;
    tranche_id: number;
    [k: string]: unknown;
  };
} | {
  vote: {
    proposal_id: number;
    tranche_id: number;
    [k: string]: unknown;
  };
} | {
  add_to_whitelist: {
    covenant_params: CovenantParams;
    [k: string]: unknown;
  };
} | {
  remove_from_whitelist: {
    covenant_params: CovenantParams;
    [k: string]: unknown;
  };
} | {
  update_max_locked_tokens: {
    max_locked_tokens: number;
    [k: string]: unknown;
  };
} | {
  pause: {
    [k: string]: unknown;
  };
} | {
  add_tranche: {
    tranche_name: string;
    [k: string]: unknown;
  };
};
export interface CovenantParams {
  funding_destination_name: string;
  outgoing_channel_id: string;
  pool_id: string;
}
export interface ExpiredUserLockupsResponse {
  lockups: LockEntry[];
}
export interface InstantiateMsg {
  denom: string;
  first_round_start: Timestamp;
  initial_whitelist: CovenantParams[];
  lock_epoch_length: number;
  max_locked_tokens: number;
  round_length: number;
  tranches: string[];
  whitelist_admins: string[];
  [k: string]: unknown;
}
export interface ProposalResponse {
  proposal: Proposal;
}
export interface Proposal {
  covenant_params: CovenantParams;
  description: string;
  percentage: Uint128;
  power: Uint128;
  proposal_id: number;
  round_id: number;
  title: string;
  tranche_id: number;
}
export type QueryMsg = {
  constants: {
    [k: string]: unknown;
  };
} | {
  tranches: {
    [k: string]: unknown;
  };
} | {
  all_user_lockups: {
    address: string;
    limit: number;
    start_from: number;
    [k: string]: unknown;
  };
} | {
  expired_user_lockups: {
    address: string;
    limit: number;
    start_from: number;
    [k: string]: unknown;
  };
} | {
  user_voting_power: {
    address: string;
    [k: string]: unknown;
  };
} | {
  user_vote: {
    address: string;
    round_id: number;
    tranche_id: number;
    [k: string]: unknown;
  };
} | {
  current_round: {
    [k: string]: unknown;
  };
} | {
  round_end: {
    round_id: number;
    [k: string]: unknown;
  };
} | {
  round_total_voting_power: {
    round_id: number;
    [k: string]: unknown;
  };
} | {
  round_proposals: {
    limit: number;
    round_id: number;
    start_from: number;
    tranche_id: number;
    [k: string]: unknown;
  };
} | {
  proposal: {
    proposal_id: number;
    round_id: number;
    tranche_id: number;
    [k: string]: unknown;
  };
} | {
  top_n_proposals: {
    number_of_proposals: number;
    round_id: number;
    tranche_id: number;
    [k: string]: unknown;
  };
} | {
  whitelist: {
    [k: string]: unknown;
  };
} | {
  whitelist_admins: {
    [k: string]: unknown;
  };
} | {
  total_locked_tokens: {
    [k: string]: unknown;
  };
};
export interface RoundEndResponse {
  round_end: Timestamp;
}
export interface RoundProposalsResponse {
  proposals: Proposal[];
}
export interface RoundTotalVotingPowerResponse {
  total_voting_power: Uint128;
}
export interface TopNProposalsResponse {
  proposals: Proposal[];
}
export interface TotalLockedTokensResponse {
  total_locked_tokens: number;
}
export interface TranchesResponse {
  tranches: Tranche[];
}
export interface Tranche {
  id: number;
  name: string;
}
export interface UserVoteResponse {
  vote: Vote;
}
export interface Vote {
  power: Uint128;
  prop_id: number;
}
export interface UserVotingPowerResponse {
  voting_power: number;
}
export type Addr = string;
export interface WhitelistAdminsResponse {
  admins: Addr[];
}
export interface WhitelistResponse {
  whitelist: CovenantParams[];
}