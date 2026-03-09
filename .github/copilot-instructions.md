# GitHub Copilot Instructions for Hydro

## Project Overview

Hydro is an auction platform for the Cosmos Hub that enables ATOM stakers to lock their staked tokens in exchange for voting power. Users vote on proposals to deploy community pool tokens into liquidity positions on decentralized exchanges as Protocol Owned Liquidity (PoL).

This repository contains:
- **CosmWasm smart contracts** (Rust) - The core Hydro platform and related contracts
- **TypeScript tooling** - Build and testing utilities using wasmkit and cw-orchestrator
- **Go testing** - End-to-end tests using Interchain Test framework

## Technology Stack

- **Primary Language**: Rust (CosmWasm smart contracts)
- **Blockchain Platform**: Cosmos SDK / Neutron
- **Smart Contract Framework**: CosmWasm 2.0
- **Testing Frameworks**: 
  - Rust unit tests
  - cw-orchestrator for e2e tests (Rust)
  - Interchain Test for integration tests (Go)
- **Additional Languages**: TypeScript, Go

## Project Structure

```
contracts/              # CosmWasm smart contracts
├── hydro/             # Main Hydro auction platform contract
├── tribute/           # Tribute management contract
├── dao-voting-adapter/
├── marketplace/
├── gatekeeper/
├── inflow/
├── liquid-collateral/
└── token-info-providers/
packages/              # Shared packages
├── interface/         # cw-orchestrator interfaces
└── cw-orch-interface/
test/                  # Test suites
├── e2e/              # End-to-end tests (cw-orchestrator)
└── interchain/       # Interchain tests (Go)
tools/                # Deployment and tooling scripts
```

## Development Commands

### Building
- `cargo build` - Build all contracts
- `make compile` - Compile contracts to WASM using Docker optimizer
- `make schema` - Generate schemas and TypeScript types

### Testing
- `make test-unit` - Run Rust unit tests
- `make test-e2e` - Run end-to-end tests (requires E2E_TESTS_MNEMONIC)
- `make test-interchain` - Run Go interchain tests (requires Docker relayer)
- `cargo test --workspace --lib` - Run all library tests

### Code Quality
- `make fmt` - Format Rust code with rustfmt
- `make fmt-check` - Check Rust code formatting
- `make clippy` - Run Clippy linter with strict warnings (`-D warnings`)
- `make coverage` - Generate code coverage reports with cargo-tarpaulin

## Coding Standards

### Rust/CosmWasm
- Follow standard Rust conventions and idioms
- Use `cargo fmt` for consistent formatting
- Code must pass `cargo clippy` with zero warnings (`-D warnings`)
- All public functions and types should have documentation comments
- Write comprehensive unit tests for contract logic
- Handle all error cases explicitly using `Result` types
- Use CosmWasm 2.0 features and patterns

### Error Handling
- Use `thiserror` for custom error types
- Provide clear, actionable error messages
- Never use `unwrap()` or `expect()` in production code paths
- Propagate errors with `?` operator where appropriate

### Security Considerations
- **Critical**: This project holds user funds - security is paramount
- All user inputs must be validated
- Check for integer overflow/underflow (overflow-checks enabled in release)
- Be mindful of gas limits and potential DoS vectors
- Avoid unbounded iterations over user-supplied data
- Review permission structures carefully (whitelist admin, ICQ managers, etc.)
- Consider reentrancy and state consistency issues
- Never log or expose sensitive information

### CosmWasm Specific
- Use `cw-storage-plus` for state management
- Implement proper migration handlers
- Follow CosmWasm best practices for contract upgrades
- Be careful with cross-contract calls and replies
- Consider query complexity and gas costs
- Use Interchain Queries (ICQ) appropriately for validator data

## Key Concepts

### Locking and Voting Power
- Users lock LSM (Liquid Staking Module) shares to gain voting power
- Voting power = locked_atoms × duration_scaling_factor
- Duration scaling factor depends on remaining lockup time
- Different validators have different power ratios (shares vs atoms)

### Rounds, Tranches, and Proposals
- Voting happens in monthly rounds
- Multiple tranches exist (general, ICS platform projects)
- Proposals are submitted during current round
- Liquidity deployment occurs based on previous round results

### Permission Structure
1. **Whitelist Admin**: Manages permissions, can pause contract
2. **Whitelisted Addresses**: Can submit proposals
3. **ICQ Managers**: Can create Interchain Queries and withdraw native denom

## Documentation Files
- `README.md` - Project overview and main features
- `CONTRIBUTING` - Contribution guidelines
- `TESTING.md` - Testing framework documentation
- `DEPLOYING.md` - Deployment procedures
- `RELEASE_PROCESS.md` - Release workflow
- Litepaper: https://forum.cosmos.network/t/atom-wars-introducing-the-hydro-auction-platform/13842
- Website: https://hydro.cosmos.network/

## Pull Request Guidelines
- Create a new branch for each contribution
- Include a clear problem statement and description of changes
- Reference related issues
- Ensure all tests pass before submitting
- Update documentation if changing public interfaces
- Follow the existing code style and patterns

## Common Tasks

### Adding a New Contract
1. Create contract directory under `contracts/`
2. Add to workspace members in root `Cargo.toml`
3. Implement standard CosmWasm entry points (instantiate, execute, query, migrate)
4. Create schema generation binary in `src/bin/` directory (e.g., `contract_name_schema.rs`)
5. Add to Makefile schema generation targets
6. Write comprehensive unit tests
7. Create cw-orchestrator interface in `packages/interface/`
8. Add e2e tests in `test/e2e/`

### Modifying Contract State
1. Update state structures in `state.rs`
2. Consider migration implications
3. Implement proper `migrate()` handler
4. Update schema generation
5. Test migration path thoroughly

### Adding New Query or Execute Messages
1. Add message variant to appropriate enum (ExecuteMsg, QueryMsg)
2. Implement handler in `contract.rs`
3. Add validation logic
4. Write unit tests for new functionality
5. Update schema and TypeScript types
6. Add e2e test coverage

## Testing Strategy
- Write unit tests for all business logic
- Use property-based testing (proptest) for invariants
- Create e2e tests using cw-orchestrator for contract interactions
- Use Interchain Test for cross-chain scenarios
- Test migration paths when modifying state
- Mock external dependencies appropriately
- Test edge cases and error conditions

## Important Notes
- Contract can be paused by whitelist admin in emergencies
- Unpausing requires Cosmos Hub governance proposal
- ICQ system limits validators to top 500 by delegation
- LSM shares represent delegated stakes with varying ratios
- User funds are held in contract - extreme care required
