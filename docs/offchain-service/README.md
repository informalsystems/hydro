# Offchain Service Documentation

This directory contains documentation for the Hydro Inflow offchain backend service.

## Documents

- **[implementation-plan.md](implementation-plan.md)** - Complete implementation plan with architecture, database schema, API endpoints, and roadmap

## Implementation Status

### Phase 1: Foundation ⏳ (In Progress)
- [ ] Initialize Go module
- [ ] Create folder structure
- [ ] Implement config
- [ ] Implement database schema and connection
- [ ] Implement models
- [ ] Implement CREATE2 address computation
- [ ] Unit tests for CREATE2
- [ ] Docker compose setup

### Phase 2: API Layer (Not Started)
### Phase 3: Blockchain Clients (Not Started)
### Phase 4: Workers (Not Started)
### Phase 5: Testing & Hardening (Not Started)
### Phase 6: Mainnet Deployment (Not Started)

## How to Continue Development

If you need to resume work on this service after closing VSCode or in a new session:

1. Open the project: `cd /home/stana/go/src/cosmos/hydro`
2. Reference the plan: "Continue implementing the offchain service according to docs/offchain-service/implementation-plan.md"
3. Check current phase: Look at this README to see which phase is in progress
4. Continue from where you left off

## Quick Start (After Implementation)

```bash
cd offchain
docker-compose -f deployments/docker-compose.yml up
```

## Project Structure

```
offchain/
├── cmd/server/main.go           # Entry point
├── internal/                    # Internal packages
│   ├── api/                     # HTTP handlers
│   ├── blockchain/              # EVM & Cosmos clients
│   ├── config/                  # Configuration
│   ├── database/                # DB connection & queries
│   ├── models/                  # Data models
│   ├── service/                 # Business logic
│   └── worker/                  # Background workers
└── deployments/                 # Docker files
```

## Key Contacts & Resources

- Implementation Plan: [implementation-plan.md](implementation-plan.md)
- EVM Forwarder Contract: `contracts/inflow/evm/contracts/CCTPUSDCForwarder.sol`
- Neutron Proxy Contract: `contracts/inflow/proxy/src/contract.rs`
- Inflow Vault Contract: `contracts/inflow/vault/src/contract.rs`
