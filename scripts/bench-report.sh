#!/usr/bin/env bash
# bench-report.sh — Run all NOVA protocol benchmarks and produce a summary.
#
# Usage:
#   ./scripts/bench-report.sh            # Print summary to stdout
#   ./scripts/bench-report.sh --save     # Also write docs/benchmarks/BENCHMARKS.md
#
# Requires: cargo, criterion (dev-dependency in protocol crate)

set -euo pipefail

SAVE_REPORT=false
if [[ "${1:-}" == "--save" ]]; then
    SAVE_REPORT=true
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REPORT_DIR="$PROJECT_ROOT/docs/benchmarks"
REPORT_FILE="$REPORT_DIR/BENCHMARKS.md"

# ── Collect system info ─────────────────────────────────────────────────────

ARCH="$(uname -m)"
OS_VERSION="$(sw_vers -productVersion 2>/dev/null || uname -r)"
RUST_VERSION="$(rustc --version)"
CPU="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo "unknown")"
RAM="$(sysctl -n hw.memsize 2>/dev/null | awk '{printf "%.0f GB", $0/1073741824}' || echo "unknown")"
RUN_DATE="$(date +%Y-%m-%d)"

# ── Run benchmarks ──────────────────────────────────────────────────────────

echo "Running NOVA protocol benchmarks..."
echo ""

SIGNING_OUT=$(cargo bench -p nova-protocol --bench signing_bench 2>&1)
ZKP_OUT=$(cargo bench -p nova-protocol --bench zkp_bench 2>&1)
CONSENSUS_OUT=$(cargo bench -p nova-protocol --bench consensus_bench 2>&1)

# ── Parse Criterion output ──────────────────────────────────────────────────
# Criterion prints lines like:
#   bench_name          time:   [low est median est high est]
# We extract the median (second value in the bracket).

parse_time() {
    local output="$1"
    local bench_name="$2"
    echo "$output" \
        | grep -A 1 "^${bench_name}" \
        | grep "time:" \
        | sed 's/.*\[.*[[:space:]]\(.*\)[[:space:]].*\]/\1/' \
        | head -1
}

parse_throughput() {
    local output="$1"
    local bench_name="$2"
    echo "$output" \
        | grep -A 2 "^${bench_name}" \
        | grep "thrpt:" \
        | sed 's/.*\[.*[[:space:]]\(.*\)[[:space:]].*\]/\1/' \
        | head -1
}

# ── Extract signing results ─────────────────────────────────────────────────

KEYGEN_TIME=$(parse_time "$SIGNING_OUT" "ed25519/keypair_generate")
SIGN_TIME=$(parse_time "$SIGNING_OUT" "ed25519/sign_message")
VERIFY_TIME=$(parse_time "$SIGNING_OUT" "ed25519/verify_signature")
SIGN_TX_TIME=$(parse_time "$SIGNING_OUT" "ed25519/sign_transaction")
BATCH_10_TIME=$(parse_time "$SIGNING_OUT" "ed25519/batch_verify/10")
BATCH_50_TIME=$(parse_time "$SIGNING_OUT" "ed25519/batch_verify/50")
BATCH_100_TIME=$(parse_time "$SIGNING_OUT" "ed25519/batch_verify/100")
BATCH_500_TIME=$(parse_time "$SIGNING_OUT" "ed25519/batch_verify/500")

# ── Extract ZKP results ─────────────────────────────────────────────────────

SETUP_TIME=$(parse_time "$ZKP_OUT" "zkp/groth16_setup")
COMMIT_TIME=$(parse_time "$ZKP_OUT" "zkp/pedersen_commit")
PROVE_TIME=$(parse_time "$ZKP_OUT" "zkp/groth16_prove")
ZKP_VERIFY_TIME=$(parse_time "$ZKP_OUT" "zkp/groth16_verify")

# ── Extract consensus results ───────────────────────────────────────────────

VOTE_CREATE_TIME=$(parse_time "$CONSENSUS_OUT" "consensus/vote_create")
VOTE_VERIFY_TIME=$(parse_time "$CONSENSUS_OUT" "consensus/vote_verify")
PROPOSE_TIME=$(parse_time "$CONSENSUS_OUT" "consensus/block_propose")
VALIDATE_TIME=$(parse_time "$CONSENSUS_OUT" "consensus/block_validate")
FINALIZE_4_TIME=$(parse_time "$CONSENSUS_OUT" "consensus/finalize_block/4")
FINALIZE_7_TIME=$(parse_time "$CONSENSUS_OUT" "consensus/finalize_block/7")
FINALIZE_13_TIME=$(parse_time "$CONSENSUS_OUT" "consensus/finalize_block/13")
FINALIZE_21_TIME=$(parse_time "$CONSENSUS_OUT" "consensus/finalize_block/21")

# ── Print summary ───────────────────────────────────────────────────────────

print_report() {
cat <<REPORT
# NOVA Protocol Benchmark Report

**Date:** ${RUN_DATE}
**System:** ${CPU} (${ARCH}), ${RAM} RAM, macOS ${OS_VERSION}
**Toolchain:** ${RUST_VERSION}, release profile
**Framework:** Criterion 0.5.1 (100 samples per benchmark)

---

## Ed25519 Signing & Verification

| Operation | Time (median) |
|---|---|
| Keypair generation | ${KEYGEN_TIME} |
| Sign message (46 B) | ${SIGN_TIME} |
| Verify signature | ${VERIFY_TIME} |
| Sign transaction (build + sign) | ${SIGN_TX_TIME} |

### Batch Verification

| Batch Size | Total Time (median) |
|---|---|
| 10 | ${BATCH_10_TIME} |
| 50 | ${BATCH_50_TIME} |
| 100 | ${BATCH_100_TIME} |
| 500 | ${BATCH_500_TIME} |

---

## Zero-Knowledge Proofs (Groth16 / BN254)

| Operation | Time (median) |
|---|---|
| Trusted setup | ${SETUP_TIME} |
| Pedersen commitment | ${COMMIT_TIME} |
| Groth16 prove | ${PROVE_TIME} |
| Groth16 verify | ${ZKP_VERIFY_TIME} |

---

## Consensus Engine

| Operation | Time (median) |
|---|---|
| Vote creation (sign) | ${VOTE_CREATE_TIME} |
| Vote verification | ${VOTE_VERIFY_TIME} |
| Block proposal (empty, 7 validators) | ${PROPOSE_TIME} |
| Block validation (empty, 7 validators) | ${VALIDATE_TIME} |

### Block Finalization

| Validators | Time (median) |
|---|---|
| 4 (quorum 3) | ${FINALIZE_4_TIME} |
| 7 (quorum 5) | ${FINALIZE_7_TIME} |
| 13 (quorum 9) | ${FINALIZE_13_TIME} |
| 21 (quorum 15) | ${FINALIZE_21_TIME} |

---

## How to Reproduce

\`\`\`bash
make bench           # Run all benchmarks
make bench-signing   # Signing only
make bench-zkp       # ZKP only
make bench-consensus # Consensus only
make bench-report    # Generate this report
\`\`\`
REPORT
}

print_report

if [[ "$SAVE_REPORT" == "true" ]]; then
    mkdir -p "$REPORT_DIR"
    print_report > "$REPORT_FILE"
    echo ""
    echo "Report saved to: ${REPORT_FILE}"
fi
