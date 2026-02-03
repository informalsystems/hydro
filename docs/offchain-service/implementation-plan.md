# Offchain Backend Service for Inflow Deposits - Simplified Implementation Plan

## Executive Summary

Building a **simplified** Go backend service in `offchain/` directory to manage USDC deposits from CEX users into Hydro Inflow vaults.

**Core Functionality:**
- Use **email addresses** as user IDs (plain text, no UUIDs)
- Deploy **one forwarder per user per EVM chain** + **one shared proxy per user** on Neutron using CREATE2
- Monitor forwarder contracts for deposits via **polling only** (no event subscriptions initially)
- Track deposit status through **simplified 4-state pipeline**
- REST API for: contract addresses, process status, fee calculations
- Support multiple EVM chains from day one

**Key Simplifications (Defer to Later):**
- No NextProcessID endpoint (can add if frontend needs it)
- No address signing/verification (add for security hardening later)
- Polling only for balance monitoring (event subscriptions can be added later)
- No Prometheus metrics initially
- No withdrawal functionality (deposits only)
- No sophisticated retry logic initially (basic retry with exponential backoff)

---

## Requirements Clarification ✓

### Confirmed Decisions from Client

**1. User Identification**
- User ID = email address (stored as plain text)
- No UUID generation needed

**2. Contract Architecture**
- **Forwarder**: One per user per EVM chain (each chain needs separate forwarder)
- **Proxy**: One per user (shared across all EVM chains)
- CREATE2 salt includes: `userEmail + chainID` (no need for contractType since bytecode is part of CREATE2 computation)

**3. Contract Deployment Flow**
- User sends USDC to precomputed forwarder address (even before contract deployed)
- Service monitors address and deploys contracts when sufficient funds arrive
- Operator wallet pays gas for all deployments and transactions
- Operator wallet configured as `operationalFeeRecipient` in forwarder contract

**4. Fee Structure**
- **No instantiation fee charged to user** - operator absorbs deployment gas costs
- Only charge **bridge operational fee** (configured in forwarder contract via `operationalFeeBps` and `minOperationalFee`)
- Fee calculation endpoint returns: bridge fee only (since no instantiation fee)

**5. Process States (Simplified to 4 states)**
1. `PENDING_FUNDS` - Waiting for user to send sufficient funds
2. `TRANSFER_IN_PROGRESS` - Forwarder bridge() called, funds moving EVM → Noble → Neutron
3. `DEPOSIT_IN_PROGRESS` - Funds at proxy, calling ForwardToInflow
4. `DEPOSIT_DONE` - Deposit complete, shares minted to proxy

**6. Process Creation**
- New process created when user sends USDC to forwarder address
- Each transfer = new process (tracked by unique process ID)
- Service monitors balance and initiates bridge when: `balance >= bridgeFee + minDeposit`

**7. Withdrawals**
- Not implementing in initial version (deposits only)
- Proxy contract has `WithdrawFunds` function that can be called by authorized addresses
- Future: authorization likely via multisig that verifies user emails

---

## Simplified Architecture

### 1. Folder Structure (Minimal)

```
offchain/
├── cmd/
│   └── server/
│       └── main.go                          # Application entry point
├── internal/
│   ├── api/
│   │   ├── handlers.go                      # All HTTP handlers in one file
│   │   ├── router.go                        # Router setup
│   │   └── types.go                         # Request/response types
│   ├── blockchain/
│   │   ├── evm/
│   │   │   ├── client.go                    # EVM client wrapper
│   │   │   ├── forwarder.go                 # Forwarder contract calls
│   │   │   └── create2.go                   # CREATE2 computation
│   │   └── cosmos/
│   │       ├── client.go                    # Cosmos client wrapper
│   │       ├── proxy.go                     # Proxy contract calls
│   │       └── instantiate.go               # Contract instantiation
│   ├── config/
│   │   └── config.go                        # Configuration (single file)
│   ├── database/
│   │   ├── db.go                            # Database connection
│   │   ├── migrations/
│   │   │   └── 001_schema.sql               # Schema
│   │   └── queries.go                       # Database queries (hand-written)
│   ├── models/
│   │   └── models.go                        # All models in one file
│   ├── service/
│   │   ├── contract.go                      # Contract service
│   │   ├── process.go                       # Process orchestration
│   │   └── fees.go                          # Fee calculation
│   └── worker/
│       ├── manager.go                       # Worker lifecycle management
│       ├── monitor.go                       # Balance monitoring (polling)
│       └── executor.go                      # Process state machine
├── deployments/
│   ├── Dockerfile
│   ├── docker-compose.yml
│   └── .env.example
├── go.mod
├── Makefile
└── README.md
```

**Key Simplifications:**
- Combine related code into fewer files (no micro-modules)
- No separate repository layer (queries directly in database package)
- No middleware package (put minimal middleware in router.go)
- No utils package initially (add as needed)
- No separate worker types (just monitor + executor)

### 2. Database Schema (Simplified)

```sql
-- Users table (email as primary key)
CREATE TABLE users (
    email VARCHAR(255) PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- Contracts table (stores precomputed addresses and deployment status)
CREATE TABLE contracts (
    id BIGSERIAL PRIMARY KEY,
    user_email VARCHAR(255) NOT NULL REFERENCES users(email) ON DELETE CASCADE,
    chain_id VARCHAR(50) NOT NULL,                    -- e.g., "1" (Ethereum), "8453" (Base)
    contract_type VARCHAR(20) NOT NULL,               -- "forwarder" or "proxy"
    address VARCHAR(66) NOT NULL,                     -- Precomputed CREATE2 address
    deployed BOOLEAN NOT NULL DEFAULT FALSE,          -- Has contract been deployed?
    deploy_tx_hash VARCHAR(66),                       -- Deployment transaction hash
    deployed_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    UNIQUE(user_email, chain_id, contract_type)
);

-- Processes table (tracks individual deposit operations)
CREATE TABLE processes (
    id BIGSERIAL PRIMARY KEY,
    process_id VARCHAR(200) NOT NULL UNIQUE,          -- e.g., "alice@example.com_1_001"
    user_email VARCHAR(255) NOT NULL REFERENCES users(email),
    chain_id VARCHAR(50) NOT NULL,
    forwarder_address VARCHAR(66) NOT NULL,           -- Denormalized for convenience
    proxy_address VARCHAR(66) NOT NULL,               -- Denormalized for convenience
    status VARCHAR(30) NOT NULL,                      -- PENDING_FUNDS, TRANSFER_IN_PROGRESS, etc.
    amount_usdc BIGINT,                               -- Amount in USDC base units (6 decimals)

    -- Transaction hashes for tracking
    bridge_tx_hash VARCHAR(66),                       -- EVM forwarder.bridge() tx
    deposit_tx_hash VARCHAR(66),                      -- Neutron proxy ForwardToInflow tx

    -- Error tracking (simplified)
    error_message TEXT,
    retry_count INT NOT NULL DEFAULT 0,

    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_contracts_user_email ON contracts(user_email);
CREATE INDEX idx_contracts_deployed ON contracts(deployed);
CREATE INDEX idx_processes_user_email ON processes(user_email);
CREATE INDEX idx_processes_status ON processes(status);
CREATE INDEX idx_processes_chain_id ON processes(chain_id);
```

**Simplifications:**
- No `monitoring_state` table (track in memory or simple config)
- No `process_status_history` audit trail (can add later)
- No `contract_signatures` table (no signing initially)
- Denormalize forwarder/proxy addresses in processes table for simpler queries
- Store only critical tx hashes (bridge + deposit, not intermediate steps)

### 3. Process Status State Machine (Simplified to 4 States)

```
PENDING_FUNDS
    ↓ (sufficient balance detected)
TRANSFER_IN_PROGRESS
    ↓ (funds arrive at proxy contract on Neutron)
DEPOSIT_IN_PROGRESS
    ↓ (ForwardToInflow succeeds)
DEPOSIT_DONE

Error state: FAILED (with error_message)
```

**State Transitions:**

1. **PENDING_FUNDS**
   - User sent USDC to forwarder address
   - Balance < (bridge fee + minimum deposit)
   - OR contracts not yet deployed
   - **Action**: Monitor balance via polling

2. **TRANSFER_IN_PROGRESS**
   - Sufficient balance detected
   - Contracts deployed (if needed)
   - `bridge()` called on forwarder
   - Funds moving: EVM → CCTP → Noble → IBC → Neutron
   - **Action**: Poll proxy contract balance on Neutron

3. **DEPOSIT_IN_PROGRESS**
   - USDC arrived at proxy contract on Neutron
   - `ForwardToInflow` called on proxy
   - **Action**: Poll transaction status

4. **DEPOSIT_DONE**
   - Deposit confirmed on Inflow vault
   - Shares minted to proxy contract
   - Process complete

**Simplification**: Collapse intermediate states (no separate states for CCTP completion, IBC transfer, etc.)

### Complete Deposit Flow

The following diagram shows the complete end-to-end flow from when a user sends USDC to their forwarder address until the deposit is completed:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              DEPOSIT FLOW                                    │
└─────────────────────────────────────────────────────────────────────────────┘

1. USER ACTION
   ┌──────────────────┐
   │ User sends USDC  │
   │ to forwarder     │
   │ address on EVM   │
   └────────┬─────────┘
            │
            ▼
2. MONITOR: detectNewDeposits() [every 30s]
   ┌──────────────────────────────────────────────────────────────────────┐
   │ For each chain:                                                       │
   │   1. Get ALL forwarder contracts from DB                              │
   │   2. For each forwarder:                                              │
   │      - Check if active process exists → skip if yes                   │
   │      - Check forwarder balance on EVM                                 │
   │      - If balance > 0:                                                │
   │          → Get proxy address for user                                 │
   │          → Create new Process (status: PENDING_FUNDS)                 │
   └────────┬─────────────────────────────────────────────────────────────┘
            │
            ▼
3. MONITOR: checkPendingFunds() [every 30s]
   ┌──────────────────────────────────────────────────────────────────────┐
   │ For each PENDING_FUNDS process:                                       │
   │   1. Check forwarder balance on EVM                                   │
   │   2. If balance >= minDepositAmount:                                  │
   │      → Update process amount                                          │
   │      → Send process to Executor via channel                           │
   └────────┬─────────────────────────────────────────────────────────────┘
            │
            ▼
4. EXECUTOR: executeBridge()
   ┌──────────────────────────────────────────────────────────────────────┐
   │ 1. Deploy forwarder contract if not deployed                          │
   │ 2. Deploy proxy contract if not deployed                              │
   │ 3. Calculate bridge fee                                               │
   │ 4. Call forwarder.bridge() on EVM                                     │
   │    → Funds: EVM → CCTP → Noble → IBC → Neutron                        │
   │ 5. Update process status → TRANSFER_IN_PROGRESS                       │
   └────────┬─────────────────────────────────────────────────────────────┘
            │
            ▼
5. MONITOR: checkTransferInProgress() [every 30s]
   ┌──────────────────────────────────────────────────────────────────────┐
   │ For each TRANSFER_IN_PROGRESS process:                                │
   │   1. Check proxy balance on Neutron                                   │
   │   2. If balance > 0 (funds arrived):                                  │
   │      → Send process to Executor via channel                           │
   └────────┬─────────────────────────────────────────────────────────────┘
            │
            ▼
6. EXECUTOR: executeDeposit()
   ┌──────────────────────────────────────────────────────────────────────┐
   │ 1. Call proxy.ForwardToInflow() on Neutron                            │
   │    → Deposits USDC into Inflow vault                                  │
   │    → Shares minted to proxy contract                                  │
   │ 2. Update process status → DEPOSIT_IN_PROGRESS                        │
   └────────┬─────────────────────────────────────────────────────────────┘
            │
            ▼
7. MONITOR: checkDepositInProgress() [every 30s]
   ┌──────────────────────────────────────────────────────────────────────┐
   │ For each DEPOSIT_IN_PROGRESS process:                                 │
   │   1. Check deposit tx confirmation on Neutron                         │
   │   2. If confirmed:                                                    │
   │      → Update process status → DEPOSIT_DONE                           │
   └────────┬─────────────────────────────────────────────────────────────┘
            │
            ▼
   ┌──────────────────┐
   │ DEPOSIT COMPLETE │
   │ Shares in proxy  │
   └──────────────────┘

ERROR HANDLING:
- On error: increment retry_count, log error
- If retry_count < 3: exponential backoff (5s, 10s, 20s), process retried on next poll
- If retry_count >= 3: mark process as FAILED
```

### 4. REST API Endpoints (Minimal)

```
GET  /health                                       # Health check

POST /api/v1/contracts/addresses                   # Get/create contract addresses
     Request: { "email": "user@example.com", "chain_ids": ["1", "8453"] }
     Response: { "email": "user@example.com",
                 "contracts": {
                   "1": { "forwarder": "0x...", "proxy": "neutron1..." },
                   "8453": { "forwarder": "0x...", "proxy": "neutron1..." }
                 }}

POST /api/v1/fees/calculate                        # Calculate bridge fees
     Request: { "chain_id": "1", "amount_usdc": "100000000" }
     Response: { "bridge_fee_usdc": "1000000",
                 "min_deposit_usdc": "10000000" }

GET  /api/v1/processes/status/:processId           # Get process status
     Response: { "process_id": "...",
                 "status": "TRANSFER_IN_PROGRESS",
                 "amount_usdc": "100000000",
                 "tx_hashes": { "bridge": "0x...", "deposit": null }}

GET  /api/v1/processes/user/:email                 # List user's processes
     Response: { "processes": [ {...}, {...} ] }
```

**Simplifications:**
- No authentication initially (can add API keys later)
- No rate limiting middleware (add later)
- No address signing/verification endpoints
- No NextProcessID endpoint (not needed yet)
- Hand-write handlers (no complex routing patterns)

### 5. Configuration (Single File)

```go
// internal/config/config.go
type Config struct {
    Server   ServerConfig
    Database DatabaseConfig
    Chains   map[string]ChainConfig  // key: chain_id
    Operator OperatorConfig
}

type ServerConfig struct {
    Port int
}

type DatabaseConfig struct {
    Host     string
    Port     int
    User     string
    Password string
    DBName   string
}

type ChainConfig struct {
    ChainID              string
    Name                 string
    RPCEndpoint          string          // EVM or Cosmos RPC
    USDCContractAddress  string          // For EVM chains
    CCTPContractAddress  string          // Skip's CCTP contract
    OperatorAddress      string          // Operator address on this chain
    OperationalFeeBps    uint16          // e.g., 50 = 0.5%
    MinOperationalFee    int64           // e.g., 1000000 = 1 USDC
    MinDepositAmount     int64           // e.g., 10000000 = 10 USDC
    ForwarderBytecode    string          // Hex-encoded bytecode
    DestinationDomain    uint32          // CCTP destination domain (Noble)
    RecipientBytes32     string          // Noble forwarding account
    DestinationCaller    string          // Skip relayer address
}

type OperatorConfig struct {
    EVMPrivateKey     string             // For EVM transactions
    NeutronMnemonic   string             // For Neutron transactions
    NeutronAddress    string
    FeeRecipient      string             // Where operational fees go
}

func LoadConfig() (*Config, error) {
    // Load from environment variables with defaults
}
```

**Simplifications:**
- Single config file, no separate chains.go / loader.go / api.go
- No worker config (use hardcoded intervals)
- Store bytecode as hex string (load at startup)
- Minimal validation (just check required fields)

### 6. Core Business Logic

#### CREATE2 Address Computation

```go
// internal/blockchain/evm/create2.go
func ComputeForwarderAddress(
    deployerAddress common.Address,
    userEmail string,
    chainID string,
    bytecode []byte,
) (common.Address, error) {
    // Salt = sha256(userEmail + ":" + chainID)
    salt := sha256.Sum256([]byte(userEmail + ":" + chainID))
    initCodeHash := crypto.Keccak256Hash(bytecode)

    // CREATE2: keccak256(0xff ++ deployer ++ salt ++ initCodeHash)[12:]
    data := make([]byte, 1+20+32+32)
    data[0] = 0xff
    copy(data[1:21], deployerAddress.Bytes())
    copy(data[21:53], salt[:])
    copy(data[53:85], initCodeHash.Bytes())

    hash := crypto.Keccak256(data)
    return common.BytesToAddress(hash[12:]), nil
}

// Similar for proxy on Neutron (use CosmWasm instantiate2)
```

#### Fee Calculation

```go
// internal/service/fees.go
func (s *FeeService) CalculateBridgeFee(chainID string, amountUSDC int64) (int64, error) {
    chainCfg := s.cfg.Chains[chainID]

    // Operational fee = max(amount * bps / 10000, minOperationalFee)
    fee := (amountUSDC * int64(chainCfg.OperationalFeeBps)) / 10000
    if fee < chainCfg.MinOperationalFee {
        fee = chainCfg.MinOperationalFee
    }

    return fee, nil
}
```

#### Balance Monitoring (Polling)

```go
// internal/worker/monitor.go
func (m *Monitor) Run(ctx context.Context) {
    ticker := time.NewTicker(30 * time.Second)  // Poll every 30s
    defer ticker.Stop()

    for {
        select {
        case <-ctx.Done():
            return
        case <-ticker.C:
            m.checkPendingProcesses(ctx)
        }
    }
}

func (m *Monitor) checkPendingProcesses(ctx context.Context) {
    // Get all processes in PENDING_FUNDS status
    processes := m.db.GetProcessesByStatus("PENDING_FUNDS")

    for _, proc := range processes {
        // Check forwarder balance
        balance := m.evmClient.GetUSDCBalance(proc.ForwarderAddress)

        if balance >= (m.cfg.Chains[proc.ChainID].MinDepositAmount) {
            // Trigger process execution
            m.executor.Execute(ctx, proc)
        }
    }
}
```

#### Process Executor (State Machine)

```go
// internal/worker/executor.go
func (e *Executor) Execute(ctx context.Context, proc *Process) error {
    switch proc.Status {
    case "PENDING_FUNDS":
        return e.handlePendingFunds(ctx, proc)
    case "TRANSFER_IN_PROGRESS":
        return e.handleTransferInProgress(ctx, proc)
    case "DEPOSIT_IN_PROGRESS":
        return e.handleDepositInProgress(ctx, proc)
    default:
        return nil
    }
}

func (e *Executor) handlePendingFunds(ctx context.Context, proc *Process) error {
    // 1. Deploy contracts if not deployed
    if !e.isForwarderDeployed(proc.ChainID, proc.UserEmail) {
        tx, err := e.deployForwarder(ctx, proc.ChainID, proc.UserEmail)
        if err != nil {
            return err
        }
        // Wait for confirmation...
    }

    if !e.isProxyDeployed(proc.UserEmail) {
        tx, err := e.deployProxy(ctx, proc.UserEmail)
        if err != nil {
            return err
        }
        // Wait for confirmation...
    }

    // 2. Call bridge() on forwarder
    balance := e.evmClient.GetUSDCBalance(proc.ForwarderAddress)
    fee := e.feeService.CalculateBridgeFee(proc.ChainID, balance)

    bridgeTx, err := e.evmClient.CallBridge(
        proc.ForwarderAddress,
        balance - fee,  // transferAmount (after fee deduction)
        fee,            // smartRelayFeeAmount (CCTP fee)
        e.cfg.Operator.FeeRecipient,
    )
    if err != nil {
        return err
    }

    // Update process
    e.db.UpdateProcess(proc.ID, "TRANSFER_IN_PROGRESS", bridgeTx.Hash())

    return nil
}

func (e *Executor) handleTransferInProgress(ctx context.Context, proc *Process) error {
    // Poll proxy contract balance on Neutron
    balance := e.cosmosClient.GetUSDCBalance(proc.ProxyAddress)

    if balance > 0 {
        // Call ForwardToInflow on proxy
        depositTx, err := e.cosmosClient.CallForwardToInflow(proc.ProxyAddress)
        if err != nil {
            return err
        }

        e.db.UpdateProcess(proc.ID, "DEPOSIT_IN_PROGRESS", depositTx.Hash())
    }

    return nil
}

func (e *Executor) handleDepositInProgress(ctx context.Context, proc *Process) error {
    // Check if deposit tx confirmed
    tx := e.cosmosClient.GetTx(proc.DepositTxHash)

    if tx.Confirmed {
        e.db.UpdateProcess(proc.ID, "DEPOSIT_DONE", "")
    }

    return nil
}
```

### 7. Dependencies (Minimal)

```go
// go.mod
require (
    github.com/gorilla/mux v1.8.1          // HTTP router
    github.com/ethereum/go-ethereum v1.13.8 // EVM client
    github.com/cosmos/cosmos-sdk v0.50.9    // Cosmos client
    github.com/lib/pq v1.10.9              // PostgreSQL driver
    github.com/jmoiron/sqlx v1.3.5         // SQL utilities
    go.uber.org/zap v1.27.0                // Logging
)
```

**Simplifications:**
- No CORS package initially (add if needed)
- No rate limiting package
- No UUID package (using emails)
- No time package (using stdlib time only)
- No testify initially (can add for tests later)

### 8. Deployment (Docker Compose)

```yaml
# deployments/docker-compose.yml
version: '3.8'

services:
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_DB: inflow_service
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: postgres
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data

  service:
    build:
      context: ..
      dockerfile: deployments/Dockerfile
    ports:
      - "8080:8080"
    environment:
      DB_HOST: postgres
      DB_PORT: 5432
      ETH_RPC_ENDPOINT: ${ETH_RPC_ENDPOINT}
      BASE_RPC_ENDPOINT: ${BASE_RPC_ENDPOINT}
      OPERATOR_EVM_PRIVATE_KEY: ${OPERATOR_EVM_PRIVATE_KEY}
      OPERATOR_NEUTRON_MNEMONIC: ${OPERATOR_NEUTRON_MNEMONIC}
    depends_on:
      - postgres

volumes:
  postgres_data:
```

```dockerfile
# deployments/Dockerfile
FROM golang:1.22-alpine AS builder
WORKDIR /app
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN go build -o server ./cmd/server

FROM alpine:latest
RUN apk --no-cache add ca-certificates
COPY --from=builder /app/server /server
CMD ["/server"]
```

---

## Implementation Roadmap (Simplified)

### Phase 1: Foundation (Week 1) ✅ COMPLETED
**Goal**: Basic structure, database, CREATE2

- [x] Initialize Go module: `go mod init hydro/offchain`
- [x] Create folder structure
- [x] Implement `internal/config/config.go` - load from env vars
- [x] Implement `internal/database/migrations/001_schema.sql` - users, contracts, processes tables
- [x] Implement `internal/database/db.go` - PostgreSQL connection
- [x] Implement `internal/database/queries.go` - basic CRUD operations
- [x] Implement `internal/models/models.go` - User, Contract, Process structs
- [x] Implement `internal/blockchain/evm/create2.go` - CREATE2 address computation
- [x] Implement `internal/blockchain/cosmos/instantiate2.go` - Neutron instantiate2 address computation
- [x] Write unit tests for CREATE2 with known test vectors
- [x] Create `deployments/docker-compose.yml` for local dev

**Deliverable**: Can start service, connect to database, compute CREATE2 addresses

### Phase 2: API Layer (Week 1-2) ✅ COMPLETED
**Goal**: REST API for contract addresses and fee calculation

- [x] Implement `internal/api/router.go` - HTTP router with gorilla/mux
- [x] Implement `internal/api/types.go` - request/response structs
- [x] Implement `internal/service/contract.go` - contract address logic
- [x] Implement `internal/service/fees.go` - fee calculation logic
- [x] Implement `internal/api/handlers.go`:
  - [x] `POST /api/v1/contracts/addresses` - get/create addresses
  - [x] `POST /api/v1/fees/calculate` - calculate bridge fees
  - [x] `GET /api/v1/processes/status/:processId` - get process status
  - [x] `GET /api/v1/processes/user/:email` - list user processes
  - [x] `GET /health` - health check
- [ ] Test endpoints with curl/Postman

**Deliverable**: Can call API to get contract addresses and calculate fees

### Phase 3: Blockchain Clients (Week 2-3) ✅ COMPLETED
**Goal**: Interact with EVM and Cosmos chains

- [x] Implement `internal/blockchain/evm/client.go`:
  - [x] Connect to EVM RPC
  - [x] Get USDC balance
  - [x] Deploy forwarder contract
  - [x] Call `bridge()` function
- [x] Implement `internal/blockchain/evm/forwarder.go`:
  - [x] Generate ABI bindings for CCTPUSDCForwarder
  - [x] Wrapper functions for contract calls
- [x] Implement `internal/blockchain/cosmos/client.go`:
  - [x] Connect to Neutron RPC/REST
  - [x] Get USDC balance (IBC denom)
  - [x] Query transaction status
  - [x] Sign and broadcast transactions using cosmos-sdk tx builder
- [x] Implement `internal/blockchain/cosmos/proxy.go`:
  - [x] Instantiate proxy contract (instantiate2)
  - [x] Call `ForwardToInflow` execute message
  - [x] Query proxy config and state
- [ ] Test with testnet (Sepolia, Neutron testnet)

**Deliverable**: Can deploy contracts and call functions on testnet

### Phase 4: Workers (Week 3-4) ✅ COMPLETED
**Goal**: Background processing for deposits

- [x] Implement `internal/worker/manager.go`:
  - [x] Initialize all blockchain clients (EVM and Cosmos)
  - [x] Manage worker lifecycle (start, shutdown)
  - [x] Graceful shutdown with context cancellation
- [x] Implement `internal/worker/monitor.go`:
  - [x] Poll forwarder contracts for balances
  - [x] Detect new deposits on forwarder addresses (scan ALL forwarders)
  - [x] Detect when balance >= min deposit threshold
  - [x] Send ready processes to executor via channel
- [x] Implement `internal/worker/executor.go`:
  - [x] Handle PENDING_FUNDS (deploy contracts, call bridge)
  - [x] Handle TRANSFER_IN_PROGRESS (poll proxy balance)
  - [x] Handle DEPOSIT_IN_PROGRESS (check deposit tx)
  - [x] Error handling with exponential backoff (max 3 retries)
- [x] Implement `internal/service/process.go`:
  - [x] Create process when funds detected
  - [x] Update process status
  - [x] Query process by ID or user email
- [x] Update `cmd/server/main.go`:
  - [x] Start workers in goroutines
  - [x] Graceful shutdown
- [ ] Test end-to-end on testnet

**Deliverable**: Can process deposits end-to-end automatically

### Phase 5: Testing & Hardening (Week 4) ✅ COMPLETED
**Goal**: Ensure reliability

- [x] Add retry logic with exponential backoff (implemented in executor.go)
- [x] Add error handling and logging (throughout codebase)
- [x] Create unit tests for fee calculation service
- [x] Create unit tests for API handlers
- [x] Update docker-compose.yml to enable service
- [x] Document setup and deployment process (README.md)
- [ ] Test with multiple concurrent deposits (manual testing needed)
- [ ] Test with insufficient funds scenarios (manual testing needed)
- [ ] Test with contract deployment failures (manual testing needed)

**Deliverable**: Production-ready service for testnet

### Phase 6: Mainnet Deployment (Week 5)
**Goal**: Deploy to production

- [ ] Update config for mainnet RPC endpoints
- [ ] Deploy to DigitalOcean
- [ ] Test with small amounts
- [ ] Monitor logs and processes
- [ ] Iterate based on issues

**Deliverable**: Service running on mainnet

---

## Critical Implementation Notes

### 1. CREATE2 Salt Calculation

Since CREATE2 address depends on:
- Deployer address (operator wallet)
- Salt (derived from userEmail + chainID)
- Init code hash (forwarder bytecode + constructor args)

**Important**: Forwarder constructor params must be consistent across all deployments for a given chain:
```solidity
constructor(
    address _cctpContract,      // Same for all users on this chain
    uint32 _destinationDomain,  // Same (Noble domain)
    address _tokenToBridge,     // Same (USDC address)
    bytes32 _recipient,         // Same (Noble forwarding account)
    bytes32 _destinationCaller, // Same (Skip relayer)
    address _operator,          // Same (our operator address)
    address _admin,             // Same (our admin address)
    uint256 _operationalFeeBps, // Same
    uint256 _minOperationalFee  // Same
)
```

**If any parameter differs, the CREATE2 address will be different!**

Solution: Store these parameters in chain config, use same values for all users.

### 2. Process ID Generation

Format: `{email}_{chainID}_{sequence}`
Example: `alice@example.com_1_001`

Sequence number: Query database for max sequence for this user+chain, increment by 1.

```go
func GenerateProcessID(email string, chainID string, db *DB) string {
    seq := db.GetMaxSequence(email, chainID) + 1
    return fmt.Sprintf("%s_%s_%03d", email, chainID, seq)
}
```

### 3. Forwarder Monitoring

Since contracts may not be deployed yet, we can't subscribe to events. Instead:
- Poll USDC contract for balance at precomputed forwarder address
- This works even before forwarder is deployed
- Check every 30 seconds for PENDING_FUNDS processes

### 4. Proxy Balance Detection

After bridge() is called:
- Funds go EVM → CCTP → Noble → IBC → Neutron
- This takes 5-15 minutes typically
- Poll proxy contract balance on Neutron every 30 seconds
- When balance > 0, call ForwardToInflow

### 5. Error Handling (Basic)

```go
func (e *Executor) Execute(ctx context.Context, proc *Process) error {
    defer func() {
        if r := recover(); r != nil {
            e.db.UpdateProcessError(proc.ID, fmt.Sprintf("panic: %v", r))
        }
    }()

    err := e.executeInternal(ctx, proc)
    if err != nil {
        proc.RetryCount++
        if proc.RetryCount > 3 {
            e.db.UpdateProcess(proc.ID, "FAILED", "")
            e.db.UpdateProcessError(proc.ID, err.Error())
        } else {
            // Retry with exponential backoff
            time.Sleep(time.Duration(math.Pow(2, float64(proc.RetryCount))) * time.Minute)
            return e.Execute(ctx, proc)
        }
    }
    return err
}
```

---

## Testing Plan

### Unit Tests
- CREATE2 address computation with known test vectors
- Fee calculation with various amounts and BPS values
- Process ID generation

### Integration Tests (Testnet)
1. Call `POST /api/v1/contracts/addresses` for test email
2. Verify precomputed addresses returned
3. Send testnet USDC to forwarder address from faucet
4. Monitor process status via `GET /api/v1/processes/status/{id}`
5. Verify status progresses: PENDING_FUNDS → TRANSFER_IN_PROGRESS → DEPOSIT_IN_PROGRESS → DEPOSIT_DONE
6. Query Inflow vault to confirm shares minted to proxy

### Manual Testing Scenarios
- Insufficient funds (should stay in PENDING_FUNDS)
- Multiple deposits from same user
- Multiple users with different emails
- Contract deployment failures
- RPC endpoint failures

---

## Environment Variables (.env.example)

```bash
# Server
SERVER_PORT=8080

# Database
DB_HOST=localhost
DB_PORT=5432
DB_USER=postgres
DB_PASSWORD=postgres
DB_NAME=inflow_service

# Ethereum
ETH_RPC_ENDPOINT=https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY
ETH_USDC_ADDRESS=0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48
ETH_CCTP_CONTRACT=0x... # Skip's CCTP contract
ETH_OPERATIONAL_FEE_BPS=50  # 0.5%
ETH_MIN_OPERATIONAL_FEE=1000000  # 1 USDC
ETH_MIN_DEPOSIT=50000000  # 50 USDC

# Base
BASE_RPC_ENDPOINT=https://base-mainnet.g.alchemy.com/v2/YOUR_KEY
BASE_USDC_ADDRESS=0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913
BASE_CCTP_CONTRACT=0x...
BASE_OPERATIONAL_FEE_BPS=50
BASE_MIN_OPERATIONAL_FEE=1000000
BASE_MIN_DEPOSIT=10000000  # 10 USDC

# Neutron
NEUTRON_RPC_ENDPOINT=https://neutron-rpc.polkachu.com:443
NEUTRON_GRPC_ENDPOINT=neutron-grpc.polkachu.com:12090
NEUTRON_CONTROL_CENTER=neutron1...
NEUTRON_INFLOW_VAULT=neutron1...

# Operator
OPERATOR_EVM_PRIVATE_KEY=0x...
OPERATOR_NEUTRON_MNEMONIC=word1 word2 ... word24
OPERATOR_FEE_RECIPIENT=0x...  # Where operational fees go
OPERATOR_NEUTRON_ADDRESS=neutron1...

# CCTP (Noble)
CCTP_DESTINATION_DOMAIN=4  # Noble
CCTP_RECIPIENT_BYTES32=0x...  # Noble forwarding account
CCTP_DESTINATION_CALLER=0x...  # Skip relayer
```

---

## Open Questions (To Resolve During Implementation)

1. **Noble Forwarding Account**: Need to confirm Noble account address for `recipient` parameter
2. **Skip Relayer Address**: Need Skip's relayer address for `destinationCaller` parameter
3. **Forwarder Bytecode**: Need to compile CCTPUSDCForwarder.sol and get bytecode with constructor args
4. **Proxy Contract Code ID**: Need code ID of deployed proxy contract on Neutron
5. **CCTP Smart Relay Fee**: How to estimate this dynamically? Start with fixed amount?

---

## Critical Files

**Contracts (Read-Only):**
- [contracts/inflow/evm/contracts/CCTPUSDCForwarder.sol](contracts/inflow/evm/contracts/CCTPUSDCForwarder.sol)
- [contracts/inflow/proxy/src/contract.rs](contracts/inflow/proxy/src/contract.rs)
- [contracts/inflow/vault/src/contract.rs](contracts/inflow/vault/src/contract.rs)

**Service Files - Phase 1 (Foundation) ✅:**
- `offchain/cmd/server/main.go` - Entry point with HTTP server
- `offchain/internal/config/config.go` - Configuration from env vars
- `offchain/internal/database/migrations/001_schema.sql` - Database schema
- `offchain/internal/database/db.go` - PostgreSQL connection
- `offchain/internal/database/queries.go` - Database operations
- `offchain/internal/models/models.go` - Data models

**Service Files - Phase 2 (API Layer) ✅:**
- `offchain/internal/api/types.go` - Request/response types
- `offchain/internal/api/handlers.go` - HTTP handlers
- `offchain/internal/api/router.go` - HTTP router with middleware
- `offchain/internal/service/contract.go` - Contract address service
- `offchain/internal/service/fees.go` - Fee calculation service

**Service Files - Phase 3 (Blockchain Clients) ✅:**
- `offchain/internal/blockchain/evm/create2.go` - CREATE2 address computation
- `offchain/internal/blockchain/evm/client.go` - EVM client (balance, deploy, tx)
- `offchain/internal/blockchain/evm/forwarder.go` - Forwarder contract ABI bindings
- `offchain/internal/blockchain/cosmos/instantiate2.go` - Neutron instantiate2 computation
- `offchain/internal/blockchain/cosmos/client.go` - Cosmos client (tx builder, queries)
- `offchain/internal/blockchain/cosmos/proxy.go` - Proxy contract interactions
- `offchain/internal/blockchain/cosmos/noble.go` - Noble forwarding address queries via RPC (ABCI query)

**Service Files - Phase 4 (Workers) ✅:**
- `offchain/internal/service/process.go` - Process lifecycle service
- `offchain/internal/worker/manager.go` - Worker lifecycle management, holds blockchain clients
- `offchain/internal/worker/monitor.go` - Balance monitoring (polling every 30s)
- `offchain/internal/worker/executor.go` - State machine for process execution

**Deployment Files ✅:**
- `offchain/deployments/docker-compose.yml` - Local development setup
- `offchain/deployments/Dockerfile` - Container image
- `offchain/deployments/.env.example` - Environment variables template

**Test Files - Phase 5 ✅:**
- `offchain/internal/service/fees_test.go` - Fee calculation tests
- `offchain/internal/api/handlers_test.go` - API handler tests
- `offchain/internal/blockchain/evm/create2_test.go` - CREATE2 address tests
- `offchain/internal/blockchain/cosmos/instantiate2_test.go` - Instantiate2 tests
- `offchain/internal/blockchain/cosmos/noble_test.go` - Noble forwarding tests

---

## Summary

This simplified plan focuses on **core functionality only**:
- ✅ Precompute contract addresses with CREATE2
- ✅ Monitor forwarder balances via polling
- ✅ Deploy contracts when sufficient funds arrive
- ✅ Execute bridge transactions
- ✅ Track deposits through 4-state pipeline
- ✅ REST API for status queries
- ✅ Background workers for automatic deposit processing
- ✅ Error handling with exponential backoff retries
- ✅ Unit tests for core functionality
- ✅ Docker deployment configuration

**Current Status**: Phases 1-5 completed. Ready for testnet testing.

**Deferred for later:**
- Address signing/verification
- Event-driven monitoring
- Prometheus metrics
- Withdrawal functionality
- Admin dashboard

**Estimated Timeline**: 4-5 weeks to production-ready testnet deployment
