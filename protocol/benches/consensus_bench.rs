// Consensus engine benchmarks for the NOVA protocol.
//
// Covers vote creation and verification, block proposal, block validation,
// and full finalization with quorum vote collection.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use nova_protocol::crypto::keys::NovaKeypair;
use nova_protocol::network::consensus::{
    ConsensusConfig, ConsensusEngine, ValidatorSet, Vote,
};

/// Sets up a consensus engine with `n` validators and returns the engine,
/// the keypairs (sorted by stake — highest first), and the validator set.
fn setup_engine(n: usize) -> (ConsensusEngine, Vec<NovaKeypair>) {
    let mut keypairs = Vec::with_capacity(n);
    let mut validator_set = ValidatorSet::new();

    for i in 0..n {
        let kp = NovaKeypair::generate();
        let address = kp.public_key().to_hex();
        // Give descending stake so sort order is predictable.
        let stake = (n - i) as u64 * 1_000_000_000;
        validator_set.add_validator(address, stake);
        keypairs.push(kp);
    }

    let config = ConsensusConfig {
        min_validators: 1,
        max_validators: n.max(4),
        ..ConsensusConfig::default()
    };

    let engine = ConsensusEngine::new(config, validator_set);
    (engine, keypairs)
}

fn bench_vote_creation(c: &mut Criterion) {
    let keypair = NovaKeypair::generate();
    let block_hash = [0xABu8; 32];

    c.bench_function("consensus/vote_create", |b| {
        b.iter(|| Vote::new(&keypair, block_hash, 0));
    });
}

fn bench_vote_verification(c: &mut Criterion) {
    let keypair = NovaKeypair::generate();
    let block_hash = [0xABu8; 32];
    let vote = Vote::new(&keypair, block_hash, 0);

    c.bench_function("consensus/vote_verify", |b| {
        b.iter(|| vote.verify());
    });
}

fn bench_block_proposal(c: &mut Criterion) {
    let (engine, keypairs) = setup_engine(7);
    let proposer = &keypairs[0]; // Highest stake = round-0 proposer.

    c.bench_function("consensus/block_propose", |b| {
        b.iter(|| engine.propose_block(vec![], proposer).unwrap());
    });
}

fn bench_block_validation(c: &mut Criterion) {
    let (engine, keypairs) = setup_engine(7);
    let proposer = &keypairs[0];
    let block = engine.propose_block(vec![], proposer).unwrap();

    c.bench_function("consensus/block_validate", |b| {
        b.iter(|| engine.validate_block(&block).unwrap());
    });
}

fn bench_finalize_block(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus/finalize_block");

    for validator_count in [4, 7, 13, 21] {
        group.throughput(Throughput::Elements(validator_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(validator_count),
            &validator_count,
            |b, &n| {
                b.iter_with_setup(
                    || {
                        let (engine, keypairs) = setup_engine(n);
                        let proposer = &keypairs[0];
                        let block = engine.propose_block(vec![], proposer).unwrap();
                        let block_hash = block.header.hash;

                        // Collect enough votes to meet quorum.
                        let quorum = engine.validator_set().quorum_threshold();
                        let votes: Vec<Vote> = keypairs
                            .iter()
                            .take(quorum)
                            .map(|kp| Vote::new(kp, block_hash, 0))
                            .collect();

                        (engine, block, votes)
                    },
                    |(mut engine, block, votes)| {
                        engine.finalize_block(block, votes).unwrap();
                    },
                );
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_vote_creation,
    bench_vote_verification,
    bench_block_proposal,
    bench_block_validation,
    bench_finalize_block,
);
criterion_main!(benches);
