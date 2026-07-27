VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

check:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo clippy --target x86_64-pc-windows-gnu -- -D warnings
	cargo clippy --target aarch64-apple-darwin -- -D warnings
	cargo build
	cargo test

release:
	cargo build --release
	cargo build --release --target x86_64-pc-windows-gnu
	rm -rf release
	mkdir -p release/linux/oryx release/windows/oryx
	cp target/release/oryx release/linux/oryx/
	cp -r themes release/linux/oryx/themes
	cp LICENSE packaging/install.sh release/linux/oryx/
	tar -C release/linux -czf release/oryx-$(VERSION)-linux-x86_64.tar.gz oryx
	cp target/x86_64-pc-windows-gnu/release/oryx.exe release/windows/oryx/
	cp -r themes release/windows/oryx/themes
	cp LICENSE packaging/install.ps1 release/windows/oryx/
	cd release/windows && zip -qr ../oryx-$(VERSION)-windows-x86_64.zip oryx
	rm -rf release/linux release/windows
	ls -l release

install:
	cargo build --release
	mkdir -p ~/.local/bin ~/.local/share/oryx
	install -m 755 target/release/oryx ~/.local/bin/oryx
	rm -rf ~/.local/share/oryx/themes
	cp -r themes ~/.local/share/oryx/themes
	~/.local/bin/oryx --register

.PHONY: check release install
