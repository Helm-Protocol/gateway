// src/synco/mod.rs — Sync-O Deep Clean Engine
//
// 에이전트 데이터 위생 처리 (Helm 내부 API)
// POST /api/v1/clean — 스트림 정제
//
// 5단계 파이프라인:
//   1. 길이 차단 (Base64/바이너리 폭탄)
//   2. HTML 태그 제거
//   3. 공백 정규화
//   4. 스팸 패턴 차단 (Redis 동적 업데이트)
//   5. XXH3 중복 제거

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use parking_lot::RwLock;
use regex::Regex;
use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::xxh3_64;

pub const MAX_TEXT_LEN:  usize = 5_000;
pub const MIN_TEXT_LEN:  usize = 2;
pub const DEDUP_WINDOW:  usize = 50_000;

// ============================
// STATE
// ============================

pub struct SyncOEngine {
    /// 동적 스팸 패턴 (Redis로 hot-reload)
    pub spam_regex: Arc<RwLock<Regex>>,
    /// 중복 제거 윈도우
    seen_set:   Arc<RwLock<HashSet<u64>>>,
    seen_queue: Arc<RwLock<VecDeque<u64>>>,
}

impl SyncOEngine {
    pub fn new() -> Self {
        // 기본 스팸 패턴
        let initial = Regex::new(
            r"(?i)(buy crypto|click here|free money|\$\$\$|giveaway|pump it)"
        ).unwrap();

        Self {
            spam_regex: Arc::new(RwLock::new(initial)),
            seen_set:   Arc::new(RwLock::new(HashSet::with_capacity(DEDUP_WINDOW))),
            seen_queue: Arc::new(RwLock::new(VecDeque::with_capacity(DEDUP_WINDOW))),
        }
    }

    /// 스팸 패턴 hot-reload (OpenClaw / Redis 연동)
    pub fn update_patterns(&self, patterns: &[String]) {
        if patterns.is_empty() { return; }
        let joined = patterns.join("|");
        let new_pat = format!("(?i)({})", joined);
        match Regex::new(&new_pat) {
            Ok(r) => { *self.spam_regex.write() = r; }
            Err(e) => tracing::warn!("스팸 패턴 컴파일 실패: {}", e),
        }
    }

    // ============================
    // MAIN CLEAN PIPELINE
    // ============================

    pub fn clean(&self, input: &[String]) -> CleanResult {
        let start = std::time::Instant::now();
        let mut clean_data   = Vec::with_capacity(input.len());
        let mut dropped = 0usize;

        let spam_guard = self.spam_regex.read();
        let mut seen_set   = self.seen_set.write();
        let mut seen_queue = self.seen_queue.write();

        // HTML 태그 정규식 (스레드 로컬)
        let html_re     = Regex::new(r"<[^>]*>").unwrap();
        let longstr_re  = Regex::new(r"\S{500,}").unwrap();
        let space_re    = Regex::new(r"\s+").unwrap();

        for text in input {
            // Step 1: 길이 체크 + Base64/Long String 차단
            if text.len() > MAX_TEXT_LEN || text.len() < MIN_TEXT_LEN {
                dropped += 1;
                continue;
            }
            if longstr_re.is_match(text) {
                dropped += 1;
                continue;
            }

            // Step 2: HTML 태그 제거
            let mut cleaned = if text.contains('<') {
                html_re.replace_all(text, " ").to_string()
            } else {
                text.clone()
            };

            // Step 3: 공백 정규화
            cleaned = space_re.replace_all(&cleaned, " ").trim().to_string();
            if cleaned.len() < MIN_TEXT_LEN {
                dropped += 1;
                continue;
            }

            // Step 4: 스팸 패턴 체크
            if spam_guard.is_match(&cleaned) {
                dropped += 1;
                continue;
            }

            // Step 5: XXH3 중복 제거
            let hash = xxh3_64(cleaned.as_bytes());
            if seen_set.contains(&hash) {
                dropped += 1;
                continue;
            }

            // 통과 — 슬라이딩 윈도우 업데이트
            if seen_queue.len() >= DEDUP_WINDOW {
                if let Some(old) = seen_queue.pop_front() {
                    seen_set.remove(&old);
                }
            }
            seen_queue.push_back(hash);
            seen_set.insert(hash);
            clean_data.push(cleaned);
        }

        CleanResult {
            clean_data,
            original_count: input.len(),
            dropped_count: dropped,
            processing_ns: start.elapsed().as_nanos(),
        }
    }
}

impl Default for SyncOEngine {
    fn default() -> Self { Self::new() }
}

// ============================
// TYPES
// ============================

#[derive(Debug, Serialize, Deserialize)]
pub struct CleanRequest {
    pub agent_id: String,
    pub stream_data: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CleanResult {
    pub clean_data: Vec<String>,
    pub original_count: usize,
    pub dropped_count: usize,
    pub processing_ns: u128,
}

// ============================
// TESTS
// ============================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_removal() {
        let engine = SyncOEngine::new();
        let input = vec!["<div>Hello world</div>".into()];
        let result = engine.clean(&input);
        assert_eq!(result.clean_data[0], "Hello world");
        assert_eq!(result.dropped_count, 0);
    }

    #[test]
    fn test_spam_blocked() {
        let engine = SyncOEngine::new();
        let input = vec!["Buy Crypto Now!!!".into(), "Normal text here".into()];
        let result = engine.clean(&input);
        assert_eq!(result.dropped_count, 1);
        assert_eq!(result.clean_data.len(), 1);
    }

    #[test]
    fn test_dedup() {
        let engine = SyncOEngine::new();
        let input = vec![
            "Same text".into(),
            "Same text".into(),
            "Different text".into(),
        ];
        let result = engine.clean(&input);
        assert_eq!(result.clean_data.len(), 2);
        assert_eq!(result.dropped_count, 1);
    }

    #[test]
    fn test_long_string_blocked() {
        let engine = SyncOEngine::new();
        let long = "a".repeat(600);
        let input = vec![long];
        let result = engine.clean(&input);
        assert_eq!(result.dropped_count, 1);
    }

    #[test]
    fn test_pattern_update() {
        let engine = SyncOEngine::new();
        engine.update_patterns(&["solana giveaway".into(), "rug pull".into()]);
        let input = vec!["Solana Giveaway click here".into()];
        let result = engine.clean(&input);
        assert_eq!(result.dropped_count, 1);
    }
}
