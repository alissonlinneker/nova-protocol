//! End-to-end integration tests for the NOVA Protocol.
//!
//! These tests exercise the full transaction lifecycle from identity creation
//! through block finalization. They prove that the protocol's core components
//! compose correctly: keypair generation, NovaId derivation, transaction
//! construction, signing, verification, mempool management, block production,
//! state tree updates, and database persistence.
//!
//! Each test stands alone with its own temporary database and state tree.
//! No shared state, no test ordering dependencies, no flaky failures.

use std::sync::Arc;

use parking_lot::RwLock;

use nova_protocol::crypto::keys::NovaKeypair;
use nova_protocol::identity::NovaId;
use nova_protocol::network::mempool::{Mempool, MempoolConfig};
use nova_protocol::network::producer::BlockProducer;
use nova_protocol::storage::block::Block;
use nova_protocol::storage::db::NovaDB;
use nova_protocol::storage::state::{AccountState, StateTree};
use nova_protocol::transaction::builder::TransactionBuilder;
use nova_protocol::transaction::signing::sign_transaction;
use nova_protocol::transaction::types::{Amount, Currency, TransactionType};
use nova_protocol::transaction::verification::verify_transaction;

// ---------------------------------------------------------------------------
// Test Helpers
// ---------------------------------------------------------------------------

/// Spins up the full block production stack with temporary storage.
/// Returns all the shared components so tests can inspect them directly.
#[allow(clippy::type_complexity)]
fn setup() -> (
    BlockProducer,
    Block,
    Arc<RwLock<StateTree>>,
    Arc<Mempool>,
    Arc<NovaDB>,
    NovaKeypair,
) {
    let db = Arc::new(NovaDB::open_temporary().expect("temp db"));
    let state_tree = Arc::new(RwLock::new(StateTree::new((*db).clone())));
    let mempool = Arc::new(Mempool::new(MempoolConfig::default()));
    let keypair = NovaKeypair::generate();
    let producer = BlockProducer::new(
        Arc::clone(&db),
        Arc::clone(&state_tree),
        Arc::clone(&mempool),
        keypair.clone(),
    );
    let genesis = Block::genesis();
    (producer, genesis, state_tree, mempool, db, keypair)
}

/// Seeds an account with a given balance in the state tree.
fn seed_balance(tree: &Arc<RwLock<StateTree>>, address: &str, balance: u64) {
    let mut t = tree.write();
    t.put(address, &AccountState::with_balance(balance));
}

/// Builds a signed transfer transaction between two NOVA addresses.
fn build_signed_transfer(
    sender_kp: &NovaKeypair,
    sender_addr: &str,
    receiver_addr: &str,
    amount: u64,
    fee: u64,
    nonce: u64,
) -> nova_protocol::transaction::Transaction {
    let mut tx = TransactionBuilder::new(TransactionType::Transfer)
        .sender(sender_addr)
        .receiver(receiver_addr)
        .amount(Amount::new(amount, Currency::NOVA))
        .fee(fee)
        .nonce(nonce)
        .build();
    sign_transaction(&mut tx, sender_kp);
    tx
}

// ---------------------------------------------------------------------------
// 1. Full Transfer Lifecycle
// ---------------------------------------------------------------------------

#[test]
fn full_transfer_lifecycle() {
    let (producer, genesis, tree, mempool, db, _validator_kp) = setup();

    // Create two identities.
    let alice_kp = NovaKeypair::generate();
    let bob_kp = NovaKeypair::generate();
    let alice_id = NovaId::from_public_key(&alice_kp.public_key());
    let bob_id = NovaId::from_public_key(&bob_kp.public_key());
    let alice_addr = alice_id.to_address();
    let bob_addr = bob_id.to_address();

    assert!(alice_addr.starts_with("nova1"));
    assert!(bob_addr.starts_with("nova1"));
    assert_ne!(alice_addr, bob_addr);

    // Fund Alice.
    seed_balance(&tree, &alice_addr, 10_000);

    // Build, sign, and verify a transfer transaction.
    let tx = build_signed_transfer(&alice_kp, &alice_addr, &bob_addr, 500, 100, 1);
    assert!(verify_transaction(&tx).is_ok());

    // Add to mempool and produce a block.
    mempool.add(tx.clone()).unwrap();
    db.put_block(&genesis).unwrap();

    let produced = producer.produce_block(&genesis, 100).unwrap();
    assert_eq!(produced.block.transactions.len(), 1);
    producer.commit_block(&produced.block).unwrap();

    // Verify balances updated correctly.
    let t = tree.read();
    let alice_state = t.get(&alice_addr).unwrap();
    let bob_state = t.get(&bob_addr).unwrap();
    assert_eq!(alice_state.balance, 9_500);
    assert_eq!(bob_state.balance, 500);

    // Verify block is persisted in the database.
    let stored_block = db.get_block(1).unwrap().expect("block 1 should exist");
    assert_eq!(stored_block.header.height, 1);
    assert_eq!(stored_block.transactions.len(), 1);

    // Verify transaction is retrievable from the database.
    let stored_tx = db.get_transaction(&tx.id).unwrap();
    assert!(stored_tx.is_some());
    assert_eq!(stored_tx.unwrap().id, tx.id);
}

// ---------------------------------------------------------------------------
// 2. Multiple Transfers in a Single Block
// ---------------------------------------------------------------------------

#[test]
fn multiple_transfers_single_block() {
    let (producer, genesis, tree, mempool, db, _) = setup();

    let alice_kp = NovaKeypair::generate();
    let bob_kp = NovaKeypair::generate();
    let alice_addr = NovaId::from_public_key(&alice_kp.public_key()).to_address();
    let bob_addr = NovaId::from_public_key(&bob_kp.public_key()).to_address();

    seed_balance(&tree, &alice_addr, 10_000);
    db.put_block(&genesis).unwrap();

    // Create 5 transfers of 100 each. The block producer uses fee priority,
    // so we give them decreasing fees to test ordering, but use distinct
    // nonces to avoid collisions.
    for i in 1..=5u64 {
        let tx = build_signed_transfer(&alice_kp, &alice_addr, &bob_addr, 100, 100 + i, i);
        mempool.add(tx).unwrap();
    }

    let produced = producer.produce_block(&genesis, 100).unwrap();
    let success_count = produced.tx_results.iter().filter(|r| r.success).count();
    assert_eq!(success_count, 5);
    assert_eq!(produced.block.transactions.len(), 5);

    producer.commit_block(&produced.block).unwrap();

    let t = tree.read();
    let alice = t.get(&alice_addr).unwrap();
    let bob = t.get(&bob_addr).unwrap();
    assert_eq!(alice.balance, 9_500);
    assert_eq!(bob.balance, 500);
}

// ---------------------------------------------------------------------------
// 3. Chain of Blocks
// ---------------------------------------------------------------------------

#[test]
fn chain_of_blocks() {
    let (producer, genesis, tree, mempool, db, _) = setup();

    let alice_kp = NovaKeypair::generate();
    let bob_kp = NovaKeypair::generate();
    let charlie_kp = NovaKeypair::generate();
    let alice_addr = NovaId::from_public_key(&alice_kp.public_key()).to_address();
    let bob_addr = NovaId::from_public_key(&bob_kp.public_key()).to_address();
    let charlie_addr = NovaId::from_public_key(&charlie_kp.public_key()).to_address();

    seed_balance(&tree, &alice_addr, 10_000);
    db.put_block(&genesis).unwrap();

    let mut parent = genesis;

    // Block 1: Alice -> Bob 1000
    let tx1 = build_signed_transfer(&alice_kp, &alice_addr, &bob_addr, 1_000, 100, 1);
    mempool.add(tx1).unwrap();
    let p1 = producer.produce_block(&parent, 100).unwrap();
    producer.commit_block(&p1.block).unwrap();
    assert_eq!(p1.block.header.height, 1);
    parent = p1.block;

    // Block 2: Bob -> Charlie 500
    let tx2 = build_signed_transfer(&bob_kp, &bob_addr, &charlie_addr, 500, 100, 1);
    mempool.add(tx2).unwrap();
    let p2 = producer.produce_block(&parent, 100).unwrap();
    producer.commit_block(&p2.block).unwrap();
    assert_eq!(p2.block.header.height, 2);
    parent = p2.block;

    // Block 3: Charlie -> Alice 200
    let tx3 = build_signed_transfer(&charlie_kp, &charlie_addr, &alice_addr, 200, 100, 1);
    mempool.add(tx3).unwrap();
    let p3 = producer.produce_block(&parent, 100).unwrap();
    producer.commit_block(&p3.block).unwrap();
    assert_eq!(p3.block.header.height, 3);

    // Verify final balances.
    let t = tree.read();
    let alice = t.get(&alice_addr).unwrap();
    let bob = t.get(&bob_addr).unwrap();
    let charlie = t.get(&charlie_addr).unwrap();
    assert_eq!(alice.balance, 9_200); // 10000 - 1000 + 200
    assert_eq!(bob.balance, 500); // 1000 - 500
    assert_eq!(charlie.balance, 300); // 500 - 200

    // Verify we have 4 blocks (genesis + 3).
    let chain = db.get_block_range(0, 3).unwrap();
    assert_eq!(chain.len(), 4);

    // Verify each block links to its parent.
    for i in 1..chain.len() {
        assert_eq!(chain[i].header.parent_hash, chain[i - 1].header.hash);
    }
}

// ---------------------------------------------------------------------------
// 4. Insufficient Balance Rejected
// ---------------------------------------------------------------------------

#[test]
fn insufficient_balance_rejected() {
    let (producer, genesis, tree, mempool, db, _) = setup();

    let alice_kp = NovaKeypair::generate();
    let bob_kp = NovaKeypair::generate();
    let alice_addr = NovaId::from_public_key(&alice_kp.public_key()).to_address();
    let bob_addr = NovaId::from_public_key(&bob_kp.public_key()).to_address();

    // Alice only has 100.
    seed_balance(&tree, &alice_addr, 100);
    db.put_block(&genesis).unwrap();

    // Try to transfer 200 — should be dropped during block production.
    let tx = build_signed_transfer(&alice_kp, &alice_addr, &bob_addr, 200, 50, 1);
    mempool.add(tx).unwrap();

    let produced = producer.produce_block(&genesis, 100).unwrap();

    // The transaction should have been dropped (insufficient balance).
    assert_eq!(produced.block.transactions.len(), 0);
    let failed = produced.tx_results.iter().find(|r| !r.success);
    assert!(failed.is_some(), "should have one failed tx result");

    // Alice balance unchanged.
    let t = tree.read();
    let alice = t.get(&alice_addr).unwrap();
    assert_eq!(alice.balance, 100);
}

// ---------------------------------------------------------------------------
// 5. Nonce Enforcement
// ---------------------------------------------------------------------------

#[test]
fn nonce_enforcement() {
    let (producer, genesis, tree, mempool, db, _) = setup();

    let alice_kp = NovaKeypair::generate();
    let bob_kp = NovaKeypair::generate();
    let alice_addr = NovaId::from_public_key(&alice_kp.public_key()).to_address();
    let bob_addr = NovaId::from_public_key(&bob_kp.public_key()).to_address();

    seed_balance(&tree, &alice_addr, 10_000);
    db.put_block(&genesis).unwrap();

    // Block 1: nonce 1 transaction should succeed.
    let tx1 = build_signed_transfer(&alice_kp, &alice_addr, &bob_addr, 100, 100, 1);
    mempool.add(tx1).unwrap();
    let p1 = producer.produce_block(&genesis, 100).unwrap();
    assert_eq!(p1.block.transactions.len(), 1);
    producer.commit_block(&p1.block).unwrap();

    // Verify nonce was incremented in state tree.
    {
        let t = tree.read();
        let alice = t.get(&alice_addr).unwrap();
        assert_eq!(alice.nonce, 1);
    }

    // Block 2: nonce 2 transaction should succeed (sequential).
    let tx2 = build_signed_transfer(&alice_kp, &alice_addr, &bob_addr, 100, 100, 2);
    mempool.add(tx2).unwrap();
    let p2 = producer.produce_block(&p1.block, 100).unwrap();
    assert_eq!(p2.block.transactions.len(), 1);
    producer.commit_block(&p2.block).unwrap();

    {
        let t = tree.read();
        let alice = t.get(&alice_addr).unwrap();
        assert_eq!(alice.nonce, 2);
    }
}

// ---------------------------------------------------------------------------
// 6. Identity to Address to State
// ---------------------------------------------------------------------------

#[test]
fn identity_to_address_to_state() {
    let db = NovaDB::open_temporary().expect("temp db");
    let mut tree = StateTree::new(db);

    // Create a NovaId from a fresh keypair.
    let kp = NovaKeypair::generate();
    let id = NovaId::from_public_key(&kp.public_key());
    let address = id.to_address();

    // The address must start with "nova1" (bech32 with NOVA HRP).
    assert!(address.starts_with("nova1"));

    // Fund the address via state tree.
    let account = AccountState::with_balance(42_000);
    tree.put(&address, &account);

    // Retrieve and verify.
    let retrieved = tree.get(&address).expect("account should exist");
    assert_eq!(retrieved.balance, 42_000);
    assert_eq!(retrieved.nonce, 0);
    assert!(!retrieved.frozen);

    // Verify the NovaId roundtrip through address encoding.
    let recovered_id = NovaId::from_address(&address).unwrap();
    assert_eq!(id, recovered_id);
}

// ---------------------------------------------------------------------------
// 7. Block Production Updates State Root
// ---------------------------------------------------------------------------

#[test]
fn block_production_updates_state_root() {
    let (producer, genesis, tree, mempool, db, _) = setup();

    let alice_kp = NovaKeypair::generate();
    let bob_kp = NovaKeypair::generate();
    let alice_addr = NovaId::from_public_key(&alice_kp.public_key()).to_address();
    let bob_addr = NovaId::from_public_key(&bob_kp.public_key()).to_address();

    seed_balance(&tree, &alice_addr, 10_000);
    db.put_block(&genesis).unwrap();

    let root_before = tree.read().root();

    let tx = build_signed_transfer(&alice_kp, &alice_addr, &bob_addr, 5_000, 100, 1);
    mempool.add(tx).unwrap();

    let produced = producer.produce_block(&genesis, 100).unwrap();

    // The state root must change after applying transfers.
    assert_ne!(produced.state_root, root_before);

    // The block header must embed the new state root.
    assert_eq!(produced.block.header.state_root, produced.state_root);
}

// ---------------------------------------------------------------------------
// 8. Verify Signed Transaction Roundtrip
// ---------------------------------------------------------------------------

#[test]
fn verify_signed_transaction_roundtrip() {
    let alice_kp = NovaKeypair::generate();
    let bob_kp = NovaKeypair::generate();
    let alice_addr = NovaId::from_public_key(&alice_kp.public_key()).to_address();
    let bob_addr = NovaId::from_public_key(&bob_kp.public_key()).to_address();

    let tx = build_signed_transfer(&alice_kp, &alice_addr, &bob_addr, 1_000, 100, 1);

    // Valid transaction should pass verification.
    assert!(verify_transaction(&tx).is_ok());

    // Tamper with the amount — verification should fail because the
    // transaction ID no longer matches the recomputed hash.
    let mut tampered = tx.clone();
    tampered.amount.value = 9_999;
    // The ID was computed from the original amount, so now there's a mismatch.
    assert!(verify_transaction(&tampered).is_err());
}

// ---------------------------------------------------------------------------
// 9. Block Merkle Root Integrity
// ---------------------------------------------------------------------------

#[test]
fn block_merkle_root_integrity() {
    let (producer, genesis, tree, mempool, db, _) = setup();

    let alice_kp = NovaKeypair::generate();
    let bob_kp = NovaKeypair::generate();
    let alice_addr = NovaId::from_public_key(&alice_kp.public_key()).to_address();
    let bob_addr = NovaId::from_public_key(&bob_kp.public_key()).to_address();

    seed_balance(&tree, &alice_addr, 10_000);
    db.put_block(&genesis).unwrap();

    let tx = build_signed_transfer(&alice_kp, &alice_addr, &bob_addr, 500, 100, 1);
    mempool.add(tx).unwrap();

    let produced = producer.produce_block(&genesis, 100).unwrap();

    // The produced block should pass structural verification.
    assert!(produced.block.verify().is_ok());

    // Tamper with a transaction in the block — verification should fail.
    let mut tampered_block = produced.block.clone();
    if !tampered_block.transactions.is_empty() {
        tampered_block.transactions[0].amount.value = 99_999;
        // Recompute the hash to avoid the hash mismatch check, but the
        // tx_root will no longer match the actual transactions.
        tampered_block.header.hash = tampered_block.compute_hash();
        assert!(tampered_block.verify().is_err());
    }
}

// ---------------------------------------------------------------------------
// 10. Concurrent Mempool and Production
// ---------------------------------------------------------------------------

#[test]
fn concurrent_mempool_and_production() {
    use std::thread;

    let (producer, genesis, tree, mempool, db, _) = setup();
    let producer = Arc::new(producer);
    let genesis = Arc::new(genesis);

    seed_balance(&tree, "nova1alice_concurrent", 1_000_000);
    db.put_block(&genesis).unwrap();

    // Pre-populate some transactions.
    for i in 0..5u64 {
        let tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1alice_concurrent")
            .receiver("nova1bob_concurrent")
            .amount(Amount::new(10, Currency::NOVA))
            .fee((i + 1) * 100)
            .nonce(i)
            .build();
        mempool.add(tx).unwrap();
    }

    // Spawn a writer thread that adds more transactions concurrently.
    let mempool_clone = Arc::clone(&mempool);
    let writer = thread::spawn(move || {
        for i in 100..120u64 {
            let tx = TransactionBuilder::new(TransactionType::Transfer)
                .sender(&format!("nova1writer_{}", i))
                .receiver("nova1receiver_concurrent")
                .amount(Amount::new(1, Currency::NOVA))
                .fee(50)
                .nonce(i)
                .build();
            let _ = mempool_clone.add(tx);
        }
    });

    // Produce a block concurrently.
    let produced = producer.produce_block(&genesis, 100);
    writer.join().expect("writer thread should not panic");

    // Block production should succeed without panics.
    assert!(produced.is_ok());
    let block = produced.unwrap().block;
    assert!(block.verify().is_ok());
}

// ---------------------------------------------------------------------------
// 11. Genesis Block in DB
// ---------------------------------------------------------------------------

#[test]
fn genesis_block_in_db() {
    let db = NovaDB::open_temporary().expect("temp db");
    let genesis = Block::genesis();

    // Persist and retrieve the genesis block.
    db.put_block(&genesis).unwrap();

    let retrieved = db.get_block(0).unwrap().expect("genesis should exist");
    assert_eq!(retrieved.header.height, 0);
    assert_eq!(retrieved.header.parent_hash, [0u8; 32]);
    assert!(retrieved.transactions.is_empty());
    assert!(retrieved.header.signature.is_empty());
    assert_eq!(retrieved.header.hash, genesis.header.hash);

    // Genesis should pass structural verification.
    assert!(retrieved.verify().is_ok());

    // The latest height should be 0.
    assert_eq!(db.get_latest_block_height().unwrap(), Some(0));
}

// ---------------------------------------------------------------------------
// 12. State Tree Merkle Proof After Transfer
// ---------------------------------------------------------------------------

#[test]
fn state_tree_merkle_proof_after_transfer() {
    let (producer, genesis, tree, mempool, db, _) = setup();

    let alice_kp = NovaKeypair::generate();
    let bob_kp = NovaKeypair::generate();
    let alice_addr = NovaId::from_public_key(&alice_kp.public_key()).to_address();
    let bob_addr = NovaId::from_public_key(&bob_kp.public_key()).to_address();

    seed_balance(&tree, &alice_addr, 10_000);
    db.put_block(&genesis).unwrap();

    // Execute a transfer via block production.
    let tx = build_signed_transfer(&alice_kp, &alice_addr, &bob_addr, 3_000, 100, 1);
    mempool.add(tx).unwrap();

    let produced = producer.produce_block(&genesis, 100).unwrap();
    producer.commit_block(&produced.block).unwrap();

    // Get the current state and generate a Merkle proof.
    let t = tree.read();
    let alice_state = t.get(&alice_addr).expect("alice should exist");
    assert_eq!(alice_state.balance, 7_000);

    let proof = t.get_proof(&alice_addr);
    assert_eq!(proof.siblings.len(), 256);
    assert_eq!(proof.path_bits.len(), 256);

    // Verify the inclusion proof against the current state root.
    let root = t.root();
    let valid = StateTree::verify_proof(&root, &alice_addr, Some(&alice_state), &proof);
    assert!(
        valid,
        "inclusion proof for sender should verify after transfer"
    );

    // Verify an incorrect value does not pass.
    let wrong_state = AccountState::with_balance(99_999);
    let invalid = StateTree::verify_proof(&root, &alice_addr, Some(&wrong_state), &proof);
    assert!(!invalid, "proof with wrong value should not verify");
}

// ---------------------------------------------------------------------------
// 13. Multiple Currencies (NOVA + USD)
// ---------------------------------------------------------------------------

#[test]
fn multiple_currencies_if_supported() {
    // Currency enum supports BRL, USD, EUR, BTC, ETH, USDC, NOVA, Custom.
    // Verify that the transaction builder and types handle different currencies
    // correctly, even though the state tree only tracks native NOVA balances.

    let alice_kp = NovaKeypair::generate();
    let bob_kp = NovaKeypair::generate();
    let alice_addr = NovaId::from_public_key(&alice_kp.public_key()).to_address();
    let bob_addr = NovaId::from_public_key(&bob_kp.public_key()).to_address();

    // NOVA transfer.
    let tx_nova = build_signed_transfer(&alice_kp, &alice_addr, &bob_addr, 1_000, 100, 1);
    assert!(verify_transaction(&tx_nova).is_ok());
    assert_eq!(tx_nova.amount.currency, Currency::NOVA);

    // USD transfer (different currency in the Amount).
    let mut tx_usd = TransactionBuilder::new(TransactionType::Transfer)
        .sender(&alice_addr)
        .receiver(&bob_addr)
        .amount(Amount::new(5_000, Currency::USD))
        .fee(100)
        .nonce(2)
        .build();
    sign_transaction(&mut tx_usd, &alice_kp);
    assert!(verify_transaction(&tx_usd).is_ok());
    assert_eq!(tx_usd.amount.currency, Currency::USD);

    // Custom token transfer.
    let mut tx_custom = TransactionBuilder::new(TransactionType::Transfer)
        .sender(&alice_addr)
        .receiver(&bob_addr)
        .amount(Amount::new(100, Currency::Custom("DOGE".to_string())))
        .fee(100)
        .nonce(3)
        .build();
    sign_transaction(&mut tx_custom, &alice_kp);
    assert!(verify_transaction(&tx_custom).is_ok());
    assert_eq!(
        tx_custom.amount.currency,
        Currency::Custom("DOGE".to_string())
    );

    // All three transaction IDs should be unique.
    assert_ne!(tx_nova.id, tx_usd.id);
    assert_ne!(tx_usd.id, tx_custom.id);
    assert_ne!(tx_nova.id, tx_custom.id);
}

// ---------------------------------------------------------------------------
// 14. Large Block Stress Test
// ---------------------------------------------------------------------------

#[test]
fn large_block_stress_test() {
    let (producer, genesis, tree, mempool, db, _) = setup();

    // Create 100 funded accounts and build transfers from each.
    let mut keypairs = Vec::with_capacity(100);
    let mut addresses = Vec::with_capacity(100);
    let receiver_kp = NovaKeypair::generate();
    let receiver_addr = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

    for _ in 0..100 {
        let kp = NovaKeypair::generate();
        let addr = NovaId::from_public_key(&kp.public_key()).to_address();
        seed_balance(&tree, &addr, 10_000);
        addresses.push(addr);
        keypairs.push(kp);
    }

    db.put_block(&genesis).unwrap();

    // Each account sends 100 NOVA to the receiver.
    for i in 0..100 {
        let tx = build_signed_transfer(&keypairs[i], &addresses[i], &receiver_addr, 100, 50, 1);
        mempool.add(tx).unwrap();
    }

    let produced = producer.produce_block(&genesis, 200).unwrap();
    let successful = produced.tx_results.iter().filter(|r| r.success).count();
    assert_eq!(successful, 100);
    assert_eq!(produced.block.transactions.len(), 100);

    producer.commit_block(&produced.block).unwrap();

    // Verify receiver got all 100 transfers.
    let t = tree.read();
    let receiver_state = t.get(&receiver_addr).unwrap();
    assert_eq!(receiver_state.balance, 10_000); // 100 * 100

    // Verify each sender was debited.
    for addr in &addresses {
        let state = t.get(addr).unwrap();
        assert_eq!(state.balance, 9_900); // 10_000 - 100
    }
}

// ---------------------------------------------------------------------------
// 15. DB Persistence Survives Reopen
// ---------------------------------------------------------------------------

#[test]
fn db_persistence_survives_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");

    let genesis = Block::genesis();
    let account = AccountState::with_balance(7_777);

    // First session: write data.
    {
        let db = NovaDB::open(dir.path()).expect("open db");
        db.put_block(&genesis).unwrap();

        let block1 = Block::new(
            &genesis,
            vec![],
            "nova1validator_test".to_string(),
            [42u8; 32],
        );
        db.put_block(&block1).unwrap();
        db.put_account("nova1test_user", &account).unwrap();
        db.flush().unwrap();
    }
    // db is dropped here.

    // Second session: reopen and verify data survived.
    {
        let db = NovaDB::open(dir.path()).expect("reopen db");

        let retrieved_genesis = db
            .get_block(0)
            .unwrap()
            .expect("genesis should survive reopen");
        assert_eq!(retrieved_genesis.header.height, 0);
        assert_eq!(retrieved_genesis.header.hash, genesis.header.hash);

        let retrieved_block1 = db
            .get_block(1)
            .unwrap()
            .expect("block 1 should survive reopen");
        assert_eq!(retrieved_block1.header.height, 1);
        assert_eq!(retrieved_block1.header.state_root, [42u8; 32]);

        let retrieved_account = db
            .get_account("nova1test_user")
            .unwrap()
            .expect("account should survive reopen");
        assert_eq!(retrieved_account.balance, 7_777);

        assert_eq!(db.get_latest_block_height().unwrap(), Some(1));
    }
}

// ---------------------------------------------------------------------------
// 16. Transaction Types Beyond Transfer
// ---------------------------------------------------------------------------

#[test]
fn non_transfer_transaction_types_accepted() {
    // CreditRequest, CreditSettlement, TokenMint, TokenBurn are accepted
    // by the block producer as no-ops (no state change, but included in block).
    let (producer, genesis, tree, mempool, db, _) = setup();
    db.put_block(&genesis).unwrap();

    seed_balance(&tree, "nova1credit_sender", 50_000);

    let tx = TransactionBuilder::new(TransactionType::CreditRequest)
        .sender("nova1credit_sender")
        .receiver("nova1credit_receiver")
        .amount(Amount::new(1_000, Currency::NOVA))
        .fee(100)
        .nonce(0)
        .build();
    mempool.add(tx).unwrap();

    let produced = producer.produce_block(&genesis, 100).unwrap();
    assert_eq!(produced.block.transactions.len(), 1);
    assert!(produced.tx_results.iter().all(|r| r.success));
}

// ---------------------------------------------------------------------------
// 17. Block Hash Determinism
// ---------------------------------------------------------------------------

#[test]
fn genesis_block_hash_deterministic() {
    let g1 = Block::genesis();
    let g2 = Block::genesis();
    assert_eq!(g1.header.hash, g2.header.hash);
    assert_eq!(g1.compute_hash(), g2.compute_hash());
}

// ---------------------------------------------------------------------------
// 18. State Tree Root Deterministic Across Independent Trees
// ---------------------------------------------------------------------------

#[test]
fn state_root_deterministic_across_independent_trees() {
    let db1 = NovaDB::open_temporary().expect("temp db 1");
    let db2 = NovaDB::open_temporary().expect("temp db 2");

    let mut tree1 = StateTree::new(db1);
    let mut tree2 = StateTree::new(db2);

    // Insert accounts in different order — the root should be the same
    // because the SMT produces order-independent roots.
    tree1.put("nova1alice", &AccountState::with_balance(1_000));
    tree1.put("nova1bob", &AccountState::with_balance(2_000));
    tree1.put("nova1charlie", &AccountState::with_balance(3_000));

    tree2.put("nova1charlie", &AccountState::with_balance(3_000));
    tree2.put("nova1alice", &AccountState::with_balance(1_000));
    tree2.put("nova1bob", &AccountState::with_balance(2_000));

    assert_eq!(tree1.root(), tree2.root());
}

// ---------------------------------------------------------------------------
// 19. Frozen Account Transfer Rejected End-to-End
// ---------------------------------------------------------------------------

#[test]
fn frozen_account_transfer_rejected_e2e() {
    let (producer, genesis, tree, mempool, db, _) = setup();
    db.put_block(&genesis).unwrap();

    let alice_kp = NovaKeypair::generate();
    let bob_kp = NovaKeypair::generate();
    let alice_addr = NovaId::from_public_key(&alice_kp.public_key()).to_address();
    let bob_addr = NovaId::from_public_key(&bob_kp.public_key()).to_address();

    // Freeze Alice's account.
    {
        let mut t = tree.write();
        t.put(
            &alice_addr,
            &AccountState {
                balance: 10_000,
                frozen: true,
                ..Default::default()
            },
        );
    }

    let tx = build_signed_transfer(&alice_kp, &alice_addr, &bob_addr, 1_000, 100, 1);
    mempool.add(tx).unwrap();

    let produced = producer.produce_block(&genesis, 100).unwrap();

    // Frozen account transfer should be dropped.
    assert_eq!(produced.block.transactions.len(), 0);
    assert!(produced.tx_results.iter().any(|r| !r.success));

    // Alice's balance should be unchanged.
    let t = tree.read();
    let alice = t.get(&alice_addr).unwrap();
    assert_eq!(alice.balance, 10_000);
    assert!(alice.frozen);
}

// ---------------------------------------------------------------------------
// 20. Full Pipeline: Identity -> Transaction -> Block -> DB -> State Proof
// ---------------------------------------------------------------------------

#[test]
fn full_pipeline_identity_through_state_proof() {
    // This test exercises the complete path through every layer of the protocol:
    //   1. Generate keypairs and derive NOVA IDs
    //   2. Build and sign a transaction
    //   3. Verify the transaction cryptographically
    //   4. Add to mempool
    //   5. Produce and commit a block
    //   6. Verify the block is in the database
    //   7. Generate and verify a Merkle proof for the updated state

    let (producer, genesis, tree, mempool, db, _) = setup();
    db.put_block(&genesis).unwrap();

    let alice_kp = NovaKeypair::generate();
    let bob_kp = NovaKeypair::generate();
    let alice_id = NovaId::from_public_key(&alice_kp.public_key());
    let bob_id = NovaId::from_public_key(&bob_kp.public_key());
    let alice_addr = alice_id.to_address();
    let bob_addr = bob_id.to_address();

    // Step 1: Verify identity properties.
    assert!(alice_addr.starts_with("nova1"));
    assert_ne!(alice_addr, bob_addr);
    let alice_recovered = NovaId::from_address(&alice_addr).unwrap();
    assert_eq!(alice_id, alice_recovered);

    // Step 2: Fund, build, sign, verify.
    seed_balance(&tree, &alice_addr, 50_000);

    let tx = build_signed_transfer(&alice_kp, &alice_addr, &bob_addr, 15_000, 200, 1);
    assert!(tx.is_signed());
    assert!(verify_transaction(&tx).is_ok());

    // Step 3: Mempool and block production.
    mempool.add(tx.clone()).unwrap();
    let produced = producer.produce_block(&genesis, 100).unwrap();
    assert_eq!(produced.block.transactions.len(), 1);
    assert!(produced.block.verify().is_ok());
    producer.commit_block(&produced.block).unwrap();

    // Step 4: Database verification.
    let db_block = db.get_block(1).unwrap().expect("block 1 in db");
    assert_eq!(db_block.header.height, 1);
    let db_tx = db.get_transaction(&tx.id).unwrap().expect("tx in db");
    assert_eq!(db_tx.id, tx.id);

    // Step 5: State tree verification with Merkle proof.
    let t = tree.read();
    let alice_state = t.get(&alice_addr).unwrap();
    assert_eq!(alice_state.balance, 35_000);

    let bob_state = t.get(&bob_addr).unwrap();
    assert_eq!(bob_state.balance, 15_000);

    let root = t.root();
    let proof = t.get_proof(&bob_addr);
    let valid = StateTree::verify_proof(&root, &bob_addr, Some(&bob_state), &proof);
    assert!(valid, "bob's inclusion proof should verify");
}
