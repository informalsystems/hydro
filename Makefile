.PHONY: test-unit test-e2e fmt fmt-check clippy compile compile-inner coverage schema

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

clippy:
	cargo clippy --all --all-targets -- -D warnings

test-unit:
	cargo test --workspace --exclude test-e2e --lib --no-fail-fast

# run locally: make test-e2e E2E_TESTS_MNEMONIC="24 word mnemonic"
test-e2e:
	cargo test e2e --no-fail-fast -- "mnemonic: $(E2E_TESTS_MNEMONIC)"

# Note: If the neutron-org/neutron-query-relayer docker image does not exist locally, run `make build-docker-relayer` before running the interchain tests.
test-interchain:
	cd test/interchain && go test ./... -timeout 50m

coverage:
	# to install see here: https://crates.io/crates/cargo-tarpaulin
	cargo tarpaulin --skip-clean --frozen --out html

compile: WORK_DIR=$(CURDIR)
compile: compile-inner

compile-inner:
	docker run --rm -v "$(WORK_DIR)":/code \
		--mount type=volume,source="$(notdir $(WORK_DIR))_cache",target=/target \
		--mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
		cosmwasm/optimizer:0.17.0

schema:
	# to install TS tooling see here: https://docs.hyperweb.io/ts-codegen
	
	cd contracts/hydro && cargo run --bin hydro_schema
	cd contracts/tribute && cargo run --bin tribute_schema
	cd contracts/dao-voting-adapter && cargo run --bin dao_voting_adapter_schema
	cd contracts/token-info-providers/st-token-info-provider && cargo run --bin st_token_info_provider_schema
	cd contracts/token-info-providers/d-token-info-provider && cargo run --bin d_token_info_provider_schema
	cd contracts/token-info-providers/lsm-token-info-provider && cargo run --bin lsm_token_info_provider_schema
	cd contracts/gatekeeper && cargo run --bin gatekeeper_schema
	cd contracts/marketplace && cargo run --bin marketplace_schema
	cd contracts/inflow/vault && cargo run --bin inflow_vault_schema
	cd contracts/inflow/control-center && cargo run --bin inflow_control_center_schema
	cd contracts/inflow/mars-adapter && cargo run --bin inflow_mars_adapter_schema
	cd contracts/inflow/ibc-adapter && cargo run --bin inflow_ibc_adapter_schema

	$(MAKE) ts-codegen-inner SCHEMA_LOCATION=./contracts/hydro/schema NAME=HydroBase
	$(MAKE) ts-codegen-inner SCHEMA_LOCATION=./contracts/tribute/schema NAME=TributeBase
	$(MAKE) ts-codegen-inner SCHEMA_LOCATION=./contracts/dao-voting-adapter/schema NAME=DAOVotingAdapterBase
	$(MAKE) ts-codegen-inner SCHEMA_LOCATION=./contracts/token-info-providers/st-token-info-provider/schema NAME=STTokenInfoProviderBase
	$(MAKE) ts-codegen-inner SCHEMA_LOCATION=./contracts/token-info-providers/d-token-info-provider/schema NAME=DTokenInfoProviderBase
	$(MAKE) ts-codegen-inner SCHEMA_LOCATION=./contracts/token-info-providers/lsm-token-info-provider/schema NAME=LSMTokenInfoProviderBase
	$(MAKE) ts-codegen-inner SCHEMA_LOCATION=./contracts/gatekeeper/schema NAME=GatekeeperBase
	$(MAKE) ts-codegen-inner SCHEMA_LOCATION=./contracts/marketplace/schema NAME=MarketplaceBase
	$(MAKE) ts-codegen-inner SCHEMA_LOCATION=./contracts/inflow/vault/schema NAME=InflowVaultBase
	$(MAKE) ts-codegen-inner SCHEMA_LOCATION=./contracts/inflow/control-center/schema NAME=InflowControlCenterBase
	$(MAKE) ts-codegen-inner SCHEMA_LOCATION=./contracts/inflow/mars-adapter/schema NAME=InflowMarsAdapterBase
	$(MAKE) ts-codegen-inner SCHEMA_LOCATION=./contracts/inflow/ibc-adapter/schema NAME=InflowIBCAdapterBase

ts-codegen-inner:
	cosmwasm-ts-codegen generate \
          --plugin client \
          --schema $(SCHEMA_LOCATION) \
          --out ./ts_types \
          --name $(NAME) \
          --no-bundle

build-docker-relayer:
	docker build -t neutron-org/neutron-query-relayer https://github.com/neutron-org/neutron-query-relayer.git#main