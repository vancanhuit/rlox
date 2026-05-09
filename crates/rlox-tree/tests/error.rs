//! Phase 1 — public-API tests for `error` module.
//!
//! Error formatting follows the book's jlox conventions:
//!
//! - Scan errors render as `[line N] Error: <msg>`.
//! - Parse errors render as `[line N] Error at '<lexeme>': <msg>`, or
//!   `[line N] Error at end: <msg>` when the offending token is EOF.
//! - Runtime errors print the message on its own line and a stack frame
//!   `[line N]` after it. The upstream test runner only checks the first
//!   stderr line against the message, hence `<msg>\n[line N]`.

use std::error::Error;

use rlox_tree::{LoxError, Token, TokenType};

#[test]
fn scan_error_displays_with_line_prefix() {
    let err = LoxError::scan(3, "Unexpected character.");
    assert_eq!(err.to_string(), "[line 3] Error: Unexpected character.");
}

#[test]
fn parse_error_at_token_includes_lexeme() {
    let token = Token::new(TokenType::RightParen, ")", None, 5);
    let err = LoxError::parse(&token, "Expect expression.");
    assert_eq!(err.to_string(), "[line 5] Error at ')': Expect expression.");
}

#[test]
fn parse_error_at_eof_uses_at_end_marker() {
    let eof = Token::new(TokenType::Eof, "", None, 9);
    let err = LoxError::parse(&eof, "Expect ';' after value.");
    assert_eq!(
        err.to_string(),
        "[line 9] Error at end: Expect ';' after value."
    );
}

#[test]
fn runtime_error_displays_message_then_line_frame() {
    let plus = Token::new(TokenType::Plus, "+", None, 12);
    let err = LoxError::runtime(&plus, "Operands must be two numbers or two strings.");
    assert_eq!(
        err.to_string(),
        "\
Operands must be two numbers or two strings.
[line 12]"
    );
}

#[test]
fn lox_error_implements_std_error() {
    fn assert_is_error<E: Error>(_: &E) {}
    let err = LoxError::scan(1, "boom");
    assert_is_error(&err);
}
