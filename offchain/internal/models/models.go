package models

// ProcessStatus represents the state of a deposit process
type ProcessStatus string

const (
	ProcessStatusPendingFunds      ProcessStatus = "PENDING_FUNDS"
	ProcessStatusTransferInProgress ProcessStatus = "TRANSFER_IN_PROGRESS"
	ProcessStatusDepositInProgress ProcessStatus = "DEPOSIT_IN_PROGRESS"
	ProcessStatusDepositDone       ProcessStatus = "DEPOSIT_DONE"
	ProcessStatusFailed            ProcessStatus = "FAILED"
)

// ContractType represents the type of contract
type ContractType string

const (
	ContractTypeForwarder ContractType = "forwarder"
	ContractTypeProxy     ContractType = "proxy"
)

// User represents a user in the system (identified by email)
type User struct {
	Email string `db:"email"`
}

// Contract represents a deployed or precomputed contract
type Contract struct {
	ID           int64        `db:"id"`
	UserEmail    string       `db:"user_email"`
	ChainID      string       `db:"chain_id"`
	ContractType ContractType `db:"contract_type"`
	Address      string       `db:"address"`
	Deployed     bool         `db:"deployed"`
	DeployTxHash *string      `db:"deploy_tx_hash"`
	DeployedAt   *string      `db:"deployed_at"`
}

// Process represents a deposit operation
type Process struct {
	ID               int64         `db:"id"`
	ProcessID        string        `db:"process_id"`
	UserEmail        string        `db:"user_email"`
	ChainID          string        `db:"chain_id"`
	ForwarderAddress string        `db:"forwarder_address"`
	ProxyAddress     string        `db:"proxy_address"`
	Status           ProcessStatus `db:"status"`
	AmountUSDC       *int64        `db:"amount_usdc"` // nullable, in base units (6 decimals)
	BridgeTxHash     *string       `db:"bridge_tx_hash"`
	DepositTxHash    *string       `db:"deposit_tx_hash"`
	ErrorMessage     *string       `db:"error_message"`
	RetryCount       int           `db:"retry_count"`
}
