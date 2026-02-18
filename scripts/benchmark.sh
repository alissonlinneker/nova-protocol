#!/usr/bin/env bash
# =============================================================================
# NOVA Protocol — Performance Benchmarks
# Runs criterion benchmarks, transaction throughput tests, and ZKP latency
# measurements, then produces a formatted results table.
#
# Usage:
#   ./scripts/benchmark.sh               # run all benchmarks
#   ./scripts/benchmark.sh --signing     # signing benchmarks only
#   ./scripts/benchmark.sh --zkp         # ZKP benchmarks only
#   ./scripts/benchmark.sh --throughput  # throughput test only
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
RESULTS_DIR="${PROJECT_ROOT}/benchmark-results"

# Defaults
RUN_SIGNING=true
RUN_ZKP=true
RUN_CONSENSUS=true
RUN_THROUGHPUT=true

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --signing)
            RUN_ZKP=false; RUN_CONSENSUS=false; RUN_THROUGHPUT=false; shift ;;
        --zkp)
            RUN_SIGNING=false; RUN_CONSENSUS=false; RUN_THROUGHPUT=false; shift ;;
        --consensus)
            RUN_SIGNING=false; RUN_ZKP=false; RUN_THROUGHPUT=false; shift ;;
        --throughput)
            RUN_SIGNING=false; RUN_ZKP=false; RUN_CONSENSUS=false; shift ;;
        --output-dir)
            RESULTS_DIR="$2"; shift 2 ;;
        -h|--help)
            echo "Usage: $0 [--signing|--zkp|--consensus|--throughput] [--output-dir DIR]"
            exit 0
            ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

# Colors
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

log()  { echo -e "${CYAN}[bench]${NC} $*"; }
ok()   { echo -e "${GREEN}[ ok  ]${NC} $*"; }

# -----------------------------------------------------------------------------
# Setup
# -----------------------------------------------------------------------------
mkdir -p "${RESULTS_DIR}"
TIMESTAMP=$(date -u +"%Y%m%d-%H%M%S")
SUMMARY_FILE="${RESULTS_DIR}/summary-${TIMESTAMP}.txt"

{
    echo "============================================================"
    echo "  NOVA Protocol — Benchmark Results"
    echo "  Date: $(date -u +"%Y-%m-%d %H:%M:%S UTC")"
    echo "  Host: $(uname -n) ($(uname -m))"
    echo "  Rust: $(rustc --version 2>/dev/null || echo 'unknown')"
    echo "============================================================"
    echo ""
} > "${SUMMARY_FILE}"

# -----------------------------------------------------------------------------
# Criterion Benchmarks
# -----------------------------------------------------------------------------
run_criterion_bench() {
    local bench_name="$1"
    local label="$2"

    log "Running ${label} benchmarks..."

    local output_file="${RESULTS_DIR}/${bench_name}-${TIMESTAMP}.txt"

    if cargo bench -p nova-protocol --bench "${bench_name}" 2>&1 | tee "${output_file}"; then
        ok "${label} benchmarks complete."
    else
        echo "  WARNING: ${label} benchmarks failed or are not yet implemented." >> "${SUMMARY_FILE}"
        return 0
    fi

    # Parse criterion output for the summary
    {
        echo "--- ${label} ---"
        grep -E "^(test |Benchmarking|  time:)" "${output_file}" 2>/dev/null || echo "  (see full output: ${output_file})"
        echo ""
    } >> "${SUMMARY_FILE}"
}

if [[ "${RUN_SIGNING}" == "true" ]]; then
    run_criterion_bench "signing_bench" "Transaction Signing"
fi

if [[ "${RUN_ZKP}" == "true" ]]; then
    run_criterion_bench "zkp_bench" "ZKP Generation & Verification"
fi

if [[ "${RUN_CONSENSUS}" == "true" ]]; then
    run_criterion_bench "consensus_bench" "Consensus"
fi

# -----------------------------------------------------------------------------
# Transaction Throughput Test
# -----------------------------------------------------------------------------
if [[ "${RUN_THROUGHPUT}" == "true" ]]; then
    log "Running transaction throughput test..."

    throughput_output="${RESULTS_DIR}/throughput-${TIMESTAMP}.txt"

    # Run the throughput test binary if it exists, otherwise use cargo test
    if cargo test -p nova-protocol --release -- throughput --nocapture 2>&1 | tee "${throughput_output}"; then
        ok "Throughput test complete."
    else
        echo "  WARNING: Throughput test failed or is not yet implemented." >> "${SUMMARY_FILE}"
    fi

    {
        echo "--- Transaction Throughput ---"
        grep -E "(tx/s|TPS|throughput|transactions)" "${throughput_output}" 2>/dev/null || echo "  (see full output: ${throughput_output})"
        echo ""
    } >> "${SUMMARY_FILE}"
fi

# -----------------------------------------------------------------------------
# Results Summary
# -----------------------------------------------------------------------------
{
    echo "============================================================"
    echo "  Full reports: ${RESULTS_DIR}/"
    echo "  Criterion HTML: ${PROJECT_ROOT}/target/criterion/"
    echo "============================================================"
} >> "${SUMMARY_FILE}"

echo ""
echo -e "${BOLD}${GREEN}"
cat "${SUMMARY_FILE}"
echo -e "${NC}"

log "Results saved to ${SUMMARY_FILE}"
log "Criterion HTML reports: ${PROJECT_ROOT}/target/criterion/"
