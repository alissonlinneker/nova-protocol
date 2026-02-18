.PHONY: all build test lint clean dev-setup devnet docs bench fmt check

# Default target
all: build test

# ============================================================================
# Build targets
# ============================================================================

build:
	cargo build --workspace

release:
	cargo build --workspace --release

# ============================================================================
# Testing
# ============================================================================

test:
	cargo test --workspace

test-verbose:
	cargo test --workspace -- --nocapture

test-protocol:
	cargo test -p nova-protocol

test-node:
	cargo test -p nova-node

test-contracts:
	cargo test -p nova-contracts

test-sdk-ts:
	cd sdk/typescript && npm test

test-sdk-py:
	cd sdk/python && python -m pytest tests/ -v

test-all: test test-sdk-ts test-sdk-py

# ============================================================================
# Code quality
# ============================================================================

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --workspace --all-targets -- -D warnings

check:
	cargo check --workspace

# ============================================================================
# Benchmarks
# ============================================================================

bench:
	cargo bench --workspace

bench-signing:
	cargo bench -p nova-protocol --bench signing_bench

bench-zkp:
	cargo bench -p nova-protocol --bench zkp_bench

bench-consensus:
	cargo bench -p nova-protocol --bench consensus_bench

# ============================================================================
# Development environment
# ============================================================================

dev-setup:
	@echo "Installing Rust toolchain..."
	rustup update stable
	rustup component add clippy rustfmt
	@echo "Installing Node.js dependencies..."
	cd sdk/typescript && npm install
	cd apps/wallet-web && npm install
	cd apps/merchant-terminal && npm install
	cd apps/explorer && npm install
	@echo "Installing Python dependencies..."
	cd sdk/python && pip install -e ".[dev]"
	@echo "Development environment ready."

devnet:
	./scripts/setup-devnet.sh

genesis:
	./scripts/generate-genesis.sh

fund-accounts:
	./scripts/fund-test-accounts.sh

# ============================================================================
# Docker
# ============================================================================

docker-build:
	docker compose -f docker/docker-compose.yml build

docker-up:
	docker compose -f docker/docker-compose.yml up -d

docker-down:
	docker compose -f docker/docker-compose.yml down

docker-logs:
	docker compose -f docker/docker-compose.yml logs -f

# ============================================================================
# Web applications
# ============================================================================

wallet-dev:
	cd apps/wallet-web && npm run dev

merchant-dev:
	cd apps/merchant-terminal && npm run dev

explorer-dev:
	cd apps/explorer && npm run dev

# ============================================================================
# Cleanup
# ============================================================================

clean:
	cargo clean
	rm -rf sdk/typescript/node_modules sdk/typescript/dist
	rm -rf apps/wallet-web/node_modules apps/wallet-web/dist
	rm -rf apps/merchant-terminal/node_modules apps/merchant-terminal/dist
	rm -rf apps/explorer/node_modules apps/explorer/dist
	rm -rf sdk/python/.eggs sdk/python/*.egg-info
	find . -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true
