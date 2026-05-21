.PHONY: dev build test typecheck check clippy clean

dev:
	npm run tauri dev

build:
	npm run tauri build

test:
	npm test

typecheck:
	npx tsc --noEmit

check:
	cargo check --manifest-path src-tauri/Cargo.toml

clippy:
	cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

clean:
	rm -rf dist/ src-tauri/target/
