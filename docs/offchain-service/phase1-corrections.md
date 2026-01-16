# Offchain Service Implementation Plan

## Overview

This document tracks the implementation of the offchain backend service for managing USDC deposits to Inflow vaults via Cross-Chain Transfer Protocol (CCTP) and IBC.

## Architecture

The service coordinates between:
- **EVM chains** (Ethereum, Base) - User deposits USDC to forwarder contracts
- **Noble** - CCTP bridge hub for USDC transfers
- **Neutron** - CosmWasm proxy contracts that forward to Inflow vaults

## Phase 1: Core Infrastructure ✅ COMPLETED

### Phase 1.1: Database & Models ✅

**Status**: Complete

**Changes Made**:
- Simplified database schema (removed audit timestamps)
- Removed unnecessary deployment tracking fields
- Database tables:
  - `users` - User records (email PK)
  - `contracts` - Precomputed addresses with deployment status
  - `processes` - Deposit operation tracking

**Files**:
- [internal/database/migrations/001_schema.sql](../internal/database/migrations/001_schema.sql)
- [internal/models/models.go](../internal/models/models.go)
- [internal/database/queries.go](../internal/database/queries.go)

### Phase 1.2: Configuration ✅

**Status**: Complete

**Features**:
- Multi-chain EVM configuration (Ethereum, Base)
- Neutron configuration with:
  - Multiple control centers (comma-separated)
  - Admin addresses for proxy contracts
  - Proxy code ID for instantiate2
- Noble gRPC endpoint configuration
- IBC channel configuration (Noble ↔ Neutron)
- MinDepositAmount per chain (kept per user request)

**Files**:
- [internal/config/config.go](../internal/config/config.go)
- [deployments/.env.example](../deployments/.env.example)

### Phase 1.3: CosmWasm instantiate2 Implementation ✅

**Status**: Complete with tests

**Implementation**:
- Deterministic proxy address computation using CosmWasm instantiate2
- Formula: `sha256("contract_addr" ++ code_id ++ salt ++ creator_canonical_address)`
- Salt generation: `sha256(userEmail + ":proxy")`
- Proper bech32 encoding/decoding with 5-bit ↔ 8-bit conversion

**Tests**:
- Input validation tests
- Deterministic address generation tests
- Uniqueness tests (different users/code IDs produce different addresses)
- Address verification tests

**Files**:
- [internal/blockchain/cosmos/instantiate2.go](../internal/blockchain/cosmos/instantiate2.go)
- [internal/blockchain/cosmos/instantiate2_test.go](../internal/blockchain/cosmos/instantiate2_test.go)

### Phase 1.4: Noble Integration ✅

**Status**: Complete (implementation ready, requires Noble protobuf types for full functionality)

**Implementation**:
- Noble gRPC client for querying forwarding addresses
- `QueryForwardingAddress()` - Queries Noble for IBC forwarding address
- `ConvertToBytes32()` - Converts Noble bech32 address to bytes32 for EVM
- `ConvertBytes32ToHex()` - Helper for hex encoding

**Architecture Decision**:
- ✅ Using gRPC (not CLI) for Noble queries
- Benefits: No binary dependency, type-safe, easier testing
- Requires: `github.com/noble-assets/forwarding` protobuf types

**Files**:
- [internal/blockchain/cosmos/noble.go](../internal/blockchain/cosmos/noble.go)

### Phase 1.5: EVM CREATE2 Implementation ✅

**Status**: Complete

**Features**:
- Forwarder CREATE2 address computation
- EVM formula: `keccak256(0xff ++ deployer ++ salt ++ keccak256(initCode))`
- Salt generation per user per chain
- Removed CosmWasm placeholder (moved to cosmos package)

**Files**:
- [internal/blockchain/evm/create2.go](../internal/blockchain/evm/create2.go)

### Phase 1.6: Docker & Deployment Setup ✅

**Status**: Complete

**Features**:
- Docker Compose with PostgreSQL
- Environment variable configuration
- Database initialization scripts

**Files**:
- [docker-compose.yml](../docker-compose.yml)
- [deployments/.env.example](../deployments/.env.example)

## Deployment Order (CRITICAL)

The correct order for contract deployment is:

1. **COMPUTE proxy address** (instantiate2 - deterministic, NO deployment yet)
2. **Query Noble** for forwarding address using computed proxy address
3. **Convert Noble address to bytes32** for EVM contract constructor
4. **DEPLOY forwarder contract** (EVM) with recipient bytes32 in constructor
5. **DEPLOY proxy contract** (Neutron) using instantiate2

**Why This Order**:
- instantiate2 allows knowing the address BEFORE deployment (like CREATE2)
- Forwarder constructor needs the Noble forwarding address (bytes32)
- Noble forwarding address corresponds to the (computed, not yet deployed) proxy
- This enables us to deploy forwarder first with the correct recipient

## Key Technical Decisions

### 1. Configuration

**Neutron**:
- ✅ Multiple control centers (array)
- ✅ Multiple admins (array)
- ✅ Proxy code ID stored
- ❌ REMOVED: Static INFLOW_VAULT (proxy queries CC)

**EVM Chains**:
- ✅ KEPT: MinDepositAmount (balance threshold check)
- ❌ REMOVED: Static RecipientBytes32 (computed dynamically per user)

### 2. Database Schema

**Simplified Approach**:
- ❌ No created_at/updated_at timestamps (unnecessary for MVP)
- ❌ No deploy_tx_hash/deployed_at (deployment tracking simplified)
- ✅ deployed boolean only (sufficient for tracking)

**Rationale**: Simpler schema, easier maintenance, sufficient for tracking

### 3. Noble Integration

**gRPC over CLI**:
- ✅ No binary dependency in Docker
- ✅ Type-safe with protobuf
- ✅ Better error handling
- ✅ Easier to mock for testing

### 4. Address Computation

**CosmWasm (instantiate2)**:
- Uses code_id (reference to stored contract)
- Deterministic before deployment
- Bech32 encoding with neutron prefix

**EVM (CREATE2)**:
- Uses full bytecode + constructor args
- Deterministic before deployment
- 20-byte hex address with 0x prefix

## Testing Status

### Unit Tests

- ✅ CosmWasm instantiate2 (8 tests, all passing)
- ⏳ Noble client (requires protobuf types)
- ⏳ EVM CREATE2 (TODO)

### Integration Tests

- ⏳ Full address chain: email → proxy → Noble → bytes32 → forwarder
- ⏳ Testnet deployment verification

## Phase 2: Service Layer (TODO)

### Contract Service
- GetOrCreateAddresses (compute and store addresses)
- DeployForwarder (EVM deployment)
- DeployProxy (Neutron instantiate2)

### Process Service
- CreateProcess (initiate deposit)
- UpdateProcessStatus (state transitions)
- GetProcessStatus (query process)

## Phase 3: Workers (TODO)

### Balance Monitor
- Poll forwarder addresses for USDC balance
- Trigger transfer when balance >= MinDepositAmount

### Process Executor
- Execute bridge transactions (forwarder.bridge())
- Execute deposit transactions (proxy.ForwardToInflow)
- Handle errors and retries

## Phase 4: API Layer (TODO)

### HTTP Handlers
- POST /addresses - Get or create addresses for user
- POST /deposit - Initiate deposit process
- GET /deposit/:id - Get deposit status
- GET /deposits - List user deposits

## Phase 5: Monitoring & Observability (TODO)

### Logging
- Structured logging with zap
- Log levels per environment

### Metrics
- Prometheus metrics
- Process success/failure rates
- Gas usage tracking

## Dependencies

### Go Modules
```
github.com/btcsuite/btcutil v1.0.2          // bech32 encoding
github.com/ethereum/go-ethereum v1.13.8     // EVM utilities
github.com/jmoiron/sqlx v1.3.5             // Database layer
github.com/lib/pq v1.10.9                  // PostgreSQL driver
go.uber.org/zap v1.27.1                    // Logging
google.golang.org/grpc v1.60.1             // gRPC client
google.golang.org/protobuf v1.32.0         // Protobuf
```

### External Dependencies (TODO)
```
github.com/noble-assets/forwarding v1.0.0   // Noble forwarding types
```

## Environment Variables Reference

See [deployments/.env.example](../deployments/.env.example) for complete list.

**Key Variables**:
```bash
# Neutron
NEUTRON_RPC_ENDPOINT=https://neutron-rpc.polkachu.com:443
NEUTRON_CONTROL_CENTERS=neutron1...,neutron1...,neutron1...  # Multiple addresses
NEUTRON_ADMINS=neutron1...,neutron1...
NEUTRON_PROXY_CODE_ID=123

# Noble
NOBLE_GRPC_ENDPOINT=noble-grpc.polkachu.com:9090
NOBLE_NEUTRON_CHANNEL=channel-18

# EVM Chains (example: Ethereum)
ETH_RPC_ENDPOINT=...
ETH_MIN_DEPOSIT=50000000  # 50 USDC (kept per user request)
ETH_CCTP_CONTRACT=...
```

## Next Steps

1. ✅ Complete Phase 1 corrections (database, models, config)
2. ✅ Implement CosmWasm instantiate2
3. ✅ Implement Noble gRPC client
4. ✅ Write unit tests
5. ⏳ Add Noble protobuf types dependency
6. ⏳ Implement Phase 2 (Service layer)
7. ⏳ Implement Phase 3 (Workers)
8. ⏳ Implement Phase 4 (API layer)

## Notes

- All Phase 1 corrections requested by user have been completed
- Code compiles successfully
- Unit tests pass (8/8 for instantiate2)
- Ready to proceed with Phase 2 (Service layer)
