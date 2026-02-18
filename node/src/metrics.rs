//! # Prometheus Metrics
//!
//! Exposes operational metrics for the validator node. Scraped by Prometheus
//! at the `/metrics` HTTP endpoint on the configured metrics port.
//!
//! All metrics are registered in a dedicated [`prometheus::Registry`] so they
//! do not collide with any default global registry consumers.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use prometheus::{Encoder, Histogram, HistogramOpts, IntCounter, IntGauge, Registry, TextEncoder};
use std::sync::Arc;

/// Holds all Prometheus metric handles for the node.
///
/// Clone-friendly (wraps `Arc` internally via prometheus handles) so it can
/// be shared across request handlers and background tasks.
#[derive(Clone)]
#[allow(dead_code)]
pub struct NodeMetrics {
    /// Prometheus registry that owns all metrics below.
    registry: Registry,
    /// Total number of blocks processed (finalized) by this node.
    pub blocks_processed_total: IntCounter,
    /// Total number of transactions processed (included in finalized blocks).
    pub transactions_processed_total: IntCounter,
    /// Current number of transactions waiting in the mempool.
    pub transactions_in_mempool: IntGauge,
    /// Number of currently connected P2P peers.
    pub connected_peers: IntGauge,
    /// Total number of consensus rounds participated in.
    pub consensus_rounds_total: IntCounter,
    /// Current block height (latest finalized block).
    pub block_height: IntGauge,
    /// Histogram of transaction processing latency in seconds.
    pub transaction_latency_seconds: Histogram,
}

impl NodeMetrics {
    /// Creates and registers all metrics. Call once at startup.
    pub fn new() -> Self {
        let registry = Registry::new_custom(Some("nova".into()), None)
            .expect("failed to create prometheus registry");

        let blocks_processed_total = IntCounter::new(
            "blocks_processed_total",
            "Total number of finalized blocks processed",
        )
        .expect("metric creation");
        registry
            .register(Box::new(blocks_processed_total.clone()))
            .expect("metric registration");

        let transactions_processed_total = IntCounter::new(
            "transactions_processed_total",
            "Total number of transactions included in finalized blocks",
        )
        .expect("metric creation");
        registry
            .register(Box::new(transactions_processed_total.clone()))
            .expect("metric registration");

        let transactions_in_mempool = IntGauge::new(
            "transactions_in_mempool",
            "Current number of pending transactions in the mempool",
        )
        .expect("metric creation");
        registry
            .register(Box::new(transactions_in_mempool.clone()))
            .expect("metric registration");

        let connected_peers =
            IntGauge::new("connected_peers", "Number of currently connected P2P peers")
                .expect("metric creation");
        registry
            .register(Box::new(connected_peers.clone()))
            .expect("metric registration");

        let consensus_rounds_total = IntCounter::new(
            "consensus_rounds_total",
            "Total number of consensus rounds this node has participated in",
        )
        .expect("metric creation");
        registry
            .register(Box::new(consensus_rounds_total.clone()))
            .expect("metric registration");

        let block_height = IntGauge::new("block_height", "Height of the latest finalized block")
            .expect("metric creation");
        registry
            .register(Box::new(block_height.clone()))
            .expect("metric registration");

        let transaction_latency_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "transaction_latency_seconds",
                "End-to-end transaction processing latency in seconds",
            )
            .buckets(vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
            ]),
        )
        .expect("metric creation");
        registry
            .register(Box::new(transaction_latency_seconds.clone()))
            .expect("metric registration");

        Self {
            registry,
            blocks_processed_total,
            transactions_processed_total,
            transactions_in_mempool,
            connected_peers,
            consensus_rounds_total,
            block_height,
            transaction_latency_seconds,
        }
    }

    /// Encodes all registered metrics into the Prometheus text exposition format.
    pub fn encode(&self) -> Result<String, prometheus::Error> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8(buffer).expect("prometheus output is valid utf-8"))
    }
}

/// Shared metrics state passed to axum handlers via extension.
pub type SharedMetrics = Arc<NodeMetrics>;

/// Axum handler that renders `/metrics` in Prometheus text format.
///
/// Returns HTTP 500 if encoding fails (should never happen in practice).
pub async fn metrics_handler(
    axum::extract::State(metrics): axum::extract::State<SharedMetrics>,
) -> impl IntoResponse {
    match metrics.encode() {
        Ok(body) => (
            StatusCode::OK,
            [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
            body,
        )
            .into_response(),
        Err(e) => {
            tracing::error!("failed to encode metrics: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "metrics encoding failed").into_response()
        }
    }
}
