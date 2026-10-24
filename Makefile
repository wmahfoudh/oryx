VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

check:
	@rustup target list --installed | grep -qx x86_64-pc-windows-gnu || { echo "the target x86_64-pc-windows-gnu is not installed: rustup target add x86_64-pc-windows-gnu"; exit 1; }
	@rustup target list --installed | grep -qx aarch64-apple-darwin || { echo "the target aarch64-apple-darwin is not installed: rustup target add aarch64-apple-darwin"; exit 1; }
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy --workspace --target x86_64-pc-windows-gnu -- -D warnings
	cargo clippy --workspace --target aarch64-apple-darwin -- -D warnings
	cargo build --workspace
	cargo test --workspace

# The fuzzers, ignored under check, in the debug profile where the
# assertions fire; PROPTEST_CASES=5000 for a deep run.
fuzz:
	cargo test --workspace fuzz_ -- --ignored

audit:
	@command -v cargo-audit >/dev/null || { echo "cargo-audit is not installed: cargo install cargo-audit"; exit 1; }
	cargo audit

release: audit
	@rustup target list --installed | grep -qx x86_64-pc-windows-gnu || { echo "the target x86_64-pc-windows-gnu is not installed: rustup target add x86_64-pc-windows-gnu"; exit 1; }
	@command -v wrestool >/dev/null || { echo "icoutils is not installed: wrestool extracts the MSI icon from the exe"; exit 1; }
	@command -v makemsix >/dev/null || { echo "makemsix is not installed: see the Tools section of internal/releasing.md"; exit 1; }
	@command -v nfpm >/dev/null || { echo "nfpm is not installed: it builds the .deb and the .rpm"; exit 1; }
	@command -v appimagetool >/dev/null || { echo "appimagetool is not installed: see the Tools section of internal/releasing.md"; exit 1; }
	@command -v patchelf >/dev/null || { echo "patchelf is not installed: it gives the AppImage's binary the path of its bundled libraries"; exit 1; }
	@command -v zsyncmake >/dev/null || { echo "zsync is not installed: zsyncmake writes the update file of the AppImage"; exit 1; }
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

channels:
	sh packaging/channels.sh $(VERSION)

# The Arch package from the release tarball, after channels wrote the
# checksums into the PKGBUILD; lands in release/ beside the other files.
arch:
	sh packaging/arch.sh $(VERSION) release

bump:
	sh packaging/bump.sh $(VERSION)

install:
	cargo build --release
	mkdir -p ~/.local/bin ~/.local/share/oryx
	install -m 755 target/release/oryx ~/.local/bin/oryx
	rm -rf ~/.local/share/oryx/themes ~/.local/share/oryx/examples
	cp -r themes ~/.local/share/oryx/themes
	cp -r examples ~/.local/share/oryx/examples
	~/.local/bin/oryx --register

.PHONY: check audit release channels arch bump install
