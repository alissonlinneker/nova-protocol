// Zero-knowledge proof benchmarks for the NOVA protocol.
//
// Benchmarks Groth16 trusted setup, proof generation, and proof verification
// for the balance-proof circuit over BN254. Also covers Pedersen commitment
// computation since it is part of the ZKP pipeline.

use criterion::{criterion_group, criterion_main, Criterion};

use ark_bn254::Fr;
use ark_ff::UniformRand;
use ark_std::rand::{rngs::StdRng, SeedableRng};

use nova_protocol::zkp::commitment::{self, PedersenParams};
use nova_protocol::zkp::prover::BalanceProver;

fn bench_groth16_setup(c: &mut Criterion) {
    c.bench_function("zkp/groth16_setup", |b| {
        b.iter(|| {
            let mut rng = StdRng::seed_from_u64(42);
            BalanceProver::setup(&mut rng)
        });
    });
}

fn bench_pedersen_commit(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(42);
    let params = PedersenParams::setup(&mut rng);
    let blinding = Fr::rand(&mut rng);

    c.bench_function("zkp/pedersen_commit", |b| {
        b.iter(|| commitment::commit(&params, 1_000_000, blinding));
    });
}

fn bench_groth16_prove(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(42);
    let (prover, _verifier) = BalanceProver::setup(&mut rng);
    let params = prover.pedersen_params();

    let balance = 10_000u64;
    let blinding = Fr::rand(&mut rng);
    let comm = commitment::commit(params, balance, blinding);

    c.bench_function("zkp/groth16_prove", |b| {
        b.iter(|| {
            prover
                .prove(balance, blinding, 500, params, &comm)
                .unwrap()
        });
    });
}

fn bench_groth16_verify(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(42);
    let (prover, verifier) = BalanceProver::setup(&mut rng);
    let params = prover.pedersen_params();

    let balance = 10_000u64;
    let blinding = Fr::rand(&mut rng);
    let comm = commitment::commit(params, balance, blinding);
    let proof = prover.prove(balance, blinding, 500, params, &comm).unwrap();

    c.bench_function("zkp/groth16_verify", |b| {
        b.iter(|| verifier.verify(&proof, &comm, 500, params).unwrap());
    });
}

criterion_group!(
    benches,
    bench_groth16_setup,
    bench_pedersen_commit,
    bench_groth16_prove,
    bench_groth16_verify,
);
criterion_main!(benches);
