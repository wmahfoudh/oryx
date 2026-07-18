check:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo clippy --target x86_64-pc-windows-gnu -- -D warnings
	cargo clippy --target aarch64-apple-darwin -- -D warnings
	cargo build
	cargo test

release:
	cargo build --release
	mkdir -p release
	cp target/release/oryx release/
	cp -r themes release/

.PHONY: check release
