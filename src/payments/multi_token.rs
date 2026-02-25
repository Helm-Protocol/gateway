// src/payments/multi_token.rs
// Multi-Token Support: BNKR, ETH, USDC, USDT, SOL, CLANKER, VIRTUAL

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Token {
    Bnkr, Eth, Usdc, Usdt, Sol, Clanker, Virtual,
}

impl Token {
    pub fn symbol(&self) -> &'static str {
        match self {
            Token::Bnkr => "BNKR", Token::Eth => "ETH", Token::Usdc => "USDC",
            Token::Usdt => "USDT", Token::Sol => "SOL", Token::Clanker => "CLANKER",
            Token::Virtual => "VIRTUAL",
        }
    }
    pub fn chain(&self) -> &'static str {
        match self {
            Token::Sol => "solana",
            _          => "base",
        }
    }
    pub fn contract_address(&self) -> Option<&'static str> {
        match self {
            Token::Bnkr    => Some("0x22af33fe49fd1fa80c7149773dde5890d3c76f3b"),
            Token::Eth     => None,
            Token::Usdc    => Some("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
            Token::Usdt    => Some("0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2"),
            Token::Sol     => None,
            Token::Clanker => Some("0x1D008f50FB828eF9DebbBEae1b71feE300B6d376"),
            Token::Virtual => Some("0x0b3e328455c4059EEb9e3f84b5543F74E24e7E1b"),
        }
    }
    pub fn decimals(&self) -> u8 {
        match self { Token::Usdc | Token::Usdt => 6, Token::Sol => 9, _ => 18 }
    }
    pub fn is_stable(&self) -> bool {
        matches!(self, Token::Usdc | Token::Usdt)
    }
    pub fn all() -> Vec<Token> {
        vec![Token::Bnkr, Token::Eth, Token::Usdc, Token::Usdt,
             Token::Sol, Token::Clanker, Token::Virtual]
    }
    pub fn fallback_usd_price(&self) -> f64 {
        match self {
            Token::Bnkr    => 0.001,
            Token::Eth     => 3500.0,
            Token::Usdc    => 1.0,
            Token::Usdt    => 1.0,
            Token::Sol     => 180.0,
            Token::Clanker => 0.05,
            Token::Virtual => 2.5,
        }
    }
}

impl std::str::FromStr for Token {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "BNKR"    => Ok(Token::Bnkr),
            "ETH"     => Ok(Token::Eth),
            "USDC"    => Ok(Token::Usdc),
            "USDT"    => Ok(Token::Usdt),
            "SOL"     => Ok(Token::Sol),
            "CLANKER" => Ok(Token::Clanker),
            "VIRTUAL" => Ok(Token::Virtual),
            other     => Err(format!("Unknown token: {}", other)),
        }
    }
}
impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.symbol())
    }
}

pub struct PriceFeed {
    cache: HashMap<String, f64>,
}
impl PriceFeed {
    pub fn new() -> Self { Self { cache: HashMap::new() } }
    pub fn usd_price(&self, token: &Token) -> f64 {
        if token.is_stable() { return 1.0; }
        self.cache.get(token.symbol()).copied().unwrap_or_else(|| token.fallback_usd_price())
    }
    pub fn usd_to_token(&self, usd: f64, token: &Token) -> f64 {
        let p = self.usd_price(token);
        if p <= 0.0 { 0.0 } else { usd / p }
    }
    pub fn token_to_usd(&self, amount: f64, token: &Token) -> f64 {
        amount * self.usd_price(token)
    }
    pub fn update(&mut self, token: &Token, price: f64) {
        self.cache.insert(token.symbol().to_string(), price);
    }
}

pub struct MultiTokenProcessor {
    pub price_feed: PriceFeed,
}
impl MultiTokenProcessor {
    pub fn new() -> Self { Self { price_feed: PriceFeed::new() } }

    pub fn fee_in_token(&self, base_fee_bnkr: f64, token: &Token) -> f64 {
        if *token == Token::Bnkr { return base_fee_bnkr; }
        let usd = self.price_feed.token_to_usd(base_fee_bnkr, &Token::Bnkr);
        self.price_feed.usd_to_token(usd, token)
    }

    /// 85% treasury, 15% referrer
    pub fn split(&self, amount: f64) -> (f64, f64) {
        (amount * 0.85, amount * 0.15)
    }

    pub fn token_info(&self) -> Vec<serde_json::Value> {
        Token::all().iter().map(|t| serde_json::json!({
            "symbol":   t.symbol(),
            "chain":    t.chain(),
            "contract": t.contract_address(),
            "decimals": t.decimals(),
            "price_usd": self.price_feed.usd_price(t),
        })).collect()
    }
}

pub fn balance_column(token: &Token) -> &'static str {
    match token {
        Token::Bnkr    => "balance_bnkr",
        Token::Eth     => "balance_eth",
        Token::Usdc    => "balance_usdc",
        Token::Usdt    => "balance_usdt",
        Token::Sol     => "balance_sol",
        Token::Clanker => "balance_clanker",
        Token::Virtual => "balance_virtual",
    }
}
