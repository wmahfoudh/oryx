VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy --workspace --target x86_64-pc-windows-gnu -- -D warnings
	cargo clippy --workspace --target aarch64-apple-darwin -- -D warnings
	cargo build --workspace
	cargo test --workspace

audit:
	@command -v cargo-audit >/dev/null || { echo "cargo-audit is not installed: cargo install cargo-audit"; exit 1; }
	cargo audit

release: audit
	@command -v wrestool >/dev/null || { echo "icoutils is not installed: wrestool extracts the MSI icon from the exe"; exit 1; }
	@command -v makemsix >/dev/null || { echo "makemsix is not installed: see the Tools section of internal/releasing.md"; exit 1; }
	@command -v nfpm >/dev/null || { echo "nfpm is not installed: it builds the .deb and the .rpm"; exit 1; }
	@command -v appimagetool >/dev/null || { echo "appimagetool is not installed: see the Tools section of internal/releasing.md"; exit 1; }
	sh packaging/build-linux.sh
	cargo build --release --target x86_64-pc-windows-gnu
	rm -rf release
	mkdir -p release/linux/oryx release/windows/oryx
	cp target/jammy/release/oryx release/linux/oryx/
	cp -r themes release/linux/oryx/themes
	cp -r examples release/linux/oryx/examples
	cp LICENSE packaging/install.sh release/linux/oryx/
	tar -C release/linux -czf release/oryx-$(VERSION)-linux-x86_64.tar.gz oryx
	sh packaging/stage-linux.sh release/linux/oryx release/linux/usr
	cd release/linux && VERSION=$(VERSION) nfpm package -f ../../packaging/nfpm.yaml -p deb -t ..
	cd release/linux && VERSION=$(VERSION) nfpm package -f ../../packaging/nfpm.yaml -p rpm -t ..
	sh packaging/appimage.sh release/linux/usr release/Oryx-$(VERSION)-x86_64.AppImage
	cp target/x86_64-pc-windows-gnu/release/oryx.exe release/windows/oryx/
	cp -r themes release/windows/oryx/themes
	cp -r examples release/windows/oryx/examples
	cp LICENSE packaging/install.ps1 release/windows/oryx/
	cd release/windows && zip -qr ../oryx-$(VERSION)-windows-x86_64.zip oryx
	wrestool -x -t 14 -o release/windows/oryx.ico release/windows/oryx/oryx.exe
	sh packaging/msi.sh $(VERSION) release/windows/oryx release/oryx-$(VERSION)-windows-x86_64.msi release/windows/oryx.ico
	sh packaging/msix.sh $(VERSION) release/windows/oryx release/Oryx-$(VERSION).msix
	rm -rf release/linux release/windows
	ls -l release

install:
	cargo build --release
	mkdir -p ~/.local/bin ~/.local/share/oryx
	install -m 755 target/release/oryx ~/.local/bin/oryx
	rm -rf ~/.local/share/oryx/themes ~/.local/share/oryx/examples
	cp -r themes ~/.local/share/oryx/themes
	cp -r examples ~/.local/share/oryx/examples
	~/.local/bin/oryx --register

.PHONY: check audit release install
