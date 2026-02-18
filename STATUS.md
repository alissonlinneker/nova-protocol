# Project Status

> **Current Release: v0.1.0-alpha (Devnet Only)**
>
> NOVA Protocol is in active development. This document describes what works,
> what is in progress, and what has not been built yet.

---

## What Works (Phase 0 -- Foundation)

### Cryptography
- [x] Ed25519 key generation, signing, and verification (ed25519-dalek 2.1)
- [x] X25519 Diffie-Hellman for Perfect Forward Secrecy
- [x] AES-256-GCM authenticated encryption with random nonces
- [x] BLAKE3 hashing (primary), SHA-256 (cross-chain compatibility)
- [x] Pedersen commitment scheme (EC-based and scalar-based)

### Identity
- [x] NOVA ID: BLAKE3(public_key) encoded as Bech32 with "nova" HRP
- [x] W3C DID Core v1.0 compatible documents (`did:nova:<address>`)
- [x] Shamir's Secret Sharing for key recovery (threshold scheme over GF(256))

### Transactions
- [x] Transaction builder with fluent API and deterministic serialization
- [x] Ed25519 signing with sender public key binding
- [x] 9-step verification pipeline (nonce, amount, self-transfer, timestamp, ID integrity, signature presence, address validity, key-to-address binding, Ed25519 verification)
- [x] Multi-currency support (NOVA, BRL, USD, EUR, BTC)
- [x] Transaction types: Transfer, CreditRequest, CreditSettlement, TokenMint, TokenBurn

### Zero-Knowledge Proofs
- [x] Groth16 proof system over BN254 (arkworks 0.4)
- [x] Balance proof circuit: Pedersen commitment correctness + 64-bit range proof
- [x] Proof generation and verification with serialization roundtrip
- [ ] Integration with transaction pipeline (proofs are not attached to transactions yet)

### Protocol (NTP)
- [x] Handshake: X25519 key exchange with session establishment
- [x] Broadcast: Transaction preparation, signing, and gossip message packaging
- [x] Settlement: Validation pipeline and state machine (Pending -> Confirmed/Rejected/TimedOut)
- [ ] Encrypted session traffic (shared secret is derived but not used for encryption)

### Network
- [x] Consensus engine: ValidatorSet, round-robin proposer, BFT quorum (2/3 + 1)
- [x] Gossip protocol: Epidemic propagation, deduplication, TTL enforcement
- [ ] Actual network transport (TCP/UDP/libp2p) -- consensus and gossip are in-memory only
- [ ] Multi-node communication

### Storage
- [x] Block structure with BLAKE3 header hash and Merkle transaction root
- [x] Genesis block creation and chain linking
- [ ] Database persistence (NovaDB is defined but not implemented)
- [ ] State tree (account balances, nonces)
- [ ] Mempool

### Node
- [x] HTTP server with axum (REST + WebSocket + JSON-RPC 2.0)
- [x] CLI: `run`, `init` (keypair generation), `status`, `version`
- [x] API endpoints: `/health`, `/status`, `/rpc`, `/ws`, `/validators`, `/blocks/:height`, `/transactions/:hash`, `/accounts/:address`
- [ ] Real block production (currently a stub counter)
- [ ] API connected to live data (currently returns placeholder responses)

### Smart Contracts
- [x] Credit escrow lifecycle (Pending -> Funded -> Active -> Completed/Defaulted/Disputed)
- [x] Interest calculation, dispute resolution, checked arithmetic
- [ ] Contract execution runtime (VM/WASM)
- [ ] Contract deployment and invocation mechanism

### SDKs
- [x] TypeScript: Ed25519 (@noble/ed25519), Bech32, TransactionBuilder, NovaClient (JSON-RPC + WebSocket), NovaWallet, credit marketplace functions
- [x] Python: Ed25519 (PyNaCl), Bech32, async httpx client, Pydantic V2 models
- [ ] Wire format alignment with Rust protocol (signing schemes differ)

### Web Applications
- [x] Consumer wallet (React + Vite + Tailwind + Zustand): Dashboard, Send/Receive, Credit Market, Transaction History, Identity
- [x] Merchant terminal: Numpad, QR generation, payment flow, analytics
- [x] Block explorer: Block list, transaction detail, address view, network stats, search
- [ ] Connection to real node (all apps use mock data)

### Infrastructure
- [x] Docker Compose 4-node devnet configuration
- [x] CI/CD: lint, test, build, security audit workflows
- [x] Release workflow with cross-platform binaries
- [x] cargo-deny for license and advisory checking

---

## Test Coverage

| Component | Tests | Status |
|-----------|-------|--------|
| Rust (protocol + node + contracts) | 415 | All passing |
| TypeScript SDK | 39 | All passing |
| Python SDK | 43 | All passing |
| **Total** | **497** | **All passing** |

---

## What Is Not Built Yet (Phase 1+)

These items are on the roadmap but have not been implemented:

- Database persistence layer
- Peer-to-peer networking (libp2p or equivalent)
- ZKP integration into transaction validation
- State tree for world state (balances, nonces)
- Transaction mempool
- Real block production and finalization
- MPC trusted setup ceremony for Groth16
- Credit marketplace (on-chain scoring, bidding)
- Contract execution runtime
- Security audit
- Public testnet

---

## Security Notice

This code has **not been independently audited**. The cryptographic implementations use well-known, audited libraries (ed25519-dalek, aes-gcm, x25519-dalek, arkworks), but the protocol logic, circuit design, and integration code have not been reviewed by external security researchers.

**Do not use this software for production transactions or with real funds.**

See [SECURITY.md](SECURITY.md) for vulnerability reporting.
