# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Hydro is an auction platform for the Cosmos Hub where ATOM stakers lock tokens in exchange for voting power to vote on proposals for deploying community pool funds into liquidity positions on decentralized exchanges. This is Protocol Owned Liquidity (PoL) that can be reused rather than spent.

The protocol operates in rounds (1 month duration) with multiple tranches. Users lock LSM (Liquid Staking Module) shares for various durations, receiving voting power that decays in steps based on remaining lock time at round end.

## Build Commands

### Rust Contracts

```bash
# Format code
make fmt

# Check formatting
make fmt-check

# Run clippy linter
make clippy

# Run unit tests (excludes e2e tests)
make test-unit

# Compile contracts with Docker optimizer
make compile

# Generate JSON schemas and TypeScript types
make schema

# Run code coverage
make coverage
```

### TypeScript

```bash
# Build TypeScript
npm run build

# Watch mode for TypeScript
npm run build:watch
```

### Testing

```bash
# E2E tests with CW Orchestrator (requires mnemonic)
make test-e2e E2E_TESTS_MNEMONIC="24 word mnemonic"

# Interchain tests with Go (requires Docker image for relayer)
make test-interchain

# Build ICQ relayer Docker image (required before interchain tests)
make build-docker-relayer
```

To run a single unit test:
```bash
cargo test --package hydro --lib test_name
```

## Repository Structure

### Core Contracts

- **contracts/hydro/** - Main Hydro contract handling locks, proposals, voting, and rounds
- **contracts/tribute/** - Separate tribute contract for distributing rewards to voters
- **contracts/dao-voting-adapter/** - Adapter for DAO DAO integration
- **contracts/gatekeeper/** - Controls locking/unlocking of tokens with additional validation
- **contracts/marketplace/** - Marketplace functionality
- **contracts/liquid-collateral/** - Liquid collateral management
- **contracts/token-info-providers/** - Providers for LSM, stToken, and dToken ratio tracking
  - lsm-token-info-provider/
  - st-token-info-provider/
  - d-token-info-provider/

### Inflow Subsystem

The Inflow subsystem manages controlled fund flows:

- **contracts/inflow/vault/** - Vault for holding funds
- **contracts/inflow/control-center/** - Central control logic
- **contracts/inflow/proxy/** - Proxy for fund routing
- **contracts/inflow/user-registry/** - User registration and management

### Packages

- **packages/interface/** - CW Orchestrator interfaces for all contracts
- **packages/cw-orch-interface/** - CW Orchestrator specific implementations
- **packages/test-utils/** - Shared testing utilities

### Tests

- **test/e2e/** - End-to-end tests using CW Orchestrator (Rust)
- **test/interchain/** - Interchain tests using Strangelove's Interchain Test framework (Go)

## Architecture

### Hydro Contract Core Components

The main Hydro contract (`contracts/hydro/src/`) is organized into these modules:

- **contract.rs** - Entry points (instantiate, execute, query, reply, migrate)
- **state.rs** - State management with storage maps for locks, votes, proposals, tranches
- **score_keeper.rs** - Calculates and tracks proposal scores and voting power
- **vote.rs** - Vote processing and validation logic
- **utils.rs** - Utility functions for power calculations, lock management
- **lsm_integration.rs** - Liquid Staking Module integration and validator tracking
- **gatekeeper.rs** - Integration with gatekeeper contract
- **token_manager.rs** - Token info provider management
- **governance.rs** - Governance queries for voting power
- **slashing.rs** - Slashing logic for proposal voters

### Key Concepts

**Locks**: Users lock LSM shares for a duration. Each lock has a unique ID and tracks the token amount, expiry, and validator.

**Voting Power**: Calculated as `locked_tokens * power_scaling_factor` where the factor depends on remaining lock duration at round end (1x, 1.25x, 1.5x, 2x, or 4x).

**Rounds and Tranches**: Voting happens in monthly rounds. Multiple tranches allow separate proposal sets. Users vote once per tranche per round.

**Proposals**: Submitted to specific tranches with metadata about liquidity deployment. Only whitelisted accounts can submit.

**Interchain Queries (ICQ)**: Used to track validator power ratios on Cosmos Hub. The contract maintains queries for the top 500 validators by delegation.

**Tribute**: Separate contract allowing anyone to lock tokens for a proposal. Voters receive tribute proportional to their voting power contribution.

### State Storage Patterns

The contract uses `cw-storage-plus` for state management:
- `Map` for key-value storage (proposals, locks, votes)
- `SnapshotMap` for historical data (not heavily used)
- Composite keys for multi-dimensional lookups (e.g., `(user, lock_id)`)

### Testing Architecture

- Unit tests are in `testing_*.rs` files within contract directories
- E2E tests use CW Orchestrator to deploy and interact with contracts on a mock chain
- Interchain tests spin up real chain instances with IBC relaying
- Proptest is used for property-based testing in some modules

## Interchain Queries (ICQ)

The Hydro contract tracks validator power ratios via Neutron's ICQ system. Key points:

- Anyone can permissionlessly add a validator by paying the query creation deposit
- Contract maintains queries for top 500 validators by delegation
- Queries below the top 500 are removed and deposits kept by contract
- ICQ managers can create queries without paying from their own funds
- See `docs/run_icq_relayer.md` for relayer setup

## Migration and Upgrades

- Migrations are handled in `contracts/hydro/src/migration/`
- Each version bump requires updating `Cargo.toml` workspace version
- Migration entrypoint must be implemented in `migrate.rs`
- Pausing: Whitelist admins can pause the contract in emergencies
- Unpausing requires Cosmos Hub governance proposal to migrate (potentially to same code ID)

## Schema Generation

After modifying contract messages:
1. Run `make schema` to regenerate JSON schemas
2. TypeScript types are auto-generated in `ts_types/` directory
3. CW Orchestrator interfaces in `packages/interface/` may need manual updates

## Artifact Compilation
After modifying any contract code (even queries, though not including tests), before committing:
1. Run `make compile` to regenerate artifacts
2. Artifacts are auto-generated in `artifacts`, including their checksums

Note that for `make compile` to work, Docker has to be running.
If `make compile` fails with a message about Docker not running, ask the user to start it.

## Development Notes

- The repo uses Rust 2021 edition with cosmwasm-std 2.1.2
- Contracts target `wasm32-unknown-unknown`
- Release builds use aggressive optimization (LTO, single codegen unit)
- The workspace version (currently 3.6.6) is synchronized across all contracts

## Before Creating a PR

Run these commands before committing:

```bash
# Format code (required)
make fmt

# Run linter
make clippy

# Run unit tests
make test-unit

# Regenerate schemas if you modified contract messages
make schema

# Compile contracts (requires Docker)
make compile
```

Add artifacts, schemas, and a changelog entry to your PR.
