# Changelog

All notable changes to NOVA Protocol will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-02-18

### Added

- Core protocol library with identity, transaction, ZKP, vault, and network modules
- NOVA ID generation using Ed25519 elliptic curve cryptography
- Zero-knowledge balance proofs using Groth16 (arkworks)
- NOVA Transfer Protocol (NTP) — complete 5-step payment flow
- Hybrid PoS/PoA consensus engine with <2s finality target
- Pedersen commitment scheme for balance hiding
- Multi-asset vault with support for tokenized fiat currencies
- Decentralized credit marketplace with real-time bidding
- On-chain credit scoring system
- P2P gossip protocol for transaction propagation
- Merkle Patricia Trie for global state management
- Standalone validator node binary with JSON-RPC API
- TypeScript SDK with full transaction builder and identity management
- Python SDK with Pydantic models and async client
- Consumer wallet web application (React + Vite + Tailwind)
- Merchant payment terminal web application
- Block explorer web application
- Smart contracts: credit escrow, dispute resolution, token factory
- Docker configurations for local devnet (4 validator nodes)
- CI/CD pipelines for lint, test, build, security audit, and release
- Comprehensive documentation and protocol specification

### Security

- Ed25519 signatures for all transaction authorization
- AES-256-GCM encryption for device-to-device communication
- X25519 Diffie-Hellman for Perfect Forward Secrecy
- Zero-knowledge proofs eliminate balance data exposure
- No shared secrets architecture — nothing to leak, nothing to phish
