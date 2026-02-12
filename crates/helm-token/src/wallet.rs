//! Wallet — address, balance management, and transfer with double-spend prevention.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::token::{TokenAmount, TokenError};

/// Hex-encoded ed25519 public key (64 hex chars = 32 bytes).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address(pub String);

impl Address {
    /// Create an address from raw 32-byte public key.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
        Self(hex)
    }

    /// Generate a new keypair and return (address, secret_key_bytes).
    /// The secret key should be stored securely and NEVER transmitted.
    pub fn generate() -> (Self, [u8; 32]) {
        use ed25519_dalek::SigningKey;
        use rand::rngs::OsRng;

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let address = Self::from_bytes(verifying_key.as_bytes());
        let secret = signing_key.to_bytes();
        (address, secret)
    }

    /// Create from a hex string (must be 64 hex chars).
    pub fn from_hex(hex: &str) -> Result<Self, TokenError> {
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(TokenError::InvalidAmount(
                "address must be 64 hex characters".into(),
            ));
        }
        Ok(Self(hex.to_lowercase()))
    }

    pub fn as_hex(&self) -> &str {
        &self.0
    }

    /// The genesis address (all zeros).
    pub fn genesis() -> Self {
        Self("0".repeat(64))
    }

    pub fn is_genesis(&self) -> bool {
        self.0.chars().all(|c| c == '0')
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Show abbreviated: first 8 + last 8 chars
        if self.0.len() == 64 {
            write!(f, "{}...{}", &self.0[..8], &self.0[56..])
        } else {
            write!(f, "{}", self.0)
        }
    }
}

/// A single transaction record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub nonce: u64,
    pub from: Address,
    pub to: Address,
    pub amount: TokenAmount,
    pub timestamp: u64,
    pub memo: String,
}

/// Individual wallet state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub address: Address,
    pub balance: TokenAmount,
    pub nonce: u64,
    pub created_at: u64,
}

impl Wallet {
    pub fn new(address: Address, timestamp: u64) -> Self {
        Self {
            address,
            balance: TokenAmount::ZERO,
            nonce: 0,
            created_at: timestamp,
        }
    }
}

/// Wallet store — manages all wallets and processes transfers.
#[derive(Debug)]
pub struct WalletStore {
    wallets: HashMap<Address, Wallet>,
    history: Vec<Transaction>,
    current_time: u64,
}

impl WalletStore {
    pub fn new() -> Self {
        Self {
            wallets: HashMap::new(),
            history: Vec::new(),
            current_time: 0,
        }
    }

    pub fn set_time(&mut self, t: u64) {
        self.current_time = t;
    }

    /// Get or create a wallet for an address.
    pub fn get_or_create(&mut self, address: &Address) -> &Wallet {
        if !self.wallets.contains_key(address) {
            self.wallets.insert(
                address.clone(),
                Wallet::new(address.clone(), self.current_time),
            );
        }
        &self.wallets[address]
    }

    pub fn get(&self, address: &Address) -> Option<&Wallet> {
        self.wallets.get(address)
    }

    pub fn balance(&self, address: &Address) -> TokenAmount {
        self.wallets
            .get(address)
            .map(|w| w.balance)
            .unwrap_or(TokenAmount::ZERO)
    }

    /// Deposit tokens into a wallet (mint/reward — no sender nonce needed).
    pub fn deposit(
        &mut self,
        to: &Address,
        amount: TokenAmount,
        memo: &str,
    ) -> Result<(), TokenError> {
        if amount.is_zero() {
            return Err(TokenError::InvalidAmount("zero deposit".into()));
        }

        let wallet = self
            .wallets
            .entry(to.clone())
            .or_insert_with(|| Wallet::new(to.clone(), self.current_time));

        wallet.balance = wallet.balance.checked_add(amount)?;

        self.history.push(Transaction {
            nonce: 0,
            from: Address::genesis(),
            to: to.clone(),
            amount,
            timestamp: self.current_time,
            memo: memo.to_string(),
        });

        Ok(())
    }

    /// Transfer tokens between wallets with nonce-based double-spend prevention.
    pub fn transfer(
        &mut self,
        from: &Address,
        to: &Address,
        amount: TokenAmount,
        nonce: u64,
        memo: &str,
    ) -> Result<(), TokenError> {
        if amount.is_zero() {
            return Err(TokenError::InvalidAmount("zero transfer".into()));
        }

        if from == to {
            return Err(TokenError::InvalidAmount("self-transfer".into()));
        }

        // Validate nonce (must be exactly current nonce)
        let sender = self
            .wallets
            .get(from)
            .ok_or_else(|| TokenError::NotFound(format!("wallet {}", from)))?;

        if nonce != sender.nonce {
            return Err(TokenError::InvalidNonce {
                expected: sender.nonce,
                got: nonce,
            });
        }

        // Check balance
        if sender.balance.base_units() < amount.base_units() {
            return Err(TokenError::InsufficientBalance {
                have: sender.balance.base_units(),
                need: amount.base_units(),
            });
        }

        // Execute transfer
        let sender = self.wallets.get_mut(from).unwrap();
        sender.balance = sender.balance.checked_sub(amount)?;
        sender.nonce += 1;

        let receiver = self
            .wallets
            .entry(to.clone())
            .or_insert_with(|| Wallet::new(to.clone(), self.current_time));
        receiver.balance = receiver.balance.checked_add(amount)?;

        self.history.push(Transaction {
            nonce,
            from: from.clone(),
            to: to.clone(),
            amount,
            timestamp: self.current_time,
            memo: memo.to_string(),
        });

        Ok(())
    }

    /// Withdraw tokens (burn — send to genesis address).
    pub fn withdraw(
        &mut self,
        from: &Address,
        amount: TokenAmount,
        nonce: u64,
    ) -> Result<(), TokenError> {
        self.transfer(from, &Address::genesis(), amount, nonce, "withdraw")
    }

    /// Transaction history for an address.
    pub fn history(&self, address: &Address) -> Vec<&Transaction> {
        self.history
            .iter()
            .filter(|tx| tx.from == *address || tx.to == *address)
            .collect()
    }

    /// All wallets.
    pub fn all_wallets(&self) -> Vec<&Wallet> {
        self.wallets.values().collect()
    }

    /// Number of wallets.
    pub fn wallet_count(&self) -> usize {
        self.wallets.len()
    }

    /// Total circulating supply across all wallets.
    pub fn circulating_supply(&self) -> TokenAmount {
        self.wallets
            .values()
            .fold(TokenAmount::ZERO, |acc, w| TokenAmount(acc.0 + w.balance.0))
    }
}

impl Default for WalletStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> Address {
        // Pad to 64 hex chars for testing
        let padded = format!("{:0>64}", s);
        Address(padded)
    }

    #[test]
    fn address_from_bytes() {
        let bytes = [0xab; 32];
        let address = Address::from_bytes(&bytes);
        assert_eq!(address.0.len(), 64);
        assert!(address.0.starts_with("abab"));
    }

    #[test]
    fn address_display_abbreviated() {
        let address = addr("aabbccdd11223344");
        let display = format!("{}", address);
        assert!(display.contains("..."));
    }

    #[test]
    fn genesis_address() {
        let genesis = Address::genesis();
        assert!(genesis.is_genesis());
        assert_eq!(genesis.0.len(), 64);

        let non_genesis = addr("ff");
        assert!(!non_genesis.is_genesis());
    }

    #[test]
    fn address_from_hex_valid() {
        let hex = "a".repeat(64);
        let addr = Address::from_hex(&hex).unwrap();
        assert_eq!(addr.0, hex);
    }

    #[test]
    fn address_from_hex_invalid_length() {
        assert!(Address::from_hex("abc").is_err());
    }

    #[test]
    fn wallet_creation() {
        let w = Wallet::new(addr("aa"), 1000);
        assert_eq!(w.balance, TokenAmount::ZERO);
        assert_eq!(w.nonce, 0);
        assert_eq!(w.created_at, 1000);
    }

    #[test]
    fn deposit_increases_balance() {
        let mut store = WalletStore::new();
        let alice = addr("aa");
        store
            .deposit(&alice, TokenAmount::from_tokens(100), "initial")
            .unwrap();
        assert_eq!(store.balance(&alice).whole_tokens(), 100);
    }

    #[test]
    fn deposit_zero_fails() {
        let mut store = WalletStore::new();
        let alice = addr("aa");
        assert!(store
            .deposit(&alice, TokenAmount::ZERO, "zero")
            .is_err());
    }

    #[test]
    fn transfer_with_correct_nonce() {
        let mut store = WalletStore::new();
        let alice = addr("aa");
        let bob = addr("bb");

        store
            .deposit(&alice, TokenAmount::from_tokens(100), "init")
            .unwrap();
        store
            .transfer(&alice, &bob, TokenAmount::from_tokens(30), 0, "payment")
            .unwrap();

        assert_eq!(store.balance(&alice).whole_tokens(), 70);
        assert_eq!(store.balance(&bob).whole_tokens(), 30);
    }

    #[test]
    fn transfer_wrong_nonce_fails() {
        let mut store = WalletStore::new();
        let alice = addr("aa");
        let bob = addr("bb");

        store
            .deposit(&alice, TokenAmount::from_tokens(100), "init")
            .unwrap();

        // Nonce 5 is wrong (should be 0)
        assert!(store
            .transfer(&alice, &bob, TokenAmount::from_tokens(10), 5, "bad")
            .is_err());
    }

    #[test]
    fn transfer_insufficient_balance() {
        let mut store = WalletStore::new();
        let alice = addr("aa");
        let bob = addr("bb");

        store
            .deposit(&alice, TokenAmount::from_tokens(10), "init")
            .unwrap();

        assert!(store
            .transfer(&alice, &bob, TokenAmount::from_tokens(50), 0, "overdraw")
            .is_err());
    }

    #[test]
    fn self_transfer_fails() {
        let mut store = WalletStore::new();
        let alice = addr("aa");

        store
            .deposit(&alice, TokenAmount::from_tokens(100), "init")
            .unwrap();

        assert!(store
            .transfer(&alice, &alice, TokenAmount::from_tokens(10), 0, "self")
            .is_err());
    }

    #[test]
    fn nonce_increments_on_transfer() {
        let mut store = WalletStore::new();
        let alice = addr("aa");
        let bob = addr("bb");

        store
            .deposit(&alice, TokenAmount::from_tokens(100), "init")
            .unwrap();

        store
            .transfer(&alice, &bob, TokenAmount::from_tokens(10), 0, "tx1")
            .unwrap();
        store
            .transfer(&alice, &bob, TokenAmount::from_tokens(10), 1, "tx2")
            .unwrap();
        store
            .transfer(&alice, &bob, TokenAmount::from_tokens(10), 2, "tx3")
            .unwrap();

        assert_eq!(store.get(&alice).unwrap().nonce, 3);
        assert_eq!(store.balance(&alice).whole_tokens(), 70);
    }

    #[test]
    fn transaction_history() {
        let mut store = WalletStore::new();
        let alice = addr("aa");
        let bob = addr("bb");

        store
            .deposit(&alice, TokenAmount::from_tokens(100), "init")
            .unwrap();
        store
            .transfer(&alice, &bob, TokenAmount::from_tokens(10), 0, "pay")
            .unwrap();

        let alice_history = store.history(&alice);
        assert_eq!(alice_history.len(), 2); // deposit + transfer

        let bob_history = store.history(&bob);
        assert_eq!(bob_history.len(), 1); // received transfer
    }

    #[test]
    fn circulating_supply() {
        let mut store = WalletStore::new();
        let alice = addr("aa");
        let bob = addr("bb");

        store
            .deposit(&alice, TokenAmount::from_tokens(100), "a")
            .unwrap();
        store
            .deposit(&bob, TokenAmount::from_tokens(200), "b")
            .unwrap();

        // genesis address also gets counted, but no withdrawals so it's 0
        assert!(store.circulating_supply().whole_tokens() >= 300);
    }

    #[test]
    fn wallet_store_default() {
        let store = WalletStore::default();
        assert_eq!(store.wallet_count(), 0);
    }

    #[test]
    fn address_generate_produces_valid_key() {
        let (addr, secret) = Address::generate();
        assert_eq!(addr.0.len(), 64);
        assert_ne!(secret, [0u8; 32]);
    }
}
