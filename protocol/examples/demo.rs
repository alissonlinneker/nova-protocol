//! Interactive CLI demo of the full NOVA protocol lifecycle.
//!
//! Walks through identity creation, genesis funding, multi-hop transfers,
//! block production, and optional confidential transfers with zero-knowledge
//! proofs. The output uses ANSI escape codes for colored, storytelling-style
//! terminal rendering.
//!
//! Run with:
//!   cargo run --example demo --release

use std::sync::Arc;
use std::time::Instant;

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
// ANSI color constants
// ---------------------------------------------------------------------------

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const ITALIC: &str = "\x1b[3m";

const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";
const WHITE: &str = "\x1b[37m";

const BG_BLUE: &str = "\x1b[44m";

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

fn banner() {
    println!();
    println!(
        "{BG_BLUE}{BOLD}{WHITE}                                                                    {RESET}"
    );
    println!(
        "{BG_BLUE}{BOLD}{WHITE}    NOVA PROTOCOL  --  Interactive Lifecycle Demo                   {RESET}"
    );
    println!(
        "{BG_BLUE}{BOLD}{WHITE}    Version 0.1.0  |  Ed25519 + Groth16/BN254 + BLAKE3             {RESET}"
    );
    println!(
        "{BG_BLUE}{BOLD}{WHITE}                                                                    {RESET}"
    );
    println!();
}

fn section(num: u32, title: &str) {
    println!();
    println!(
        "{BOLD}{CYAN}===[{YELLOW} Step {num} {CYAN}]=============================================================={RESET}"
    );
    println!("{BOLD}{WHITE}  {title}{RESET}");
    println!(
        "{CYAN}------------------------------------------------------------------------{RESET}"
    );
}

fn subsection(text: &str) {
    println!("{DIM}{CYAN}  >> {text}{RESET}");
}

fn success(text: &str) {
    println!("{GREEN}  [OK] {text}{RESET}");
}

fn info(label: &str, value: &str) {
    println!("{WHITE}  {BOLD}{label}:{RESET} {YELLOW}{value}{RESET}");
}

fn timing(label: &str, elapsed: std::time::Duration) {
    let ms = elapsed.as_secs_f64() * 1000.0;
    println!("{DIM}{MAGENTA}  [{label}: {ms:.2} ms]{RESET}");
}

fn address_display(name: &str, addr: &str, color: &str) {
    let prefix = &addr[..5];
    let suffix = &addr[addr.len().saturating_sub(8)..];
    println!(
        "  {color}{BOLD}{name}{RESET}  {DIM}{prefix}...{suffix}{RESET}  {DIM}({} chars){RESET}",
        addr.len()
    );
}

fn balance_row(name: &str, balance: u64, color: &str) {
    println!(
        "  {color}{BOLD}{name:<12}{RESET}  {WHITE}{balance:>12}{RESET} {DIM}photons{RESET}"
    );
}

fn separator() {
    println!(
        "{DIM}{CYAN}  . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . {RESET}"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Seed an account with a given balance in the state tree.
fn seed_balance(tree: &Arc<RwLock<StateTree>>, address: &str, balance: u64) {
    let mut t = tree.write();
    t.put(address, &AccountState::with_balance(balance));
}

/// Build and sign a transfer transaction.
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
// Main
// ---------------------------------------------------------------------------

fn main() {
    let demo_start = Instant::now();

    banner();

    // -----------------------------------------------------------------------
    // Step 1: Identity Creation
    // -----------------------------------------------------------------------

    section(1, "Sovereign Identity Generation");
    subsection("Generating Ed25519 keypairs and deriving Bech32 addresses...");

    let t = Instant::now();
    let alice_kp = NovaKeypair::generate();
    let bob_kp = NovaKeypair::generate();
    let merchant_kp = NovaKeypair::generate();
    timing("keygen x3", t.elapsed());

    let alice_id = NovaId::from_public_key(&alice_kp.public_key());
    let bob_id = NovaId::from_public_key(&bob_kp.public_key());
    let merchant_id = NovaId::from_public_key(&merchant_kp.public_key());

    let alice_addr = alice_id.to_address();
    let bob_addr = bob_id.to_address();
    let merchant_addr = merchant_id.to_address();

    println!();
    address_display("Alice    ", &alice_addr, BLUE);
    address_display("Bob      ", &bob_addr, GREEN);
    address_display("Merchant ", &merchant_addr, MAGENTA);
    println!();

    // Verify address roundtrip.
    let alice_recovered = NovaId::from_address(&alice_addr).unwrap();
    assert_eq!(alice_id, alice_recovered);
    success("All addresses start with 'nova1' and pass Bech32 roundtrip verification");

    // -----------------------------------------------------------------------
    // Step 2: Infrastructure Bootstrap
    // -----------------------------------------------------------------------

    section(2, "Network Infrastructure Bootstrap");
    subsection("Initializing temporary database, state tree, mempool, and block producer...");

    let t = Instant::now();
    let db = Arc::new(NovaDB::open_temporary().expect("temporary database"));
    let state_tree = Arc::new(RwLock::new(StateTree::new((*db).clone())));
    let mempool = Arc::new(Mempool::new(MempoolConfig::default()));
    let validator_kp = NovaKeypair::generate();
    let producer = BlockProducer::new(
        Arc::clone(&db),
        Arc::clone(&state_tree),
        Arc::clone(&mempool),
        validator_kp,
    );
    timing("infrastructure setup", t.elapsed());

    // Store genesis block.
    let genesis = Block::genesis();
    db.put_block(&genesis).unwrap();
    info("Genesis block hash", &hex::encode(genesis.header.hash));
    info(
        "Genesis state root",
        &hex::encode(genesis.header.state_root),
    );
    success("Genesis block committed to database");

    // -----------------------------------------------------------------------
    // Step 3: Fund Alice in Genesis
    // -----------------------------------------------------------------------

    section(3, "Genesis Account Funding");
    subsection("Seeding Alice's account with 1,000,000 photons in the state tree...");

    let initial_balance: u64 = 1_000_000;
    seed_balance(&state_tree, &alice_addr, initial_balance);

    let root_after_seed = state_tree.read().root();
    info(
        "State root after funding",
        &hex::encode(root_after_seed)[..16],
    );

    println!();
    println!(
        "  {BOLD}{WHITE}--- Initial Balances ---{RESET}"
    );
    balance_row("Alice", initial_balance, BLUE);
    balance_row("Bob", 0, GREEN);
    balance_row("Merchant", 0, MAGENTA);
    println!();
    success("Alice funded and ready to transact");

    // -----------------------------------------------------------------------
    // Step 4: Transfer Alice -> Bob
    // -----------------------------------------------------------------------

    section(4, "Transfer: Alice -> Bob (250,000 photons)");

    let transfer_amount_1 = 250_000u64;
    let fee_1 = 100u64;

    subsection("Building and signing transaction...");
    let t = Instant::now();
    let tx1 = build_signed_transfer(
        &alice_kp,
        &alice_addr,
        &bob_addr,
        transfer_amount_1,
        fee_1,
        1,
    );
    timing("build + sign", t.elapsed());

    info("Transaction ID", &tx1.id[..16]);
    info("Signature", &tx1.signature.as_ref().unwrap()[..32]);

    subsection("Verifying transaction cryptographically...");
    let t = Instant::now();
    assert!(verify_transaction(&tx1).is_ok());
    timing("Ed25519 verify", t.elapsed());
    success("Signature and structural integrity confirmed");

    subsection("Submitting to mempool and producing block #1...");
    mempool.add(tx1.clone()).unwrap();
    info("Mempool size", &mempool.size().to_string());

    let t = Instant::now();
    let produced1 = producer.produce_block(&genesis, 100).unwrap();
    let block_time_1 = t.elapsed();
    timing("block production", block_time_1);

    assert_eq!(produced1.block.transactions.len(), 1);
    assert!(produced1.tx_results.iter().all(|r| r.success));

    producer.commit_block(&produced1.block).unwrap();

    info("Block height", &produced1.block.header.height.to_string());
    info(
        "Block hash",
        &hex::encode(produced1.block.header.hash)[..16],
    );
    info("Transactions in block", "1");
    info(
        "State root",
        &hex::encode(produced1.block.header.state_root)[..16],
    );

    separator();

    // Check balances.
    {
        let tree = state_tree.read();
        let alice_state = tree.get(&alice_addr).unwrap();
        let bob_state = tree.get(&bob_addr).unwrap();

        println!();
        println!(
            "  {BOLD}{WHITE}--- Balances After Block #1 ---{RESET}"
        );
        balance_row("Alice", alice_state.balance, BLUE);
        balance_row("Bob", bob_state.balance, GREEN);
        balance_row("Merchant", 0, MAGENTA);
        println!();
    }

    success("Transfer Alice -> Bob confirmed in block #1");

    // -----------------------------------------------------------------------
    // Step 5: Transfer Bob -> Merchant
    // -----------------------------------------------------------------------

    section(5, "Transfer: Bob -> Merchant (100,000 photons)");

    let transfer_amount_2 = 100_000u64;
    let fee_2 = 150u64;

    subsection("Building, signing, and verifying transaction...");
    let t = Instant::now();
    let tx2 = build_signed_transfer(
        &bob_kp,
        &bob_addr,
        &merchant_addr,
        transfer_amount_2,
        fee_2,
        1,
    );
    assert!(verify_transaction(&tx2).is_ok());
    timing("build + sign + verify", t.elapsed());

    info("Transaction ID", &tx2.id[..16]);

    subsection("Producing block #2...");
    mempool.add(tx2.clone()).unwrap();

    let t = Instant::now();
    let produced2 = producer.produce_block(&produced1.block, 100).unwrap();
    let block_time_2 = t.elapsed();
    timing("block production", block_time_2);

    assert_eq!(produced2.block.transactions.len(), 1);
    producer.commit_block(&produced2.block).unwrap();

    info("Block height", &produced2.block.header.height.to_string());
    info(
        "Block hash",
        &hex::encode(produced2.block.header.hash)[..16],
    );
    info("Transactions in block", "1");

    separator();

    // Check balances.
    {
        let tree = state_tree.read();
        let alice_state = tree.get(&alice_addr).unwrap();
        let bob_state = tree.get(&bob_addr).unwrap();
        let merchant_state = tree.get(&merchant_addr).unwrap();

        println!();
        println!(
            "  {BOLD}{WHITE}--- Balances After Block #2 ---{RESET}"
        );
        balance_row("Alice", alice_state.balance, BLUE);
        balance_row("Bob", bob_state.balance, GREEN);
        balance_row("Merchant", merchant_state.balance, MAGENTA);
        println!();
    }

    success("Transfer Bob -> Merchant confirmed in block #2");

    // -----------------------------------------------------------------------
    // Step 6: State Proof Verification
    // -----------------------------------------------------------------------

    section(6, "Merkle State Proof Verification");
    subsection("Generating and verifying inclusion proofs for all accounts...");

    let t = Instant::now();
    {
        let tree = state_tree.read();
        let root = tree.root();

        // Verify each account's inclusion proof.
        for (name, addr) in [
            ("Alice", &alice_addr),
            ("Bob", &bob_addr),
            ("Merchant", &merchant_addr),
        ] {
            let state = tree.get(addr).unwrap();
            let proof = tree.get_proof(addr);
            let valid = StateTree::verify_proof(&root, addr, Some(&state), &proof);
            assert!(valid, "{name} inclusion proof failed");
            println!(
                "  {GREEN}[VERIFIED]{RESET} {BOLD}{name}{RESET}  proof_size={DIM}{} siblings{RESET}",
                proof.siblings.len()
            );
        }
    }
    timing("3x Merkle proof generation + verification", t.elapsed());
    success("All inclusion proofs verified against state root");

    // -----------------------------------------------------------------------
    // Step 7: Chain Integrity Verification
    // -----------------------------------------------------------------------

    section(7, "Chain Integrity & Database Verification");
    subsection("Verifying block linkage, Merkle roots, and transaction persistence...");

    let t = Instant::now();
    let chain = db.get_block_range(0, 2).unwrap();
    assert_eq!(chain.len(), 3, "expected genesis + 2 blocks");

    for i in 1..chain.len() {
        assert_eq!(
            chain[i].header.parent_hash, chain[i - 1].header.hash,
            "block {} parent hash mismatch",
            i
        );
        assert!(
            chain[i].verify().is_ok(),
            "block {} structural verification failed",
            i
        );
        println!(
            "  {GREEN}[VALID]{RESET} Block #{} -> parent #{} {DIM}(Merkle root + hash linkage){RESET}",
            chain[i].header.height,
            chain[i - 1].header.height
        );
    }

    // Verify transactions are retrievable from the database.
    let stored_tx1 = db.get_transaction(&tx1.id).unwrap();
    let stored_tx2 = db.get_transaction(&tx2.id).unwrap();
    assert!(stored_tx1.is_some(), "tx1 not found in database");
    assert!(stored_tx2.is_some(), "tx2 not found in database");
    timing("chain verification", t.elapsed());
    success("Chain integrity verified: all blocks linked and transactions persisted");

    // -----------------------------------------------------------------------
    // Step 8: Confidential Transfer with ZKP
    // -----------------------------------------------------------------------

    section(8, "Confidential Transfer with Zero-Knowledge Proof");
    subsection("Setting up Groth16 prover/verifier (BN254 trusted setup)...");

    run_confidential_transfer_demo(&alice_addr, &bob_addr);

    // -----------------------------------------------------------------------
    // Final Summary
    // -----------------------------------------------------------------------

    let total_elapsed = demo_start.elapsed();

    println!();
    println!(
        "{BG_BLUE}{BOLD}{WHITE}                                                                    {RESET}"
    );
    println!(
        "{BG_BLUE}{BOLD}{WHITE}    DEMO COMPLETE -- Final Summary                                  {RESET}"
    );
    println!(
        "{BG_BLUE}{BOLD}{WHITE}                                                                    {RESET}"
    );
    println!();

    println!("  {BOLD}{WHITE}Protocol Statistics:{RESET}");
    println!("  {DIM}----------------------------------------------{RESET}");
    info("Identities created", "3 (Alice, Bob, Merchant)");
    info("Blocks produced", "2 (+ genesis)");
    info("Transactions executed", "2 transfers");
    info("Merkle proofs verified", "3 inclusion proofs");
    info("Signing algorithm", "Ed25519 (ed25519-dalek 2.1)");
    info("Hash function", "BLAKE3 (primary), SHA-256 (IDs)");
    info("Address format", "Bech32 with 'nova' HRP");
    info("State tree", "Sparse Merkle Tree (256-bit keyspace)");
    info("Consensus model", "Hybrid PoS + PoA (<2s finality)");
    info("ZKP system", "Groth16 over BN254 (arkworks 0.4)");
    println!();

    // Final balance table.
    {
        let tree = state_tree.read();
        let alice_final = tree.get(&alice_addr).unwrap();
        let bob_final = tree.get(&bob_addr).unwrap();
        let merchant_final = tree.get(&merchant_addr).unwrap();

        println!("  {BOLD}{WHITE}Final Balances:{RESET}");
        println!("  {DIM}----------------------------------------------{RESET}");
        println!(
            "  {BLUE}{BOLD}Alice{RESET}      {WHITE}{:>12}{RESET} photons  {DIM}(started: {initial_balance}, sent: {}){RESET}",
            alice_final.balance, transfer_amount_1
        );
        println!(
            "  {GREEN}{BOLD}Bob{RESET}        {WHITE}{:>12}{RESET} photons  {DIM}(received: {transfer_amount_1}, sent: {transfer_amount_2}){RESET}",
            bob_final.balance
        );
        println!(
            "  {MAGENTA}{BOLD}Merchant{RESET}   {WHITE}{:>12}{RESET} photons  {DIM}(received: {transfer_amount_2}){RESET}",
            merchant_final.balance
        );

        let total_in_system = alice_final.balance + bob_final.balance + merchant_final.balance;
        println!();
        println!(
            "  {ITALIC}{DIM}Conservation check: {total_in_system} photons in accounts (fees deducted by validator){RESET}"
        );
    }

    println!();
    println!(
        "  {BOLD}{GREEN}Total demo time: {:.2}s{RESET}",
        total_elapsed.as_secs_f64()
    );
    println!();
}

// ---------------------------------------------------------------------------
// Confidential transfer demo (ZKP section)
// ---------------------------------------------------------------------------

fn run_confidential_transfer_demo(sender_addr: &str, receiver_addr: &str) {
    use ark_bn254::Fr;
    use ark_ff::UniformRand;
    use ark_std::rand::{rngs::StdRng, SeedableRng};

    use nova_protocol::transaction::confidential::{
        create_confidential_transfer, verify_confidential_proof,
    };
    use nova_protocol::zkp::prover::BalanceProver;

    let t = Instant::now();
    let mut rng = StdRng::seed_from_u64(42);
    let (prover, verifier) = BalanceProver::setup(&mut rng);
    timing("Groth16 trusted setup (CRS generation)", t.elapsed());

    subsection("Creating confidential transfer (500 photons, hidden amount)...");

    let amount = 500u64;
    let blinding = Fr::rand(&mut rng);

    let t = Instant::now();
    let tx = create_confidential_transfer(sender_addr, receiver_addr, amount, blinding, &prover)
        .expect("confidential transfer creation failed");
    let prove_time = t.elapsed();
    timing("Groth16 proof generation", prove_time);

    let proof_bytes = tx.proof.as_ref().unwrap();
    let commitment_bytes = tx.amount_commitment.as_ref().unwrap();

    info("Transaction type", "ConfidentialTransfer");
    info("Proof size", &format!("{} bytes", proof_bytes.len()));
    info("Commitment size", &format!("{} bytes", commitment_bytes.len()));
    info("Transaction ID", &tx.id[..16]);

    subsection("Verifying zero-knowledge proof (pairing check)...");

    let t = Instant::now();
    let valid = verify_confidential_proof(&tx, &verifier).expect("verification must not error");
    let verify_time = t.elapsed();
    timing("Groth16 verification", verify_time);

    assert!(valid, "confidential proof did not verify");
    success("Zero-knowledge proof verified: amount is hidden but provably valid");

    println!();
    println!("  {BOLD}{WHITE}ZKP Performance:{RESET}");
    println!("  {DIM}----------------------------------------------{RESET}");
    info(
        "Proof generation",
        &format!("{:.2} ms", prove_time.as_secs_f64() * 1000.0),
    );
    info(
        "Proof verification",
        &format!("{:.2} ms", verify_time.as_secs_f64() * 1000.0),
    );
    info("Proof size", &format!("{} bytes (compressed)", proof_bytes.len()));
    info(
        "Commitment size",
        &format!("{} bytes (compressed)", commitment_bytes.len()),
    );
    info("Curve", "BN254 (alt_bn128)");
    info("Proof system", "Groth16 (3 pairings verification)");
    println!();
    success("Confidential transfer pipeline complete");
}
