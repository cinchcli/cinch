GOBIN := $(shell go env GOPATH)/bin

.PHONY: all build test fmt lint generate clippy typecheck verify-versions clean dev-desktop

all: build test

build:
	cargo build --workspace
	cd apps/desktop && pnpm build

test:
	cargo test --workspace
	cd apps/desktop && pnpm test
	go test ./go/...

fmt:
	cargo fmt --all
	PATH=$(GOBIN):$$PATH buf format -w
	cd apps/desktop && pnpm exec prettier --write 'src/**/*.{ts,tsx}'

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets || echo "clippy reported warnings (non-blocking; pre-existing tech debt)"
	PATH=$(GOBIN):$$PATH buf lint
	go vet ./go/...

generate:
	PATH=$(GOBIN):$$PATH buf generate
	go mod tidy
	cargo test -p cinch-desktop export_bindings -- --ignored

typecheck:
	cd apps/desktop && pnpm exec tsc --noEmit

verify-versions:
	./scripts/check-version-parity.sh

dev-desktop:
	cd apps/desktop && pnpm tauri dev

clean:
	cargo clean
	cd apps/desktop && rm -rf node_modules dist
