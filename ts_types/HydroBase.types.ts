/**
* This file was automatically generated by @cosmwasm/ts-codegen@1.11.1.
* DO NOT MODIFY IT BY HAND. Instead, modify the source JSONSchema file,
* and run the @cosmwasm/ts-codegen generate command to regenerate this file.
*/

export type Uint128 = string;
export type Timestamp = Uint64;
export type Uint64 = string;
export interface AllUserLockupsResponse {
  lockups: LockEntryWithPower[];
}
export interface LockEntryWithPower {
  current_voting_power: Uint128;
  lock_entry: LockEntry;
}
export interface LockEntry {
  funds: Coin;
  lock_end: Timestamp;
  lock_id: number;
  lock_start: Timestamp;
}
export interface Coin {
  amount: Uint128;
  denom: string;
}
export interface ConstantsResponse {
  constants: Constants;
}
export interface Constants {
  first_round_start: Timestamp;
  hub_connection_id: string;
  hub_transfer_channel_id: string;
  icq_update_period: number;
  lock_epoch_length: number;
  max_locked_tokens: number;
  max_validator_shares_participating: number;
  paused: boolean;
  round_length: number;
}
export interface CurrentRoundResponse {
  round_end: Timestamp;
  round_id: number;
}
export type ExecuteMsg = {
  lock_tokens: {
    lock_duration: number;
  };
} | {
  refresh_lock_duration: {
    lock_duration: number;
    lock_id: number;
  };
} | {
  unlock_tokens: {};
} | {
  create_proposal: {
    description: string;
    title: string;
    tranche_id: number;
  };
} | {
  vote: {
    proposal_id: number;
    tranche_id: number;
  };
} | {
  add_account_to_whitelist: {
    address: string;
  };
} | {
  remove_account_from_whitelist: {
    address: string;
  };
} | {
  update_max_locked_tokens: {
    max_locked_tokens: number;
  };
} | {
  pause: {};
} | {
  add_tranche: {
    tranche: TrancheInfo;
  };
} | {
  edit_tranche: {
    tranche_id: number;
    tranche_metadata?: string | null;
    tranche_name?: string | null;
  };
} | {
  create_icqs_for_validators: {
    validators: string[];
  };
};
export interface TrancheInfo {
  metadata: string;
  name: string;
}
export interface ExpiredUserLockupsResponse {
  lockups: LockEntry[];
}
export interface InstantiateMsg {
  first_round_start: Timestamp;
  hub_connection_id: string;
  hub_transfer_channel_id: string;
  icq_update_period: number;
  initial_whitelist: string[];
  lock_epoch_length: number;
  max_locked_tokens: Uint128;
  max_validator_shares_participating: number;
  round_length: number;
  tranches: TrancheInfo[];
  whitelist_admins: string[];
}
export interface ProposalResponse {
  proposal: Proposal;
}
export interface Proposal {
  description: string;
  percentage: Uint128;
  power: Uint128;
  proposal_id: number;
  round_id: number;
  title: string;
  tranche_id: number;
}
export type QueryMsg = {
  constants: {};
} | {
  tranches: {};
} | {
  all_user_lockups: {
    address: string;
    limit: number;
    start_from: number;
  };
} | {
  expired_user_lockups: {
    address: string;
    limit: number;
    start_from: number;
  };
} | {
  user_voting_power: {
    address: string;
  };
} | {
  user_vote: {
    address: string;
    round_id: number;
    tranche_id: number;
  };
} | {
  current_round: {};
} | {
  round_end: {
    round_id: number;
  };
} | {
  round_total_voting_power: {
    round_id: number;
  };
} | {
  round_proposals: {
    limit: number;
    round_id: number;
    start_from: number;
    tranche_id: number;
  };
} | {
  proposal: {
    proposal_id: number;
    round_id: number;
    tranche_id: number;
  };
} | {
  top_n_proposals: {
    number_of_proposals: number;
    round_id: number;
    tranche_id: number;
  };
} | {
  whitelist: {};
} | {
  whitelist_admins: {};
} | {
  total_locked_tokens: {};
} | {
  registered_validator_queries: {};
} | {
  validator_power_ratio: {
    round_id: number;
    validator: string;
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
  metadata: string;
  name: string;
}
export type Decimal = string;
export interface UserVoteResponse {
  vote: VoteWithPower;
}
export interface VoteWithPower {
  power: Decimal;
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
  whitelist: Addr[];
}