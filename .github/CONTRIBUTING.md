# Contributing to NOVA Protocol

Thank you for your interest in contributing to NOVA Protocol. This guide covers the development workflow, coding standards, and submission process.

## Table of Contents

- [Development Setup](#development-setup)
- [Building the Project](#building-the-project)
- [Running Tests](#running-tests)
- [Code Style](#code-style)
- [Commit Message Format](#commit-message-format)
- [Pull Request Process](#pull-request-process)
- [Release Process](#release-process)
- [Getting Help](#getting-help)

## Development Setup

### Prerequisites

| Tool       | Version  | Purpose                       |
|------------|----------|-------------------------------|
| Rust       | >= 1.75  | Core protocol and node        |
| Node.js    | >= 18    | TypeScript SDK and web apps   |
| Python     | >= 3.10  | Python SDK                    |
| Docker     | >= 24    | Local devnet and CI           |
| `jq`       | any      | Scripts                       |

### Initial Setup

```bash
# Clone the repository
git clone https://github.com/nova-protocol/nova-protocol.git
cd nova-protocol

# Install all dependencies
make dev-setup

# Verify everything compiles
make build

# Run the test suite
make test
```

### Local Development Network

Spin up a 4-validator devnet for integration testing:

```bash
# Start a fresh devnet
./scripts/setup-devnet.sh

# Or wipe all data and restart
./scripts/setup-devnet.sh --clean
```

This starts 4 validator nodes (ports 8080-8083), an API gateway (port 8090), and a block explorer (port 3000).

## Building the Project

```bash
# Debug build (fast compilation)
make build

# Release build (optimized)
make release

# Docker images
make docker-build
```

### Workspace Structure

| Crate / Package   | Path               | Description                    |
|--------------------|--------------------|--------------------------------|
| `nova-protocol`   | `protocol/`        | Core protocol library          |
| `nova-node`       | `node/`            | Validator node binary          |
| `nova-contracts`  | `contracts/`       | Smart contract runtime         |
| `@nova-protocol/sdk` | `sdk/typescript/` | TypeScript SDK              |
| `nova-sdk`        | `sdk/python/`      | Python SDK                     |
| Explorer           | `apps/explorer/`   | Block explorer web app         |
| Wallet             | `apps/wallet-web/` | Web wallet application         |

## Running Tests

```bash
# All Rust tests
make test

# Specific crate
make test-protocol
make test-node
make test-contracts

# TypeScript SDK
make test-sdk-ts

# Python SDK
make test-sdk-py

# Everything (Rust + TypeScript + Python)
make test-all

# Benchmarks
make bench
```

## Code Style

### Rust

- Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/).
- Format with `cargo fmt` (enforced in CI).
- No clippy warnings (`cargo clippy -- -D warnings`).
- All public items must have doc comments (`///`).
- Use `thiserror` for library errors, `anyhow` in binaries.
- Prefer strong types over primitives (e.g., `NovaId` over `String`).

```bash
# Format and lint before committing
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

### TypeScript

- Strict TypeScript (`strict: true` in tsconfig).
- ESLint for linting (`npm run lint` in `sdk/typescript/`).
- Prefer `readonly` properties where applicable.
- Use named exports, avoid default exports.

### Python

- Target Python >= 3.10.
- Type annotations on all public functions.
- Format with `black` and lint with `ruff`.
- Use `pydantic` models for structured data.

## Commit Message Format

We follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):

```
<type>(<scope>): <subject>

[optional body]

[optional footer(s)]
```

### Types

| Type       | Description                                    |
|------------|------------------------------------------------|
| `feat`     | A new feature                                  |
| `fix`      | A bug fix                                      |
| `docs`     | Documentation changes only                     |
| `style`    | Formatting, missing semicolons, etc.           |
| `refactor` | Code change that neither fixes a bug nor adds a feature |
| `perf`     | Performance improvement                        |
| `test`     | Adding or updating tests                       |
| `build`    | Build system or external dependencies          |
| `ci`       | CI/CD configuration                            |
| `chore`    | Maintenance tasks                              |

### Scopes

Use the crate or package name: `protocol`, `node`, `contracts`, `sdk-ts`, `sdk-py`, `explorer`, `wallet`, `docker`, `ci`.

### Examples

```
feat(protocol): add Pedersen commitment scheme for confidential transfers
fix(node): resolve peer discovery timeout on network partition
docs(sdk-ts): add examples for identity creation
ci: add weekly security audit workflow
```

## Pull Request Process

1. **Fork** the repository and create a branch from `main`.
2. **Branch naming**: `<type>/<short-description>` (e.g., `feat/confidential-transfers`, `fix/peer-timeout`).
3. **Make your changes** following the code style guidelines.
4. **Write tests** that cover the new behavior or fix.
5. **Run the full check locally**:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
6. **Commit** using conventional commit messages.
7. **Open a pull request** against `main` and fill in the PR template.
8. **Address review feedback** promptly. Push fixes as new commits (do not force-push during review).
9. Once approved and CI passes, a maintainer will merge your PR.

### Review Criteria

- All CI checks pass.
- Code is covered by tests.
- No new clippy warnings.
- Public APIs are documented.
- CHANGELOG is updated for user-facing changes.
- Breaking changes are clearly noted and justified.

## Release Process

Releases follow [Semantic Versioning](https://semver.org/):

1. Maintainers update version numbers in `Cargo.toml`, `package.json`, and `pyproject.toml`.
2. Update `CHANGELOG.md` with the release notes.
3. Create and push a tag: `git tag v0.2.0 && git push origin v0.2.0`.
4. The release workflow automatically:
   - Builds binaries for all platforms.
   - Creates a GitHub Release with artifacts.
   - Pushes Docker images to GHCR.
   - Publishes the TypeScript SDK to npm.
   - Publishes the Python SDK to PyPI.

## Getting Help

- **Discussions**: [GitHub Discussions](https://github.com/nova-protocol/nova-protocol/discussions) for questions and ideas.
- **Issues**: [GitHub Issues](https://github.com/nova-protocol/nova-protocol/issues) for bugs and feature requests.
- **Security**: See [SECURITY.md](../SECURITY.md) for vulnerability reporting.

We appreciate every contribution, whether it is code, documentation, bug reports, or feedback. Thank you for helping build NOVA Protocol.
