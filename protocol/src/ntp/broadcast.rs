//! # NTP Step 3 — Sign and Broadcast
//!
//! After the sender proves they have sufficient funds, they construct
//! the actual on-chain transaction, sign it with their Ed25519 key, and
//! broadcast it to the NOVA network for inclusion in a block.
//!
//! This module handles three responsibilities:
//!
//! 1. **Transaction preparation** — Build the unsigned transaction from
//!    the session's payment parameters.
//! 2. **Signing** — Attach the sender's Ed25519 signature to the
//!    transaction payload.
//! 3. **Broadcast packaging** — Wrap the signed transaction with network
//!    metadata (propagation TTL, priority hints) for gossip.
//!
//! ## Wire Format
//!
//! The `BroadcastMessage` is what gets transmitted over the libp2p gossipsub
//! topic `nova/txs/1.0.0`. Validators pull transactions from this topic
//! and submit them to the mempool.

use serde::{Deserialize, Serialize};

use crate::config;
use crate::crypto::keys::NovaKeypair;
use crate::transaction::builder::{Transaction, TransactionBuilder};
use crate::transaction::signing::sign_transaction;
use crate::transaction::types::{Amount, TransactionType};

use super::error::NtpError;
use super::handshake::EstablishedSession;

// ---------------------------------------------------------------------------
// SignedTransaction
// ---------------------------------------------------------------------------

/// A fully signed transaction ready for network broadcast.
///
/// This is the output of the NTP signing step. The `transaction` field
/// contains the complete payload including the Ed25519 signature. The
/// `tx_hash` is pre-computed for convenience (BLAKE3 of the transaction body).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignedTransaction {
    /// The complete, signed transaction.
    pub transaction: Transaction,
    /// Pre-computed transaction hash (hex-encoded for logging/display).
    pub tx_hash: String,
    /// Session ID this transaction belongs to.
    pub session_id: String,
}

// ---------------------------------------------------------------------------
// BroadcastMessage
// ---------------------------------------------------------------------------

/// Network propagation wrapper around a signed transaction.
///
/// Contains the transaction itself plus metadata that validators and
/// relay nodes use for prioritization, deduplication, and TTL management.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BroadcastMessage {
    /// The signed transaction payload.
    pub signed_tx: SignedTransaction,
    /// Protocol version for this message format.
    pub protocol_version: String,
    /// Network identifier (mainnet, testnet, devnet).
    pub network_id: u32,
    /// Number of hops remaining before this message is dropped.
    /// Decremented by each relay node. Prevents infinite propagation.
    pub ttl: u8,
    /// Unix timestamp (milliseconds) when this message was created.
    pub broadcast_timestamp: u64,
    /// Sender's self-declared priority hint (0 = normal, 1 = high).
    /// Validators are free to ignore this — actual priority is
    /// determined by `fee_per_byte`.
    pub priority: u8,
}

// ---------------------------------------------------------------------------
// Transaction Construction
// ---------------------------------------------------------------------------

/// Prepare an unsigned transaction from session parameters.
///
/// Builds the transaction body from the payment parameters agreed
/// during the handshake. The transaction is unsigned — call
/// [`sign_and_prepare`] to attach the signature.
///
/// # Arguments
///
/// * `session` — The established NTP session.
/// * `sender_address` — The sender's NOVA address.
/// * `nonce` — The sender's current account nonce.
///
/// # Returns
///
/// An unsigned `Transaction` with all fields populated except `signature`.
pub fn prepare_transaction(
    session: &EstablishedSession,
    sender_address: &str,
    nonce: u64,
) -> Result<Transaction, NtpError> {
    let params = &session.payment_params;

    let tx = TransactionBuilder::new(TransactionType::Transfer)
        .sender(sender_address)
        .receiver(&session.peer_nova_id)
        .amount(Amount::new(params.amount, params.currency.clone()))
        .fee(config::MIN_TX_FEE_PHOTONS)
        .nonce(nonce)
        .payload(format!("NTP:{} | {}", session.session_id, params.description).into_bytes())
        .build();

    Ok(tx)
}

/// Sign a transaction and wrap it as a [`SignedTransaction`].
///
/// Attaches the sender's Ed25519 signature to the transaction body,
/// then packages it with the session ID and pre-computed hash.
///
/// # Arguments
///
/// * `transaction` — The unsigned transaction from [`prepare_transaction`].
/// * `keypair` — The sender's signing keypair.
/// * `session_id` — The NTP session ID for cross-referencing.
pub fn sign_and_prepare(
    mut transaction: Transaction,
    keypair: &NovaKeypair,
    session_id: &str,
) -> SignedTransaction {
    sign_transaction(&mut transaction, keypair);
    let tx_hash = transaction.id_hex();

    SignedTransaction {
        transaction,
        tx_hash,
        session_id: session_id.to_string(),
    }
}

/// Create a broadcast-ready message from a signed transaction.
///
/// Wraps the signed transaction with network metadata for gossip
/// propagation. The default TTL is 8 hops (matching `GOSSIP_FANOUT`).
///
/// # Arguments
///
/// * `signed_tx` — The signed transaction.
/// * `network_id` — The network this transaction targets (mainnet/testnet/devnet).
pub fn create_broadcast_message(signed_tx: SignedTransaction, network_id: u32) -> BroadcastMessage {
    let broadcast_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    BroadcastMessage {
        signed_tx,
        protocol_version: config::PROTOCOL_VERSION.to_string(),
        network_id,
        ttl: config::GOSSIP_FANOUT as u8,
        broadcast_timestamp,
        priority: 0,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::NovaKeypair;
    use crate::ntp::handshake::{HandshakeSession, PaymentParams};
    use crate::transaction::types::Currency;

    fn setup_session_and_keypair() -> (EstablishedSession, NovaKeypair) {
        let sender_kp = NovaKeypair::generate();
        let receiver_kp = NovaKeypair::generate();

        let payment = PaymentParams {
            amount: 2500,
            currency: Currency::BRL,
            description: "Test payment".to_string(),
        };

        let (sender_session, request) = HandshakeSession::initiate(&sender_kp, vec![Currency::BRL]);
        let (response, _receiver) =
            HandshakeSession::respond(&request, &receiver_kp, payment).unwrap();
        let established = sender_session.complete(&response).unwrap();
        (established, sender_kp)
    }

    #[test]
    fn prepare_transaction_builds_correctly() {
        let (session, _kp) = setup_session_and_keypair();

        let tx = prepare_transaction(&session, "nova:sender123", 0)
            .expect("transaction preparation should succeed");

        assert_eq!(tx.amount.value, 2500);
        assert_eq!(tx.amount.currency, Currency::BRL);
        assert_eq!(tx.tx_type, TransactionType::Transfer);
        assert_eq!(tx.sender, "nova:sender123");
        assert_eq!(tx.receiver, session.peer_nova_id);
        assert_eq!(tx.nonce, 0);
        assert!(!tx.is_signed(), "unsigned tx should have no signature");
    }

    #[test]
    fn sign_and_prepare_attaches_signature() {
        let (session, kp) = setup_session_and_keypair();

        let tx = prepare_transaction(&session, &session.our_nova_id, 0).unwrap();
        let signed = sign_and_prepare(tx, &kp, &session.session_id);

        assert!(signed.transaction.is_signed());
        assert_eq!(signed.session_id, session.session_id);
        assert!(!signed.tx_hash.is_empty());
    }

    #[test]
    fn broadcast_message_structure() {
        let (session, kp) = setup_session_and_keypair();

        let tx = prepare_transaction(&session, &session.our_nova_id, 0).unwrap();
        let signed = sign_and_prepare(tx, &kp, &session.session_id);
        let broadcast = create_broadcast_message(signed, config::NETWORK_ID_TESTNET);

        assert_eq!(broadcast.network_id, config::NETWORK_ID_TESTNET);
        assert_eq!(broadcast.protocol_version, config::PROTOCOL_VERSION);
        assert_eq!(broadcast.ttl, config::GOSSIP_FANOUT as u8);
        assert_eq!(broadcast.priority, 0);
        assert!(broadcast.broadcast_timestamp > 0);
    }

    #[test]
    fn broadcast_message_serialization_roundtrip() {
        let (session, kp) = setup_session_and_keypair();

        let tx = prepare_transaction(&session, &session.our_nova_id, 0).unwrap();
        let signed = sign_and_prepare(tx, &kp, &session.session_id);
        let broadcast = create_broadcast_message(signed, config::NETWORK_ID_DEVNET);

        let json = serde_json::to_string(&broadcast).expect("serialize");
        let recovered: BroadcastMessage = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(recovered.network_id, config::NETWORK_ID_DEVNET);
        assert_eq!(
            recovered.signed_tx.session_id,
            broadcast.signed_tx.session_id
        );
    }
}
