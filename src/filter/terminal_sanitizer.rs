// src/filter/terminal_sanitizer.rs
// CSI (Command Sequence Injection) 방어 — 터미널 판 XSS
//
// 악성 에이전트가 ANSI escape code를 보내 터미널을 장악하는 것을 방지.
// 모든 외부 입력은 이 sanitizer를 통과해야 TUI에 렌더링된다.

use regex::Regex;
use lazy_static::lazy_static;

lazy_static! {
    /// ANSI escape sequence 패턴 (CSI, OSC, etc.)
    static ref ANSI_RE: Regex = Regex::new(
        r"(\x1B\[[0-?]*[ -/]*[@-~]|\x1B\][^\x07]*\x07|\x1B[^\[\]].?)"
    ).expect("ANSI regex must compile");
}

/// 외부 입력에서 제어 문자와 ANSI escape sequence를 제거한다.
/// 개행(\n)과 탭(\t)만 허용. 나머지 control char는 삭제.
pub fn sanitize_for_terminal(input: &str) -> String {
    // Step 1: ANSI escape sequence 제거
    let no_ansi = ANSI_RE.replace_all(input, "");

    // Step 2: 제어 문자 제거 (개행/탭 제외)
    no_ansi
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_removes_ansi_color_codes() {
        let input = "\x1B[31mRED TEXT\x1B[0m";
        assert_eq!(sanitize_for_terminal(input), "RED TEXT");
    }

    #[test]
    fn test_removes_cursor_movement() {
        let input = "\x1B[2J\x1B[H Goodbye World!";
        assert_eq!(sanitize_for_terminal(input), " Goodbye World!");
    }

    #[test]
    fn test_removes_control_chars() {
        let input = "Hello\x00\x01\x02World";
        assert_eq!(sanitize_for_terminal(input), "HelloWorld");
    }

    #[test]
    fn test_preserves_newline_and_tab() {
        let input = "Line1\nLine2\tTabbed";
        assert_eq!(sanitize_for_terminal(input), "Line1\nLine2\tTabbed");
    }

    #[test]
    fn test_clean_input_unchanged() {
        let input = "Normal agent response with G-Score: 0.42";
        assert_eq!(sanitize_for_terminal(input), input);
    }

    #[test]
    fn test_osc_sequence_removed() {
        // OSC (Operating System Command) — 터미널 제목 변경 공격
        let input = "\x1B]0;HACKED TITLE\x07Normal text";
        assert_eq!(sanitize_for_terminal(input), "Normal text");
    }
}
