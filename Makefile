.PHONY: test fmt fmt-fix compile compile-rust-optimizer

fmt:
	@cargo fmt --all --check

fmt-fix:
	@cargo fmt --all

test:
	@cargo test

compile:
	@RUSTFLAGS='-C link-arg=-s' cargo build --release --target wasm32-unknown-unknown

compile-rust-optimizer:
	@docker run --rm -v "$(CURDIR)":/code \
		--mount type=volume,source="$(notdir $(CURDIR))_cache",target=/target \
		--mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
		cosmwasm/optimizer:0.15.0
