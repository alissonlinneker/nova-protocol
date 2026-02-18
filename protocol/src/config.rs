//! # Protocol Configuration & Constants
//!
//! Every magic number in NOVA lives here. If you're hardcoding a constant
//! somewhere else, you're doing it wrong and you owe the team coffee.
//!
//! These values define the DNA of the network. Changing them after mainnet
//! launch is somewhere between "difficult" and "career-ending", so choose
//! wisely during devnet.

use std::time::Duration;

// ---------------------------------------------------------------------------
// Network Identifiers
// ---------------------------------------------------------------------------

/// Mainnet — the real deal. Mistakes here cost real money.
pub const NETWORK_ID_MAINNET: u32 = 0x4E4F5641; // "NOVA" in ASCII hex. Yes, we're that cute.

/// Testnet — where we break things on purpose and call it "testing."
pub const NETWORK_ID_TESTNET: u32 = 0x4E4F5654; // "NOVT"

/// Devnet — the wild west. Reset weekly, no promises, no survivors.
pub const NETWORK_ID_DEVNET: u32 = 0x4E4F5644; // "NOVD"

/// Human-readable network prefixes for addresses.
/// Bech32 HRP values — short enough to type, long enough to be unambiguous.
pub const MAINNET_HRP: &str = "nova";
pub const TESTNET_HRP: &str = "tnova";
pub const DEVNET_HRP: &str = "dnova";

// ---------------------------------------------------------------------------
// Protocol Version
// ---------------------------------------------------------------------------

/// Protocol magic bytes used in the P2P wire format preamble. Every NOVA
/// message on the wire starts with these 4 bytes so peers can quickly reject
/// non-NOVA traffic without parsing further.
pub const PROTOCOL_MAGIC: u32 = 0x414C4153; // "ALAS" — A Ledger for Autonomous Settlement

/// Protocol fingerprint for network identification.
/// Used in handshake messages and version negotiation to uniquely identify
/// the NOVA protocol family and build generation.
pub const PROTOCOL_FINGERPRINT: &str = "ALAS-NOVA-2026";

/// Major version — bump on breaking consensus changes. A.k.a. hard forks.
pub const PROTOCOL_VERSION_MAJOR: u16 = 0;

/// Minor version — bump on backward-compatible additions.
pub const PROTOCOL_VERSION_MINOR: u16 = 1;

/// Patch version — bump on non-consensus bug fixes.
pub const PROTOCOL_VERSION_PATCH: u16 = 0;

/// The full version string, assembled at compile time so we don't allocate
/// for something this trivial at runtime.
pub const PROTOCOL_VERSION: &str = "0.1.0";

/// Wire protocol version for P2P messages. Separate from the crate version
/// because networking changes don't always mean consensus changes.
pub const WIRE_PROTOCOL_VERSION: u16 = 1;

// ---------------------------------------------------------------------------
// Cryptographic Parameters
// ---------------------------------------------------------------------------

/// Ed25519 — the only sane choice for signatures in 2024+.
/// 128-bit security level, deterministic, and resistant to side-channel
/// attacks when implemented correctly (which ed25519-dalek is).
pub const SIGNING_ALGORITHM: &str = "Ed25519";

/// Signing key length in bytes. Ed25519 secret keys are 32 bytes.
pub const SIGNING_KEY_LENGTH: usize = 32;

/// Public (verifying) key length in bytes.
pub const VERIFYING_KEY_LENGTH: usize = 32;

/// Ed25519 signature length. Always 64 bytes. If yours isn't, something
/// has gone terribly wrong.
pub const SIGNATURE_LENGTH: usize = 64;

/// X25519 for Diffie-Hellman key exchange. Same curve as Ed25519 but in
/// Montgomery form — because mathematicians enjoy making things confusing.
pub const KEY_EXCHANGE_ALGORITHM: &str = "X25519";

/// AES-256-GCM for symmetric encryption. 256-bit keys, 96-bit nonces,
/// 128-bit authentication tags. The holy trinity of authenticated encryption.
pub const SYMMETRIC_ALGORITHM: &str = "AES-256-GCM";

/// AES-256-GCM key length in bytes.
pub const AES_KEY_LENGTH: usize = 32;

/// AES-256-GCM nonce length in bytes. 96 bits is the standard and the only
/// length you should use. 12 bytes. Not 16. Not 8. Twelve.
pub const AES_NONCE_LENGTH: usize = 12;

/// AES-256-GCM authentication tag length in bytes.
pub const AES_TAG_LENGTH: usize = 16;

/// The hash function we use for transaction IDs and Merkle trees.
/// BLAKE3 is faster than SHA-256 on every platform that matters, and it's
/// a proper cryptographic hash — not a toy.
pub const PRIMARY_HASH_FUNCTION: &str = "BLAKE3";

/// Hash output length in bytes. Both SHA-256 and BLAKE3 produce 32-byte digests.
pub const HASH_OUTPUT_LENGTH: usize = 32;

/// ZKP curve: BN254 (a.k.a. alt_bn128). Chosen because:
/// 1. Groth16 support is mature in arkworks.
/// 2. Ethereum precompiles exist for it (interop matters).
/// 3. The proving times are acceptable for payment-sized circuits.
/// Yes, BLS12-381 has better security margins, but we need EVM compatibility.
pub const ZKP_CURVE: &str = "BN254";

/// Maximum circuit constraint count we're willing to tolerate.
/// Beyond this, proving times get ugly on consumer hardware.
pub const MAX_ZKP_CONSTRAINTS: usize = 1 << 20; // ~1M constraints

// ---------------------------------------------------------------------------
// Timing Constants
// ---------------------------------------------------------------------------

/// Target block time. 2 seconds is aggressive but achievable with our
/// consensus mechanism. Fast enough for payments, slow enough for validators
/// to keep up on commodity hardware.
pub const BLOCK_TIME: Duration = Duration::from_secs(2);

/// Block time as milliseconds — because some APIs want a u64, not a Duration.
/// Keep this in sync with BLOCK_TIME or face the wrath of integration tests.
pub const BLOCK_TIME_MS: u64 = 2_000;

/// Finality timeout. After this many seconds without finalization, something
/// is seriously wrong and the node should start screaming (via alerts).
/// 30 seconds = 15 blocks. If we can't finalize in 15 blocks, the network
/// has bigger problems than a timeout.
pub const FINALITY_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum clock skew tolerated between validators. NTP keeps us honest,
/// but we allow 500ms of drift because the real world is messy.
pub const MAX_CLOCK_SKEW: Duration = Duration::from_millis(500);

/// How often validators should sync their clocks via NTP.
/// Every 60 seconds is plenty — clocks don't drift *that* fast.
pub const NTP_SYNC_INTERVAL: Duration = Duration::from_secs(60);

/// Transaction expiry window. Transactions older than this are rejected.
/// 5 minutes is generous — if your tx hasn't been included by then,
/// something went wrong and you should resubmit with a higher fee.
pub const TX_EXPIRY_WINDOW: Duration = Duration::from_secs(300);

/// Heartbeat interval for P2P connections. If a peer doesn't heartbeat
/// within 2x this interval, they're presumed dead.
pub const PEER_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

/// Peer connection timeout. 10 seconds to establish a connection or we
/// move on. Life's too short for slow peers.
pub const PEER_CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// Fee Parameters
// ---------------------------------------------------------------------------

/// Minimum transaction fee in the smallest unit (called "photons" because
/// every crypto project needs a cute name for its smallest denomination).
/// 100 photons is basically free, but enough to prevent spam.
pub const MIN_TX_FEE_PHOTONS: u64 = 100;

/// Base fee per byte of transaction data. Larger transactions pay more.
/// This prevents people from stuffing arbitrary data into the chain
/// (looking at you, NFT inscriptions).
pub const FEE_PER_BYTE: u64 = 10;

/// Fee multiplier for private (shielded) transactions. ZK proofs are
/// computationally expensive to verify, so shielded txs cost 3x.
/// Still cheaper than the privacy you get.
pub const SHIELDED_TX_FEE_MULTIPLIER: u64 = 3;

/// Maximum fee cap. No transaction should ever need to pay more than this.
/// If the network is so congested that fees hit this ceiling, we have
/// a capacity problem, not a fee problem.
pub const MAX_TX_FEE_PHOTONS: u64 = 10_000_000;

/// Fee precision — number of decimal places in the fee currency.
/// 8 decimals, same as Bitcoin. We're not reinventing this wheel.
pub const FEE_DECIMALS: u8 = 8;

// ---------------------------------------------------------------------------
// Transaction Limits
// ---------------------------------------------------------------------------

/// Maximum transaction size in bytes. 256 KiB should be enough for anyone.
/// (Famous last words, but we can always bump this in a future version.)
pub const MAX_TX_SIZE_BYTES: usize = 256 * 1024;

/// Maximum number of inputs per transaction. Keeps validation bounded.
pub const MAX_TX_INPUTS: usize = 256;

/// Maximum number of outputs per transaction.
pub const MAX_TX_OUTPUTS: usize = 256;

/// Maximum memo field length in bytes. Enough for a short message,
/// not enough for your novel.
pub const MAX_MEMO_LENGTH: usize = 512;

// ---------------------------------------------------------------------------
// Network Parameters
// ---------------------------------------------------------------------------

/// Default P2P listening port. 9740 = "N" (78) * 100 + "O" (79) - 39.
/// Just kidding, we picked it because it wasn't taken.
pub const DEFAULT_P2P_PORT: u16 = 9740;

/// Default RPC API port.
pub const DEFAULT_RPC_PORT: u16 = 9741;

/// Default metrics (Prometheus) port.
pub const DEFAULT_METRICS_PORT: u16 = 9742;

/// Maximum number of connected peers. Too many and you're wasting bandwidth
/// on gossip. Too few and you're at risk of network partitions.
pub const MAX_PEERS: usize = 50;

/// Minimum peers required for a node to consider itself "connected enough"
/// to participate in consensus. Below this, the node should be in catch-up mode.
pub const MIN_PEERS_FOR_CONSENSUS: usize = 3;

/// Gossip fanout — number of peers to forward each message to.
/// 8 gives us good propagation with manageable bandwidth.
pub const GOSSIP_FANOUT: usize = 8;

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Maximum RocksDB write buffer size. 64 MiB per column family.
pub const ROCKSDB_WRITE_BUFFER_SIZE: usize = 64 * 1024 * 1024;

/// Maximum number of write buffers before stalling.
pub const ROCKSDB_MAX_WRITE_BUFFERS: i32 = 3;

/// Block cache size for RocksDB reads. 256 MiB is a good default.
/// Tune up on beefy validator nodes, tune down on resource-constrained ones.
pub const ROCKSDB_BLOCK_CACHE_SIZE: usize = 256 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

/// Returns the human-readable prefix for a given network ID.
/// Returns `None` for unrecognized networks — we don't guess.
pub fn hrp_for_network(network_id: u32) -> Option<&'static str> {
    match network_id {
        NETWORK_ID_MAINNET => Some(MAINNET_HRP),
        NETWORK_ID_TESTNET => Some(TESTNET_HRP),
        NETWORK_ID_DEVNET => Some(DEVNET_HRP),
        _ => None,
    }
}

/// Returns a friendly name for a network ID, mainly for logging.
/// Unknown networks get a hex dump because we're helpful like that.
pub fn network_name(network_id: u32) -> String {
    match network_id {
        NETWORK_ID_MAINNET => "mainnet".to_string(),
        NETWORK_ID_TESTNET => "testnet".to_string(),
        NETWORK_ID_DEVNET => "devnet".to_string(),
        other => format!("unknown(0x{:08X})", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_ids_are_distinct() {
        // If these collide, someone has been editing hex while sleep-deprived.
        assert_ne!(NETWORK_ID_MAINNET, NETWORK_ID_TESTNET);
        assert_ne!(NETWORK_ID_MAINNET, NETWORK_ID_DEVNET);
        assert_ne!(NETWORK_ID_TESTNET, NETWORK_ID_DEVNET);
    }

    #[test]
    fn test_protocol_magic_is_valid_ascii() {
        // The magic bytes should decode to a readable 4-char ASCII tag.
        let bytes = PROTOCOL_MAGIC.to_be_bytes();
        assert!(bytes.iter().all(|b| b.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_protocol_fingerprint_format() {
        // Fingerprint must be non-empty and contain the protocol family name.
        assert!(!PROTOCOL_FINGERPRINT.is_empty());
        assert!(PROTOCOL_FINGERPRINT.contains("NOVA"));
    }

    #[test]
    fn test_hrp_for_known_networks() {
        assert_eq!(hrp_for_network(NETWORK_ID_MAINNET), Some("nova"));
        assert_eq!(hrp_for_network(NETWORK_ID_TESTNET), Some("tnova"));
        assert_eq!(hrp_for_network(NETWORK_ID_DEVNET), Some("dnova"));
    }

    #[test]
    fn test_hrp_for_unknown_network() {
        assert_eq!(hrp_for_network(0xDEADBEEF), None);
    }

    #[test]
    fn test_network_name_formatting() {
        assert_eq!(network_name(NETWORK_ID_MAINNET), "mainnet");
        assert_eq!(network_name(0xCAFEBABE), "unknown(0xCAFEBABE)");
    }

    #[test]
    fn test_timing_constants_sanity() {
        // Block time should be positive and less than finality timeout.
        // If finality is faster than block production, we have a physics problem.
        assert!(BLOCK_TIME < FINALITY_TIMEOUT);
        assert!(BLOCK_TIME.as_millis() > 0);
        assert_eq!(BLOCK_TIME.as_millis() as u64, BLOCK_TIME_MS);
    }

    #[test]
    fn test_fee_constants_sanity() {
        // Min fee should be less than max fee. Obvious, but stranger things
        // have shipped to production.
        assert!(MIN_TX_FEE_PHOTONS < MAX_TX_FEE_PHOTONS);
        assert!(FEE_PER_BYTE > 0);
    }

    #[test]
    fn test_crypto_parameter_sizes() {
        assert_eq!(SIGNING_KEY_LENGTH, 32);
        assert_eq!(VERIFYING_KEY_LENGTH, 32);
        assert_eq!(SIGNATURE_LENGTH, 64);
        assert_eq!(AES_KEY_LENGTH, 32);
        assert_eq!(AES_NONCE_LENGTH, 12);
        assert_eq!(HASH_OUTPUT_LENGTH, 32);
    }
}
