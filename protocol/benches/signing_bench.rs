// Signing & verification benchmarks for the NOVA protocol.
//
// Covers Ed25519 keypair generation, single-message signing and verification,
// transaction signing, and batch verification at various sizes.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use nova_protocol::crypto::keys::NovaKeypair;
use nova_protocol::crypto::signatures::{batch_verify, sign, verify};
use nova_protocol::transaction::builder::TransactionBuilder;
use nova_protocol::transaction::signing::sign_transaction;
use nova_protocol::transaction::types::{Amount, Currency, TransactionType};

fn bench_keypair_generation(c: &mut Criterion) {
    c.bench_function("ed25519/keypair_generate", |b| {
        b.iter(NovaKeypair::generate);
    });
}

fn bench_sign_message(c: &mut Criterion) {
    let keypair = NovaKeypair::generate();
    let message = b"transfer 500 NOVA from alice to bob; nonce=42";

    c.bench_function("ed25519/sign_message", |b| {
        b.iter(|| sign(&keypair, message));
    });
}

fn bench_verify_signature(c: &mut Criterion) {
    let keypair = NovaKeypair::generate();
    let message = b"transfer 500 NOVA from alice to bob; nonce=42";
    let signature = sign(&keypair, message);
    let public_key = keypair.public_key();

    c.bench_function("ed25519/verify_signature", |b| {
        b.iter(|| verify(&public_key, message, &signature));
    });
}

fn bench_sign_transaction(c: &mut Criterion) {
    let keypair = NovaKeypair::generate();

    c.bench_function("ed25519/sign_transaction", |b| {
        b.iter(|| {
            let mut tx = TransactionBuilder::new(TransactionType::Transfer)
                .sender("nova1qw508d6qejxtdg4y5r3zarvary0c5xw7k3sxhl")
                .receiver("nova1qrp33g0q5b5698ahp5jnf0y5ems8f9rrm4n7dh")
                .amount(Amount::new(1_000_000, Currency::NOVA))
                .fee(100)
                .nonce(42)
                .timestamp(1_700_000_000_000)
                .build();
            sign_transaction(&mut tx, &keypair);
        });
    });
}

fn bench_batch_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("ed25519/batch_verify");

    for size in [10, 50, 100, 500] {
        let items: Vec<_> = (0..size)
            .map(|i| {
                let kp = NovaKeypair::generate();
                let msg = format!("tx-{:06}", i).into_bytes();
                let sig = sign(&kp, &msg);
                (kp.public_key(), msg, sig)
            })
            .collect();

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &items, |b, items| {
            b.iter(|| batch_verify(items).unwrap());
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_keypair_generation,
    bench_sign_message,
    bench_verify_signature,
    bench_sign_transaction,
    bench_batch_verify,
);
criterion_main!(benches);
