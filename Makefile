.PHONY: test fmt clippy compile compile-rust-optimizer coverage

fmt:
	@cargo fmt --all

clippy:
	@cargo clippy --all --all-targets -- -D warnings

test:
	@cargo test

coverage:
	# to install see here: https://crates.io/crates/cargo-tarpaulin
	@cargo tarpaulin --skip-clean --frozen --out html

compile:
	@RUSTFLAGS='-C link-arg=-s' cargo build --release --target wasm32-unknown-unknown

compile-rust-optimizer:
	@docker run --rm -v "$(CURDIR)":/code \
		--mount type=volume,source="$(notdir $(CURDIR))_cache",target=/target \
		--mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
		cosmwasm/optimizer:0.15.0
