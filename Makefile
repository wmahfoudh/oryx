release:
	cargo build --release
	mkdir -p release
	cp target/release/oryx release/
	cp -r themes release/

.PHONY: release
