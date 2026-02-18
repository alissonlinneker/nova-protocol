//! # Token Standard
//!
//! Defines the token abstraction for the NOVA protocol. Every asset that
//! can be held in a vault -- fiat-backed stablecoins, wrapped crypto,
//! loyalty points, tokenized real-world assets -- is represented as a
//! [`TokenInfo`] with a unique [`TokenId`].
//!
//! Token IDs are deterministic BLAKE3 hashes of the token's canonical
//! properties (name, symbol, token type, issuer). This means the same
//! token always gets the same ID regardless of when or where it's
//! registered -- no registry needed, no coordination required.
//!
//! ## Pre-defined Tokens
//!
//! The protocol ships with a set of well-known token constants for the
//! assets we expect to see on day one: `nova_brl()`, `nova_usd()`,
//! `nova_btc()`, etc.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::crypto::hash::blake3_hash;
use crate::transaction::types::Currency;

// ---------------------------------------------------------------------------
// TokenId
// ---------------------------------------------------------------------------

/// A unique, content-addressed identifier for a token type.
///
/// Computed as `BLAKE3(name || symbol || token_type_tag || issuer_address)`.
/// Two tokens with identical properties will always produce the same ID,
/// making this a natural deduplication key across the network.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TokenId([u8; 32]);

impl TokenId {
    /// Creates a `TokenId` from raw 32-byte hash.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the raw 32-byte identifier.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the hex-encoded token ID.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parses a hex-encoded token ID.
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(s)?;
        if bytes.len() != 32 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Derives a `TokenId` from the canonical token properties.
    ///
    /// The hash input is the concatenation of:
    /// - `name` (UTF-8 bytes)
    /// - `0x00` separator
    /// - `symbol` (UTF-8 bytes)
    /// - `0x00` separator
    /// - `token_type_tag` (single byte discriminant)
    /// - `0x00` separator
    /// - `issuer` (UTF-8 bytes of the issuer address)
    ///
    /// The separator bytes prevent ambiguity when one field's suffix
    /// matches another field's prefix.
    pub fn derive(name: &str, symbol: &str, token_type: &TokenType, issuer: &str) -> Self {
        let mut preimage = Vec::with_capacity(name.len() + symbol.len() + issuer.len() + 8);
        preimage.extend_from_slice(name.as_bytes());
        preimage.push(0x00);
        preimage.extend_from_slice(symbol.as_bytes());
        preimage.push(0x00);
        preimage.push(token_type.discriminant());
        preimage.push(0x00);
        preimage.extend_from_slice(issuer.as_bytes());

        Self(blake3_hash(&preimage))
    }
}

impl fmt::Debug for TokenId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TokenId({}...)", &self.to_hex()[..12])
    }
}

impl fmt::Display for TokenId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl std::str::FromStr for TokenId {
    type Err = hex::FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}

// ---------------------------------------------------------------------------
// Serde helper: serialize HashMap<TokenId, V> with hex-string keys
// ---------------------------------------------------------------------------

/// Serde helper module for serializing/deserializing `HashMap<TokenId, V>`
/// as a JSON object with hex-encoded string keys.
///
/// JSON requires map keys to be strings, but `TokenId` wraps `[u8; 32]`
/// which serde would serialize as an array. This module converts keys
/// to/from their hex representation so the map serializes correctly.
///
/// # Usage
///
/// ```ignore
/// #[derive(Serialize, Deserialize)]
/// struct MyStruct {
///     #[serde(with = "crate::vault::token::token_id_map")]
///     balances: HashMap<TokenId, SomeValue>,
/// }
/// ```
pub mod token_id_map {
    use super::TokenId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;

    pub fn serialize<V, S>(map: &HashMap<TokenId, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        V: Serialize,
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        let mut ser_map = serializer.serialize_map(Some(map.len()))?;
        for (key, value) in map {
            ser_map.serialize_entry(&key.to_hex(), value)?;
        }
        ser_map.end()
    }

    pub fn deserialize<'de, V, D>(deserializer: D) -> Result<HashMap<TokenId, V>, D::Error>
    where
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        let string_map: HashMap<String, V> = HashMap::deserialize(deserializer)?;
        string_map
            .into_iter()
            .map(|(key, value)| {
                TokenId::from_hex(&key)
                    .map(|id| (id, value))
                    .map_err(serde::de::Error::custom)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// TokenType
// ---------------------------------------------------------------------------

/// Classification of a token by its backing model.
///
/// This affects how the token is treated by the settlement engine: fiat-backed
/// tokens require proof of reserves, crypto wrappers need bridge attestations,
/// and native tokens are governed by protocol consensus.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TokenType {
    /// Fiat-backed token, potentially CBDC-compatible.
    /// The inner [`Currency`] specifies which fiat currency backs it.
    FiatBacked(Currency),

    /// Wrapped cryptocurrency from an external chain.
    /// The inner [`Currency`] identifies the underlying asset (BTC, ETH, etc.).
    Crypto(Currency),

    /// Dollar-pegged stablecoin (USDC, USDT, etc.).
    /// The inner [`Currency`] identifies the specific stablecoin.
    Stablecoin(Currency),

    /// The native NOVA protocol token.
    /// Used for fees, staking, governance, and as the unit of account
    /// for credit scoring.
    Native,

    /// Loyalty or reward points issued by a merchant or platform.
    LoyaltyPoints,

    /// Tokenized real-world asset: real estate deeds, equity shares,
    /// commodity receipts, etc.
    /// The `String` is a free-form asset class descriptor (e.g.,
    /// "real_estate", "equity", "commodity").
    Asset(String),
}

impl TokenType {
    /// Returns a single-byte discriminant for use in hash derivation.
    ///
    /// These values are part of the wire format and must never change
    /// once mainnet launches. Adding new variants is fine -- just append
    /// new discriminant values.
    pub fn discriminant(&self) -> u8 {
        match self {
            TokenType::FiatBacked(_) => 0x01,
            TokenType::Crypto(_) => 0x02,
            TokenType::Stablecoin(_) => 0x03,
            TokenType::Native => 0x04,
            TokenType::LoyaltyPoints => 0x05,
            TokenType::Asset(_) => 0x06,
        }
    }
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenType::FiatBacked(c) => write!(f, "FiatBacked({})", c),
            TokenType::Crypto(c) => write!(f, "Crypto({})", c),
            TokenType::Stablecoin(c) => write!(f, "Stablecoin({})", c),
            TokenType::Native => write!(f, "Native"),
            TokenType::LoyaltyPoints => write!(f, "LoyaltyPoints"),
            TokenType::Asset(class) => write!(f, "Asset({})", class),
        }
    }
}

// ---------------------------------------------------------------------------
// TokenInfo
// ---------------------------------------------------------------------------

/// Complete metadata for a registered token.
///
/// This is the canonical record for a token type on the NOVA network.
/// It lives in the global token registry (maintained by validators) and
/// is referenced by [`TokenId`] everywhere else.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenInfo {
    /// Content-addressed identifier derived from this token's properties.
    pub id: TokenId,

    /// Human-readable token name (e.g., "NOVA Brazilian Real").
    pub name: String,

    /// Trading symbol / ticker (e.g., "nBRL").
    pub symbol: String,

    /// Number of decimal places for display purposes.
    ///
    /// A token with `decimals = 2` and raw amount `12345` displays as
    /// `123.45`. The protocol never performs division -- this is purely
    /// for UI rendering.
    pub decimals: u8,

    /// Current total supply in smallest units.
    ///
    /// Updated on mint/burn operations. For fiat-backed tokens, this
    /// should match the proof-of-reserves attestation (enforced off-chain).
    pub total_supply: u64,

    /// The NOVA address of the entity authorized to mint/burn this token.
    ///
    /// For native NOVA, this is the protocol itself (represented as a
    /// well-known system address). For fiat-backed tokens, this is the
    /// licensed issuer's address.
    pub issuer: String,

    /// The backing model for this token.
    pub token_type: TokenType,
}

// ---------------------------------------------------------------------------
// Token Factory
// ---------------------------------------------------------------------------

/// Factory for creating [`TokenInfo`] instances with derived IDs.
///
/// This is the only correct way to create a token -- it ensures the ID
/// is always consistent with the token's properties.
pub struct Token;

impl Token {
    /// Creates a new [`TokenInfo`] with a deterministically derived [`TokenId`].
    ///
    /// The `total_supply` is initialized to `0`. Use mint operations to
    /// increase supply after creation.
    ///
    /// # Arguments
    ///
    /// * `name` -- Human-readable name (e.g., "NOVA Brazilian Real")
    /// * `symbol` -- Ticker symbol (e.g., "nBRL")
    /// * `decimals` -- Display decimal places (e.g., 2 for BRL, 8 for BTC)
    /// * `token_type` -- Backing model classification
    /// * `issuer` -- NOVA address of the authorized issuer
    pub fn new(
        name: &str,
        symbol: &str,
        decimals: u8,
        token_type: TokenType,
        issuer: &str,
    ) -> TokenInfo {
        let id = TokenId::derive(name, symbol, &token_type, issuer);

        TokenInfo {
            id,
            name: name.to_string(),
            symbol: symbol.to_string(),
            decimals,
            total_supply: 0,
            issuer: issuer.to_string(),
            token_type,
        }
    }

    /// Creates a new [`TokenInfo`] with an explicit initial supply.
    ///
    /// Use this for tokens that are pre-minted at genesis (e.g., the
    /// native NOVA token).
    pub fn new_with_supply(
        name: &str,
        symbol: &str,
        decimals: u8,
        token_type: TokenType,
        issuer: &str,
        initial_supply: u64,
    ) -> TokenInfo {
        let mut info = Self::new(name, symbol, decimals, token_type, issuer);
        info.total_supply = initial_supply;
        info
    }
}

// ---------------------------------------------------------------------------
// Pre-defined Token Constants
// ---------------------------------------------------------------------------

/// System issuer address used for protocol-level tokens.
/// This address is not backed by a real keypair -- tokens issued by this
/// address are created by validator consensus, not by any single entity.
const SYSTEM_ISSUER: &str = "nova:0000000000000000000000000000000000000000000000000000000000000000";

/// NOVA Brazilian Real -- fiat-backed BRL token for the Brazilian market.
///
/// 2 decimal places (centavos). This is the workhorse token for NOVA's
/// primary market: Pix-compatible payments, merchant settlement, and
/// cross-border remittances to/from Brazil.
pub fn nova_brl() -> TokenInfo {
    Token::new(
        "NOVA Brazilian Real",
        "nBRL",
        2,
        TokenType::FiatBacked(Currency::BRL),
        SYSTEM_ISSUER,
    )
}

/// NOVA US Dollar -- fiat-backed USD token.
///
/// 2 decimal places (cents). Used for USD-denominated payments and as a
/// settlement currency for international transactions.
pub fn nova_usd() -> TokenInfo {
    Token::new(
        "NOVA US Dollar",
        "nUSD",
        2,
        TokenType::FiatBacked(Currency::USD),
        SYSTEM_ISSUER,
    )
}

/// NOVA Euro -- fiat-backed EUR token.
///
/// 2 decimal places (euro cents).
pub fn nova_eur() -> TokenInfo {
    Token::new(
        "NOVA Euro",
        "nEUR",
        2,
        TokenType::FiatBacked(Currency::EUR),
        SYSTEM_ISSUER,
    )
}

/// NOVA-wrapped Bitcoin.
///
/// 8 decimal places (satoshis). Bridged from the Bitcoin network via
/// a federated peg or trustless bridge (implementation TBD).
pub fn nova_btc() -> TokenInfo {
    Token::new(
        "NOVA Bitcoin",
        "nBTC",
        8,
        TokenType::Crypto(Currency::BTC),
        SYSTEM_ISSUER,
    )
}

/// NOVA-wrapped Ether.
///
/// 18 decimal places (wei). Bridged from Ethereum.
pub fn nova_eth() -> TokenInfo {
    Token::new(
        "NOVA Ether",
        "nETH",
        18,
        TokenType::Crypto(Currency::ETH),
        SYSTEM_ISSUER,
    )
}

/// NOVA-wrapped USDC stablecoin.
///
/// 6 decimal places (matching native USDC on Ethereum/Solana).
pub fn nova_usdc() -> TokenInfo {
    Token::new(
        "NOVA USDC",
        "nUSDC",
        6,
        TokenType::Stablecoin(Currency::USDC),
        SYSTEM_ISSUER,
    )
}

/// NOVA-wrapped USDT stablecoin.
///
/// 6 decimal places.
pub fn nova_usdt() -> TokenInfo {
    Token::new(
        "NOVA USDT",
        "nUSDT",
        6,
        TokenType::Stablecoin(Currency::Custom("USDT".to_string())),
        SYSTEM_ISSUER,
    )
}

/// The native NOVA protocol token.
///
/// 8 decimal places (photons). Used for transaction fees, validator
/// staking, and governance voting. The smallest unit -- one photon --
/// is 10^-8 NOVA.
pub fn nova_native() -> TokenInfo {
    Token::new_with_supply(
        "NOVA",
        "NOVA",
        8,
        TokenType::Native,
        SYSTEM_ISSUER,
        // Genesis supply: 1 billion NOVA = 10^17 photons.
        100_000_000_000_000_000,
    )
}

// ---------------------------------------------------------------------------
// Convenience: TokenId constants for pre-defined tokens
// ---------------------------------------------------------------------------

/// Returns the [`TokenId`] for the NOVA native token.
pub fn native_token_id() -> TokenId {
    nova_native().id
}

/// Returns the [`TokenId`] for NOVA BRL.
pub fn brl_token_id() -> TokenId {
    nova_brl().id
}

/// Returns the [`TokenId`] for NOVA USD.
pub fn usd_token_id() -> TokenId {
    nova_usd().id
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_id_derivation_is_deterministic() {
        let id1 = TokenId::derive("Test", "TST", &TokenType::Native, "nova:issuer");
        let id2 = TokenId::derive("Test", "TST", &TokenType::Native, "nova:issuer");
        assert_eq!(id1, id2);
    }

    #[test]
    fn different_names_produce_different_ids() {
        let id1 = TokenId::derive("Token A", "A", &TokenType::Native, "nova:issuer");
        let id2 = TokenId::derive("Token B", "B", &TokenType::Native, "nova:issuer");
        assert_ne!(id1, id2);
    }

    #[test]
    fn different_types_produce_different_ids() {
        let id1 = TokenId::derive(
            "Dollar",
            "USD",
            &TokenType::FiatBacked(Currency::USD),
            "nova:issuer",
        );
        let id2 = TokenId::derive(
            "Dollar",
            "USD",
            &TokenType::Stablecoin(Currency::USD),
            "nova:issuer",
        );
        assert_ne!(id1, id2);
    }

    #[test]
    fn different_issuers_produce_different_ids() {
        let id1 = TokenId::derive("Token", "TKN", &TokenType::Native, "nova:alice");
        let id2 = TokenId::derive("Token", "TKN", &TokenType::Native, "nova:bob");
        assert_ne!(id1, id2);
    }

    #[test]
    fn token_id_hex_roundtrip() {
        let id = TokenId::derive("Test", "TST", &TokenType::Native, "nova:issuer");
        let hex_str = id.to_hex();
        let recovered = TokenId::from_hex(&hex_str).unwrap();
        assert_eq!(id, recovered);
    }

    #[test]
    fn token_factory_sets_zero_supply() {
        let token = Token::new("Test Token", "TST", 8, TokenType::Native, "nova:issuer");
        assert_eq!(token.total_supply, 0);
        assert_eq!(token.symbol, "TST");
        assert_eq!(token.decimals, 8);
    }

    #[test]
    fn token_factory_with_supply() {
        let token = Token::new_with_supply(
            "Test Token",
            "TST",
            8,
            TokenType::Native,
            "nova:issuer",
            1_000_000,
        );
        assert_eq!(token.total_supply, 1_000_000);
    }

    #[test]
    fn predefined_brl_token_properties() {
        let brl = nova_brl();
        assert_eq!(brl.symbol, "nBRL");
        assert_eq!(brl.decimals, 2);
        assert_eq!(brl.token_type, TokenType::FiatBacked(Currency::BRL));
        assert_eq!(brl.total_supply, 0);
    }

    #[test]
    fn predefined_native_token_has_genesis_supply() {
        let native = nova_native();
        assert_eq!(native.symbol, "NOVA");
        assert_eq!(native.decimals, 8);
        assert!(
            native.total_supply > 0,
            "native token must have genesis supply"
        );
    }

    #[test]
    fn predefined_token_ids_are_stable() {
        let id1 = native_token_id();
        let id2 = native_token_id();
        assert_eq!(id1, id2);

        let brl1 = brl_token_id();
        let brl2 = brl_token_id();
        assert_eq!(brl1, brl2);

        assert_ne!(native_token_id(), brl_token_id());
    }

    #[test]
    fn token_type_discriminants_are_unique() {
        let types: Vec<TokenType> = vec![
            TokenType::FiatBacked(Currency::BRL),
            TokenType::Crypto(Currency::BTC),
            TokenType::Stablecoin(Currency::USDC),
            TokenType::Native,
            TokenType::LoyaltyPoints,
            TokenType::Asset("equity".into()),
        ];
        let discriminants: Vec<u8> = types.iter().map(|t| t.discriminant()).collect();
        let mut deduped = discriminants.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(
            discriminants.len(),
            deduped.len(),
            "token type discriminants must be unique"
        );
    }

    #[test]
    fn token_info_serialization_roundtrip() {
        let token = nova_brl();
        let json = serde_json::to_string(&token).expect("serialize");
        let recovered: TokenInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(token, recovered);
    }
}
