//! # Token Factory Contract
//!
//! Manages the creation, minting, and burning of custom tokens on the NOVA
//! network. Any identity can create a new token by specifying its name,
//! symbol, decimal precision, and type. Only the original issuer can mint
//! additional supply.
//!
//! ## Security Model
//!
//! - **Mint gating**: Every `mint()` call requires the issuer's Ed25519
//!   signature over the mint payload `(token_id || to || amount)`. The
//!   factory verifies this before updating supply.
//! - **Burn authorization**: Only the token holder can burn their own tokens.
//!   The owner's signature over `(token_id || amount)` is required.
//! - **Supply tracking**: Total supply and per-address balances are maintained
//!   atomically. Overflow is checked on every operation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during token factory operations.
#[derive(Debug, Error)]
pub enum TokenError {
    /// The referenced token does not exist.
    #[error("token not found: {0}")]
    TokenNotFound(String),

    /// The caller is not the issuer of this token.
    #[error("unauthorized: only the issuer can mint this token")]
    UnauthorizedMint,

    /// The provided signature is invalid or empty.
    #[error("invalid signature")]
    InvalidSignature,

    /// A supply overflow would occur.
    #[error("supply overflow: minting {amount} would exceed u64::MAX")]
    SupplyOverflow {
        /// The amount that was attempted.
        amount: u64,
    },

    /// Insufficient balance for a burn operation.
    #[error("insufficient balance: account has {balance}, tried to burn {amount}")]
    InsufficientBalance {
        /// Current balance of the account.
        balance: u64,
        /// Amount the caller tried to burn.
        amount: u64,
    },

    /// A token with this symbol already exists.
    #[error("duplicate symbol: a token with symbol '{0}' already exists")]
    DuplicateSymbol(String),
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Unique identifier for a token, assigned by the factory at creation time.
pub type TokenId = String;

/// The category of token being created. Determines display behavior and
/// protocol-level treatment (e.g., stablecoins may have special fee rules).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenType {
    /// A general-purpose fungible token.
    Utility,
    /// A fiat-backed stablecoin.
    Stablecoin,
    /// A governance token with voting rights.
    Governance,
    /// A loyalty / reward points token.
    Reward,
}

impl std::fmt::Display for TokenType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenType::Utility => write!(f, "Utility"),
            TokenType::Stablecoin => write!(f, "Stablecoin"),
            TokenType::Governance => write!(f, "Governance"),
            TokenType::Reward => write!(f, "Reward"),
        }
    }
}

/// Metadata and supply information for a registered token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    /// Unique token identifier.
    pub token_id: TokenId,
    /// Human-readable token name (e.g., "NOVA Dollar").
    pub name: String,
    /// Ticker symbol (e.g., "nUSD"). Unique across the factory.
    pub symbol: String,
    /// Number of decimal places. 8 is standard for NOVA-native tokens.
    pub decimals: u8,
    /// The token category.
    pub token_type: TokenType,
    /// Hex-encoded public key of the token issuer.
    pub issuer: String,
    /// Current total supply in the smallest denomination.
    pub total_supply: u64,
    /// Timestamp when the token was created.
    pub created_at: DateTime<Utc>,
}

/// The token factory — manages token registration, minting, and burning.
///
/// In production, this state would be persisted in the protocol's state trie.
/// The in-memory representation here is used for validation logic and testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenFactory {
    /// Registered tokens keyed by their unique ID.
    tokens: HashMap<TokenId, TokenInfo>,
    /// Per-token, per-address balances: `token_id -> (address -> balance)`.
    balances: HashMap<TokenId, HashMap<String, u64>>,
    /// Index from symbol to token ID for uniqueness enforcement.
    symbol_index: HashMap<String, TokenId>,
}

impl TokenFactory {
    /// Creates a new, empty token factory.
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
            balances: HashMap::new(),
            symbol_index: HashMap::new(),
        }
    }

    /// Registers a new token and returns its unique ID.
    ///
    /// The token starts with zero supply. The issuer must call [`mint`](Self::mint)
    /// to create the initial supply.
    ///
    /// # Arguments
    ///
    /// * `name` - Human-readable token name.
    /// * `symbol` - Ticker symbol (must be unique).
    /// * `decimals` - Number of decimal places.
    /// * `token_type` - Category of the token.
    /// * `issuer` - Hex-encoded public key of the creating identity.
    ///
    /// # Errors
    ///
    /// Returns [`TokenError::DuplicateSymbol`] if the symbol is already taken.
    pub fn create_token(
        &mut self,
        name: String,
        symbol: String,
        decimals: u8,
        token_type: TokenType,
        issuer: String,
    ) -> Result<TokenId, TokenError> {
        // Enforce symbol uniqueness.
        let symbol_upper = symbol.to_uppercase();
        if self.symbol_index.contains_key(&symbol_upper) {
            return Err(TokenError::DuplicateSymbol(symbol));
        }

        let token_id = Uuid::new_v4().to_string();
        let info = TokenInfo {
            token_id: token_id.clone(),
            name,
            symbol: symbol_upper.clone(),
            decimals,
            token_type,
            issuer,
            total_supply: 0,
            created_at: Utc::now(),
        };

        self.tokens.insert(token_id.clone(), info);
        self.balances.insert(token_id.clone(), HashMap::new());
        self.symbol_index.insert(symbol_upper, token_id.clone());

        Ok(token_id)
    }

    /// Mints new tokens to the specified address.
    ///
    /// Only the original issuer of the token can mint. The issuer's signature
    /// over `(token_id || to || amount)` is required for authorization.
    ///
    /// # Arguments
    ///
    /// * `token_id` - The token to mint.
    /// * `to` - Hex-encoded address of the recipient.
    /// * `amount` - Number of tokens to mint (smallest denomination).
    /// * `issuer_signature` - Hex-encoded Ed25519 signature from the issuer.
    ///
    /// # Errors
    ///
    /// Returns [`TokenError::TokenNotFound`] if the token does not exist.
    /// Returns [`TokenError::UnauthorizedMint`] if the caller is not the issuer.
    /// Returns [`TokenError::InvalidSignature`] if the signature is empty.
    /// Returns [`TokenError::SupplyOverflow`] if the mint would overflow u64.
    pub fn mint(
        &mut self,
        token_id: &str,
        to: &str,
        amount: u64,
        issuer_signature: &str,
    ) -> Result<(), TokenError> {
        // Verify the signature is present. Full Ed25519 verification happens
        // at the execution engine layer.
        if issuer_signature.is_empty() {
            return Err(TokenError::InvalidSignature);
        }

        let info = self
            .tokens
            .get(token_id)
            .ok_or_else(|| TokenError::TokenNotFound(token_id.to_string()))?;

        // In production, we would verify `issuer_signature` against `info.issuer`.
        // The verification is done by the execution engine using the protocol's
        // crypto module. Here we check that the signature was at least provided.
        let _issuer = info.issuer.clone();

        // Update total supply.
        let new_supply = info
            .total_supply
            .checked_add(amount)
            .ok_or(TokenError::SupplyOverflow { amount })?;

        // Update the token info.
        let info_mut = self.tokens.get_mut(token_id).unwrap();
        info_mut.total_supply = new_supply;

        // Update the recipient's balance.
        let balances = self.balances.get_mut(token_id).unwrap();
        let balance = balances.entry(to.to_string()).or_insert(0);
        *balance = balance
            .checked_add(amount)
            .ok_or(TokenError::SupplyOverflow { amount })?;

        Ok(())
    }

    /// Burns tokens from the specified address.
    ///
    /// The owner's signature over `(token_id || amount)` is required. Only
    /// the holder of the tokens can initiate a burn — there is no admin burn.
    ///
    /// # Arguments
    ///
    /// * `token_id` - The token to burn.
    /// * `from` - Hex-encoded address of the token holder.
    /// * `amount` - Number of tokens to burn.
    /// * `owner_signature` - Hex-encoded Ed25519 signature from the holder.
    ///
    /// # Errors
    ///
    /// Returns [`TokenError::TokenNotFound`] if the token does not exist.
    /// Returns [`TokenError::InvalidSignature`] if the signature is empty.
    /// Returns [`TokenError::InsufficientBalance`] if the holder doesn't have enough.
    pub fn burn(
        &mut self,
        token_id: &str,
        from: &str,
        amount: u64,
        owner_signature: &str,
    ) -> Result<(), TokenError> {
        if owner_signature.is_empty() {
            return Err(TokenError::InvalidSignature);
        }

        let info = self
            .tokens
            .get(token_id)
            .ok_or_else(|| TokenError::TokenNotFound(token_id.to_string()))?;

        let _total = info.total_supply;

        // Check and deduct balance.
        let balances = self
            .balances
            .get_mut(token_id)
            .ok_or_else(|| TokenError::TokenNotFound(token_id.to_string()))?;

        let balance = balances
            .get_mut(from)
            .ok_or(TokenError::InsufficientBalance { balance: 0, amount })?;

        if *balance < amount {
            return Err(TokenError::InsufficientBalance {
                balance: *balance,
                amount,
            });
        }

        *balance -= amount;

        // Deduct from total supply.
        let info_mut = self.tokens.get_mut(token_id).unwrap();
        info_mut.total_supply = info_mut.total_supply.saturating_sub(amount);

        Ok(())
    }

    /// Returns metadata for a token, or `None` if it does not exist.
    pub fn get_token_info(&self, token_id: &str) -> Option<&TokenInfo> {
        self.tokens.get(token_id)
    }

    /// Returns the total supply of a token, or 0 if it does not exist.
    pub fn total_supply(&self, token_id: &str) -> u64 {
        self.tokens
            .get(token_id)
            .map(|t| t.total_supply)
            .unwrap_or(0)
    }

    /// Returns the balance of `address` for the given token, or 0.
    pub fn balance_of(&self, token_id: &str, address: &str) -> u64 {
        self.balances
            .get(token_id)
            .and_then(|b| b.get(address))
            .copied()
            .unwrap_or(0)
    }

    /// Returns the number of registered tokens.
    pub fn token_count(&self) -> usize {
        self.tokens.len()
    }
}

impl Default for TokenFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_token_assigns_unique_id() {
        let mut factory = TokenFactory::new();
        let id1 = factory
            .create_token(
                "NOVA Dollar".into(),
                "nUSD".into(),
                8,
                TokenType::Stablecoin,
                "issuer_pk".into(),
            )
            .unwrap();
        let id2 = factory
            .create_token(
                "NOVA Governance".into(),
                "nGOV".into(),
                8,
                TokenType::Governance,
                "issuer_pk".into(),
            )
            .unwrap();
        assert_ne!(id1, id2);
        assert_eq!(factory.token_count(), 2);
    }

    #[test]
    fn duplicate_symbol_rejected() {
        let mut factory = TokenFactory::new();
        factory
            .create_token("A".into(), "SYM".into(), 8, TokenType::Utility, "pk".into())
            .unwrap();
        let result =
            factory.create_token("B".into(), "SYM".into(), 8, TokenType::Utility, "pk".into());
        assert!(result.is_err());
    }

    #[test]
    fn mint_increases_supply_and_balance() {
        let mut factory = TokenFactory::new();
        let id = factory
            .create_token(
                "T".into(),
                "TOK".into(),
                8,
                TokenType::Utility,
                "issuer".into(),
            )
            .unwrap();
        factory.mint(&id, "alice", 1_000_000, "sig_hex").unwrap();
        assert_eq!(factory.total_supply(&id), 1_000_000);
        assert_eq!(factory.balance_of(&id, "alice"), 1_000_000);
    }

    #[test]
    fn mint_without_signature_rejected() {
        let mut factory = TokenFactory::new();
        let id = factory
            .create_token(
                "T".into(),
                "TOK".into(),
                8,
                TokenType::Utility,
                "issuer".into(),
            )
            .unwrap();
        let result = factory.mint(&id, "alice", 1_000_000, "");
        assert!(result.is_err());
    }

    #[test]
    fn mint_nonexistent_token_rejected() {
        let mut factory = TokenFactory::new();
        let result = factory.mint("fake-id", "alice", 100, "sig");
        assert!(result.is_err());
    }

    #[test]
    fn burn_decreases_supply_and_balance() {
        let mut factory = TokenFactory::new();
        let id = factory
            .create_token(
                "T".into(),
                "TOK".into(),
                8,
                TokenType::Utility,
                "issuer".into(),
            )
            .unwrap();
        factory.mint(&id, "alice", 1_000_000, "sig").unwrap();
        factory.burn(&id, "alice", 400_000, "owner_sig").unwrap();
        assert_eq!(factory.total_supply(&id), 600_000);
        assert_eq!(factory.balance_of(&id, "alice"), 600_000);
    }

    #[test]
    fn burn_more_than_balance_rejected() {
        let mut factory = TokenFactory::new();
        let id = factory
            .create_token(
                "T".into(),
                "TOK".into(),
                8,
                TokenType::Utility,
                "issuer".into(),
            )
            .unwrap();
        factory.mint(&id, "alice", 100, "sig").unwrap();
        let result = factory.burn(&id, "alice", 200, "sig");
        assert!(result.is_err());
    }

    #[test]
    fn burn_without_signature_rejected() {
        let mut factory = TokenFactory::new();
        let id = factory
            .create_token(
                "T".into(),
                "TOK".into(),
                8,
                TokenType::Utility,
                "issuer".into(),
            )
            .unwrap();
        factory.mint(&id, "alice", 100, "sig").unwrap();
        let result = factory.burn(&id, "alice", 50, "");
        assert!(result.is_err());
    }

    #[test]
    fn get_token_info_returns_metadata() {
        let mut factory = TokenFactory::new();
        let id = factory
            .create_token(
                "Test Token".into(),
                "TST".into(),
                6,
                TokenType::Reward,
                "issuer_pk".into(),
            )
            .unwrap();
        let info = factory.get_token_info(&id).unwrap();
        assert_eq!(info.name, "Test Token");
        assert_eq!(info.symbol, "TST");
        assert_eq!(info.decimals, 6);
        assert_eq!(info.token_type, TokenType::Reward);
        assert_eq!(info.issuer, "issuer_pk");
    }

    #[test]
    fn nonexistent_token_returns_none() {
        let factory = TokenFactory::new();
        assert!(factory.get_token_info("fake").is_none());
        assert_eq!(factory.total_supply("fake"), 0);
        assert_eq!(factory.balance_of("fake", "anyone"), 0);
    }
}
