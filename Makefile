.PHONY: test-unit test-e2e fmt clippy compile compile-rust-optimizer coverage schema

fmt:
	@cargo fmt --all

clippy:
	@cargo clippy --all --all-targets -- -D warnings

test-unit:
	@cargo test --lib --no-fail-fast --workspace --exclude test-e2e

test-e2e:
	@cargo test e2e --no-fail-fast -- "mnemonic: [SET_PRIVATE_MNEMONIC]"

coverage:
	# to install see here: https://crates.io/crates/cargo-tarpaulin
	@cargo tarpaulin --skip-clean --frozen --out html

compile:
	@RUSTFLAGS='-C link-arg=-s' cargo build --release --target wasm32-unknown-unknown --lib

compile-rust-optimizer:
	@docker run --rm -v "$(CURDIR)":/code \
		--mount type=volume,source="$(notdir $(CURDIR))_cache",target=/target \
		--mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
		cosmwasm/optimizer:0.16.0

schema:
	# to install ts tooling see here: https://docs.cosmology.zone/ts-codegen
	cd contracts/hydro && cargo run --bin hydro_schema
	cd contracts/tribute && cargo run --bin tribute_schema

	cosmwasm-ts-codegen generate \
          --plugin client \
          --schema ./contracts/hydro/schema \
          --out ./ts_types \
          --name HydroBase \
          --no-bundle
	cosmwasm-ts-codegen generate \
          --plugin client \
          --schema ./contracts/tribute/schema \
          --out ./ts_types \
          --name TributeBase \
          --no-bundle