//! # Structured Logging
//!
//! Initializes the `tracing` subscriber with configurable format (JSON or
//! pretty-printed) and environment-based filtering via `RUST_LOG`.
//!
//! All log output is written to stderr so that stdout remains available for
//! structured data (e.g., JSON-RPC responses piped through the binary).

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum LogFormat {
    /// Human-readable, colored output. Suitable for local development.
    Pretty,
    /// Machine-parseable JSON lines. Suitable for production log aggregation.
    Json,
}

impl LogFormat {
    /// Parse a format string. Accepts "json" or "pretty" (case-insensitive).
    /// Returns `Pretty` for any unrecognized value.
    #[allow(dead_code)]
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => LogFormat::Json,
            _ => LogFormat::Pretty,
        }
    }
}

/// Initialize the global tracing subscriber.
///
/// Call this exactly once, early in `main()`. Subsequent calls will panic.
///
/// # Arguments
///
/// * `default_level` - The default log level when `RUST_LOG` is not set.
///   Typical values: `"info"`, `"debug"`, `"nova_node=debug,nova_protocol=info"`.
/// * `format` - Output format (JSON or pretty-printed).
///
/// # Environment
///
/// The `RUST_LOG` environment variable overrides `default_level` when set.
/// Syntax follows the `tracing_subscriber::EnvFilter` directives, e.g.:
///
/// ```text
/// RUST_LOG=nova_node=debug,nova_protocol=info,tower_http=debug
/// ```
pub fn init_logging(default_level: &str, format: LogFormat) {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    match format {
        LogFormat::Pretty => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    fmt::layer()
                        .with_target(true)
                        .with_thread_ids(false)
                        .with_file(true)
                        .with_line_number(true),
                )
                .init();
        }
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().json().with_target(true))
                .init();
        }
    }

    tracing::info!("logging initialized (format={:?})", format);
}
