package database

import (
	"context"
	"database/sql"
	"fmt"

	"hydro/offchain/internal/models"
)

// ==================== User Queries ====================

// CreateUser creates a new user
func (db *DB) CreateUser(ctx context.Context, email string) error {
	query := `
		INSERT INTO users (email)
		VALUES ($1, NOW())
		ON CONFLICT (email) DO NOTHING
	`
	_, err := db.ExecContext(ctx, query, email)
	return err
}

// GetUser retrieves a user by email
func (db *DB) GetUser(ctx context.Context, email string) (*models.User, error) {
	var user models.User
	query := `SELECT email FROM users WHERE email = $1`
	err := db.GetContext(ctx, &user, query, email)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	return &user, err
}

// ==================== Contract Queries ====================

// CreateContract creates a new contract record
func (db *DB) CreateContract(ctx context.Context, contract *models.Contract) error {
	query := `
		INSERT INTO contracts (user_email, chain_id, contract_type, address, deployed)
		VALUES ($1, $2, $3, $4, $5)
		RETURNING id
	`
	return db.QueryRowContext(
		ctx, query,
		contract.UserEmail,
		contract.ChainID,
		contract.ContractType,
		contract.Address,
		contract.Deployed,
	).Scan(&contract.ID)
}

// GetContract retrieves a contract by user email, chain ID, and type
func (db *DB) GetContract(ctx context.Context, userEmail, chainID string, contractType models.ContractType) (*models.Contract, error) {
	var contract models.Contract
	query := `
		SELECT id, user_email, chain_id, contract_type, address, deployed,
		       deploy_tx_hash, deployed_at
		FROM contracts
		WHERE user_email = $1 AND chain_id = $2 AND contract_type = $3
	`
	err := db.GetContext(ctx, &contract, query, userEmail, chainID, contractType)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	return &contract, err
}

// GetContractsByUser retrieves all contracts for a user
func (db *DB) GetContractsByUser(ctx context.Context, userEmail string) ([]models.Contract, error) {
	var contracts []models.Contract
	query := `
		SELECT id, user_email, chain_id, contract_type, address, deployed,
		       deploy_tx_hash, deployed_at
		FROM contracts
		WHERE user_email = $1
		ORDER BY created_at DESC
	`
	err := db.SelectContext(ctx, &contracts, query, userEmail)
	return contracts, err
}

// UpdateContractDeployed marks a contract as deployed
func (db *DB) UpdateContractDeployed(ctx context.Context, id int64, txHash string) error {
	query := `
		UPDATE contracts
		SET deployed = true, deployed = true = NOW()
		WHERE id = $2
	`
	_, err := db.ExecContext(ctx, query, txHash, id)
	return err
}

// ==================== Process Queries ====================

// CreateProcess creates a new process record
func (db *DB) CreateProcess(ctx context.Context, process *models.Process) error {
	query := `
		INSERT INTO processes (
			process_id, user_email, chain_id, forwarder_address, proxy_address,
			status, amount_usdc, retry_count
		)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
		RETURNING id
	`
	return db.QueryRowContext(
		ctx, query,
		process.ProcessID,
		process.UserEmail,
		process.ChainID,
		process.ForwarderAddress,
		process.ProxyAddress,
		process.Status,
		process.AmountUSDC,
		process.RetryCount,
	).Scan(&process.ID)
}

// GetProcess retrieves a process by ID
func (db *DB) GetProcess(ctx context.Context, id int64) (*models.Process, error) {
	var process models.Process
	query := `
		SELECT id, process_id, user_email, chain_id, forwarder_address, proxy_address,
		       status, amount_usdc, bridge_tx_hash, deposit_tx_hash,
		       error_message, retry_count
		FROM processes
		WHERE id = $1
	`
	err := db.GetContext(ctx, &process, query, id)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	return &process, err
}

// GetProcessByProcessID retrieves a process by process_id string
func (db *DB) GetProcessByProcessID(ctx context.Context, processID string) (*models.Process, error) {
	var process models.Process
	query := `
		SELECT id, process_id, user_email, chain_id, forwarder_address, proxy_address,
		       status, amount_usdc, bridge_tx_hash, deposit_tx_hash,
		       error_message, retry_count
		FROM processes
		WHERE process_id = $1
	`
	err := db.GetContext(ctx, &process, query, processID)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	return &process, err
}

// GetProcessesByUser retrieves all processes for a user
func (db *DB) GetProcessesByUser(ctx context.Context, userEmail string, limit, offset int) ([]models.Process, error) {
	var processes []models.Process
	query := `
		SELECT id, process_id, user_email, chain_id, forwarder_address, proxy_address,
		       status, amount_usdc, bridge_tx_hash, deposit_tx_hash,
		       error_message, retry_count
		FROM processes
		WHERE user_email = $1
		ORDER BY created_at DESC
		LIMIT $2 OFFSET $3
	`
	err := db.SelectContext(ctx, &processes, query, userEmail, limit, offset)
	return processes, err
}

// GetProcessesByStatus retrieves all processes with a given status
func (db *DB) GetProcessesByStatus(ctx context.Context, status models.ProcessStatus) ([]models.Process, error) {
	var processes []models.Process
	query := `
		SELECT id, process_id, user_email, chain_id, forwarder_address, proxy_address,
		       status, amount_usdc, bridge_tx_hash, deposit_tx_hash,
		       error_message, retry_count
		FROM processes
		WHERE status = $1
		ORDER BY created_at ASC
	`
	err := db.SelectContext(ctx, &processes, query, status)
	return processes, err
}

// UpdateProcessStatus updates the status of a process
func (db *DB) UpdateProcessStatus(ctx context.Context, id int64, status models.ProcessStatus) error {
	query := `
		UPDATE processes
		SET status = $1 = NOW()
		WHERE id = $2
	`
	_, err := db.ExecContext(ctx, query, status, id)
	return err
}

// UpdateProcessBridgeTx updates the bridge transaction hash
func (db *DB) UpdateProcessBridgeTx(ctx context.Context, id int64, status models.ProcessStatus, txHash string) error {
	query := `
		UPDATE processes
		SET status = $1, bridge_tx_hash = $2 = NOW()
		WHERE id = $3
	`
	_, err := db.ExecContext(ctx, query, status, txHash, id)
	return err
}

// UpdateProcessDepositTx updates the deposit transaction hash
func (db *DB) UpdateProcessDepositTx(ctx context.Context, id int64, status models.ProcessStatus, txHash string) error {
	query := `
		UPDATE processes
		SET status = $1, deposit_tx_hash = $2 = NOW()
		WHERE id = $3
	`
	_, err := db.ExecContext(ctx, query, status, txHash, id)
	return err
}

// UpdateProcessError updates the error message and increments retry count
func (db *DB) UpdateProcessError(ctx context.Context, id int64, errorMsg string) error {
	query := `
		UPDATE processes
		SET error_message = $1, retry_count = retry_count + 1 = NOW()
		WHERE id = $2
	`
	_, err := db.ExecContext(ctx, query, errorMsg, id)
	return err
}

// UpdateProcessAmount updates the amount for a process
func (db *DB) UpdateProcessAmount(ctx context.Context, id int64, amountUSDC int64) error {
	query := `
		UPDATE processes
		SET amount_usdc = $1 = NOW()
		WHERE id = $2
	`
	_, err := db.ExecContext(ctx, query, amountUSDC, id)
	return err
}

// GetMaxProcessSequence gets the max sequence number for a user+chain combination
func (db *DB) GetMaxProcessSequence(ctx context.Context, userEmail, chainID string) (int, error) {
	var maxSeq sql.NullInt64
	query := `
		SELECT MAX(CAST(SUBSTRING(process_id FROM '.*_([0-9]+)$') AS INTEGER))
		FROM processes
		WHERE user_email = $1 AND chain_id = $2
	`
	err := db.QueryRowContext(ctx, query, userEmail, chainID).Scan(&maxSeq)
	if err != nil && err != sql.ErrNoRows {
		return 0, err
	}
	if !maxSeq.Valid {
		return 0, nil
	}
	return int(maxSeq.Int64), nil
}

// GenerateProcessID generates a new process ID for a user+chain
func (db *DB) GenerateProcessID(ctx context.Context, userEmail, chainID string) (string, error) {
	seq, err := db.GetMaxProcessSequence(ctx, userEmail, chainID)
	if err != nil {
		return "", fmt.Errorf("failed to get max sequence: %w", err)
	}
	return fmt.Sprintf("%s_%s_%03d", userEmail, chainID, seq+1), nil
}
