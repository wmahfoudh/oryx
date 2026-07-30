# Convenience targets for the samples folder.
BIN := ../../target/release/oryx

.PHONY: view clean

view: $(BIN)
	$(BIN) sample.kt

$(BIN):
	cargo build --release

clean:
	rm -f *.tmp
