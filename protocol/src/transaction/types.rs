//! Core type definitions for NOVA transactions.
//!
//! These types form the vocabulary of every transaction on the network.
//! They are intentionally kept small and `Copy`-friendly where possible
//! to avoid heap allocations on the hot validation path.

use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// TransactionType
// ---------------------------------------------------------------------------

/// Discriminant for the operation a transaction represents.
///
/// Every transaction on the NOVA network falls into exactly one of these
/// categories. The type determines which validation rules apply and how
/// the state machine transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransactionType {
    /// Simple value transfer between two addresses.
    Transfer,
    /// Request to open or draw from a credit line.
    CreditRequest,
    /// Settlement (repayment) of an outstanding credit obligation.
    CreditSettlement,
    /// Mint new tokens into existence (requires issuer authority).
    TokenMint,
    /// Permanently destroy tokens, removing them from circulation.
    TokenBurn,
}

impl fmt::Display for TransactionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transfer => write!(f, "Transfer"),
            Self::CreditRequest => write!(f, "CreditRequest"),
            Self::CreditSettlement => write!(f, "CreditSettlement"),
            Self::TokenMint => write!(f, "TokenMint"),
            Self::TokenBurn => write!(f, "TokenBurn"),
        }
    }
}

// ---------------------------------------------------------------------------
// TransactionStatus
// ---------------------------------------------------------------------------

/// Lifecycle state of a transaction.
///
/// Transactions are `Pending` when first submitted to the mempool,
/// `Confirmed` once included in a finalized block, `Failed` if validation
/// or execution rejects them, and `Expired` if they sit in the mempool
/// past the TTL window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransactionStatus {
    /// Submitted to the mempool, awaiting block inclusion.
    Pending,
    /// Included in a finalized block and executed successfully.
    Confirmed,
    /// Rejected during validation or execution.
    Failed,
    /// Exceeded the mempool TTL without being included in a block.
    Expired,
}

impl fmt::Display for TransactionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::Confirmed => write!(f, "Confirmed"),
            Self::Failed => write!(f, "Failed"),
            Self::Expired => write!(f, "Expired"),
        }
    }
}

// ---------------------------------------------------------------------------
// Currency
// ---------------------------------------------------------------------------

/// Supported currency denominations.
///
/// These are the currencies the protocol natively understands for fee
/// calculation, exchange rate lookups, and display formatting. Custom
/// tokens use [`Currency::Custom`] with an arbitrary ticker string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Currency {
    /// Brazilian Real (smallest unit: centavo, 10^-2).
    BRL,
    /// United States Dollar (smallest unit: cent, 10^-2).
    USD,
    /// Euro (smallest unit: cent, 10^-2).
    EUR,
    /// Bitcoin (smallest unit: satoshi, 10^-8).
    BTC,
    /// Ether (smallest unit: wei, 10^-18, but we use gwei = 10^-9 in practice).
    ETH,
    /// USD Coin stablecoin (smallest unit: 10^-6).
    USDC,
    /// NOVA native token (smallest unit: photon, 10^-8).
    NOVA,
    /// Arbitrary token identifier for non-standard assets.
    Custom(String),
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BRL => write!(f, "BRL"),
            Self::USD => write!(f, "USD"),
            Self::EUR => write!(f, "EUR"),
            Self::BTC => write!(f, "BTC"),
            Self::ETH => write!(f, "ETH"),
            Self::USDC => write!(f, "USDC"),
            Self::NOVA => write!(f, "NOVA"),
            Self::Custom(ticker) => write!(f, "{}", ticker),
        }
    }
}

impl Currency {
    /// Returns the number of decimal places for display formatting.
    ///
    /// This is purely for human-readable output. The protocol always
    /// operates on integer amounts in the smallest unit.
    pub fn decimals(&self) -> u8 {
        match self {
            Self::BRL | Self::USD | Self::EUR => 2,
            Self::BTC | Self::NOVA => 8,
            Self::ETH => 9, // gwei precision
            Self::USDC => 6,
            Self::Custom(_) => 8, // sensible default
        }
    }
}

// ---------------------------------------------------------------------------
// Amount
// ---------------------------------------------------------------------------

/// A monetary amount expressed in the smallest indivisible unit of a currency.
///
/// `value` is always an integer -- no floating point anywhere near money.
/// For BTC, `value = 100_000_000` means 1 BTC. For USD, `value = 100`
/// means $1.00. The `currency` field determines the denomination.
///
/// # Examples
///
/// ```
/// use nova_protocol::transaction::types::{Amount, Currency};
///
/// let one_btc = Amount::new(100_000_000, Currency::BTC);
/// let fifty_cents = Amount::new(50, Currency::USD);
/// let ten_nova = Amount::new(1_000_000_000, Currency::NOVA);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Amount {
    /// Value in the smallest indivisible unit of the currency.
    pub value: u64,
    /// The currency denomination.
    pub currency: Currency,
}

impl Amount {
    /// Creates a new amount.
    pub fn new(value: u64, currency: Currency) -> Self {
        Self { value, currency }
    }

    /// Returns `true` if the amount is zero.
    pub fn is_zero(&self) -> bool {
        self.value == 0
    }

    /// Returns a human-readable string with decimal formatting.
    ///
    /// Example: `Amount { value: 150_000_000, currency: BTC }` becomes `"1.50000000 BTC"`.
    pub fn display_decimal(&self) -> String {
        let decimals = self.currency.decimals() as u32;
        let divisor = 10u64.pow(decimals);
        let whole = self.value / divisor;
        let frac = self.value % divisor;
        format!(
            "{}.{:0>width$} {}",
            whole,
            frac,
            self.currency,
            width = decimals as usize
        )
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.value, self.currency)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_type_display() {
        assert_eq!(TransactionType::Transfer.to_string(), "Transfer");
        assert_eq!(TransactionType::TokenBurn.to_string(), "TokenBurn");
    }

    #[test]
    fn transaction_status_display() {
        assert_eq!(TransactionStatus::Pending.to_string(), "Pending");
        assert_eq!(TransactionStatus::Confirmed.to_string(), "Confirmed");
    }

    #[test]
    fn currency_decimals() {
        assert_eq!(Currency::BTC.decimals(), 8);
        assert_eq!(Currency::USD.decimals(), 2);
        assert_eq!(Currency::NOVA.decimals(), 8);
        assert_eq!(Currency::USDC.decimals(), 6);
    }

    #[test]
    fn amount_display_decimal() {
        let amt = Amount::new(150_000_000, Currency::BTC);
        assert_eq!(amt.display_decimal(), "1.50000000 BTC");

        let usd = Amount::new(1050, Currency::USD);
        assert_eq!(usd.display_decimal(), "10.50 USD");
    }

    #[test]
    fn amount_is_zero() {
        assert!(Amount::new(0, Currency::NOVA).is_zero());
        assert!(!Amount::new(1, Currency::NOVA).is_zero());
    }

    #[test]
    fn currency_serde_roundtrip() {
        let currencies = vec![
            Currency::BRL,
            Currency::USD,
            Currency::EUR,
            Currency::BTC,
            Currency::ETH,
            Currency::USDC,
            Currency::NOVA,
            Currency::Custom("DOGE".to_string()),
        ];
        for c in currencies {
            let json = serde_json::to_string(&c).unwrap();
            let recovered: Currency = serde_json::from_str(&json).unwrap();
            assert_eq!(c, recovered);
        }
    }

    #[test]
    fn amount_serde_roundtrip() {
        let amt = Amount::new(42_000, Currency::NOVA);
        let json = serde_json::to_string(&amt).unwrap();
        let recovered: Amount = serde_json::from_str(&json).unwrap();
        assert_eq!(amt, recovered);
    }

    #[test]
    fn transaction_type_serde_roundtrip() {
        let types = vec![
            TransactionType::Transfer,
            TransactionType::CreditRequest,
            TransactionType::CreditSettlement,
            TransactionType::TokenMint,
            TransactionType::TokenBurn,
        ];
        for t in types {
            let json = serde_json::to_string(&t).unwrap();
            let recovered: TransactionType = serde_json::from_str(&json).unwrap();
            assert_eq!(t, recovered);
        }
    }
}
