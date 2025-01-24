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
	cd test/interchain && go test ./... -timeout 30m

coverage:
	# to install see here: https://crates.io/crates/cargo-tarpaulin
	cargo tarpaulin --skip-clean --frozen --out html

compile: WORK_DIR=$(CURDIR)
compile: compile-inner

compile-inner:
	docker run --rm -v "$(WORK_DIR)":/code \
		--mount type=volume,source="$(notdir $(WORK_DIR))_cache",target=/target \
		--mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
		cosmwasm/optimizer:0.16.0

CONTRACTS := hydro tribute fund_calculation

schema:
	# to install ts tooling see here: https://docs.cosmology.zone/ts-codegen
	$(foreach contract,$(CONTRACTS),cd contracts/$(contract) && cargo run --bin $(contract)_schema && cd ../../;)

	$(foreach contract,$(CONTRACTS),\
		cosmwasm-ts-codegen generate \
		--plugin client \
		--schema ./contracts/$(contract)/schema \
		--out ./ts_types \
		--name $(shell echo '$(contract)' | sed 's/\b\(.\)/\u\1/g')Base \
		--no-bundle;)

	$(foreach contract,$(CONTRACTS),cd contracts/$(contract)/schema && python3 generate_full_schema.py && cd ../../../;)

build-docker-relayer:
	docker build -t neutron-org/neutron-query-relayer https://github.com/neutron-org/neutron-query-relayer.git#main