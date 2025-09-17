package chainsuite

type Funds struct {
	Denom  string `json:"denom"`
	Amount string `json:"amount"`
}

type LockEntry struct {
	LockID    int64  `json:"lock_id"`
	Funds     Funds  `json:"funds"`
	LockStart string `json:"lock_start"`
	LockEnd   string `json:"lock_end"`
}

type Lockup struct {
	LockEntry          LockEntry `json:"lock_entry"`
	CurrentVotingPower string    `json:"current_voting_power"`
}

type LockData struct {
	Lockups []Lockup `json:"lockups"`
}

type LockResponse struct {
	Data LockData `json:"data"`
}

type RegisteredQuery struct {
	ID        string `json:"id"`
	Owner     string `json:"owner"`
	QueryType string `json:"query_type"`
	Keys      []struct {
		Path string `json:"path"`
		Key  string `json:"key"`
	} `json:"keys"`
	TransactionsFilter              string `json:"transactions_filter"`
	ConnectionID                    string `json:"connection_id"`
	UpdatePeriod                    string `json:"update_period"`
	LastSubmittedResultLocalHeight  string `json:"last_submitted_result_local_height"`
	LastSubmittedResultRemoteHeight struct {
		RevisionNumber string `json:"revision_number"`
		RevisionHeight string `json:"revision_height"`
	} `json:"last_submitted_result_remote_height"`
	Deposit []struct {
		Denom  string `json:"denom"`
		Amount string `json:"amount"`
	} `json:"deposit"`
	SubmitTimeout      string `json:"submit_timeout"`
	RegisteredAtHeight string `json:"registered_at_height"`
}

type QueryResponse struct {
	RegisteredQueries []RegisteredQuery `json:"registered_queries"`
}

type Proposal struct {
	RoundID     int    `json:"round_id"`
	TrancheID   int    `json:"tranche_id"`
	ProposalID  int    `json:"proposal_id"`
	Title       string `json:"title"`
	Description string `json:"description"`
	Power       string `json:"power"`
	Percentage  string `json:"percentage"`
}

type ProposalData struct {
	Data struct {
		Proposals []Proposal `json:"proposals"`
	} `json:"data"`
}

type RoundData struct {
	Data struct {
		RoundID int `json:"round_id"`
	} `json:"data"`
}

type RoundVotingPower struct {
	Data struct {
		VotingPower string `json:"total_voting_power"`
	} `json:"data"`
}

type UserVotingPower struct {
	Data struct {
		VotingPower int64 `json:"voting_power"`
	} `json:"data"`
}

type TotalPowerAtHeight struct {
	Data struct {
		Power  string `json:"power"`
		Height int64  `json:"height"`
	} `json:"data"`
}

type VotingPowerAtHeight struct {
	Data struct {
		Power  string `json:"power"`
		Height int64  `json:"height"`
	} `json:"data"`
}

type TokenInfoProvidersData struct {
	Data struct {
		Providers []TokenInfoProvider `json:"providers"`
	} `json:"data"`
}

type TokenInfoProvider struct {
	Derivative *DerivativeTokenInfoProvider `json:"derivative,omitempty"`
	LSM        *LSMTokenInfoProvider        `json:"lsm,omitempty"`
}

type DerivativeTokenInfoProvider struct {
	Contract string         `json:"contract"`
	Cache    map[string]any `json:"cache"`
}

type LSMTokenInfoProvider struct {
	Contract             string         `json:"contract"`
	Cache                map[string]any `json:"cache"`
	HubTransferChannelID string         `json:"hub_transfer_channel_id"`
}

type Gatekeeper struct {
	Data struct {
		Gatekeeper string `json:"gatekeeper"`
	} `json:"data"`
}

type StageData struct {
	Data struct {
		Stage Stage `json:"stage"`
	} `json:"data"`
}

type Stage struct {
	StageID    int     `json:"stage_id"`
	ActivateAt string  `json:"activate_at"`
	MerkleRoot string  `json:"merkle_root"`
	Hrp        *string `json:"hrp"`
}

type SignatureInfo struct {
	ClaimMsg  string `json:"claim_msg"`
	Signature string `json:"signature"`
}

type CosmosSignature struct {
	PubKey    string `json:"pub_key"`
	Signature string `json:"signature"`
}

type CosmosTx struct {
	Body       map[string]any `json:"body"`
	AuthInfo   AuthInfo       `json:"auth_info"`
	Signatures []string       `json:"signatures"`
}

type AuthInfo struct {
	SignerInfos []SignerInfo `json:"signer_infos"`
	Fee         any          `json:"fee"`
	Tip         *any         `json:"tip"`
}

type SignerInfo struct {
	PublicKey PublicKey `json:"public_key,omitempty"`
	ModeInfo  any       `json:"mode_info,omitempty"`
	Sequence  any       `json:"sequence,omitempty"`
}

type PublicKey struct {
	Type string `json:"@type"`
	Key  string `json:"key"`
}
