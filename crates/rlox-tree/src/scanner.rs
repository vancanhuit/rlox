//! Lexical scanner for the Lox language.
//!
//! Walks a source string and produces a [`Token`] stream. Lexical errors are
//! collected (jlox-style) so callers see every problem in one pass; the EOF
//! token is always appended at the tail.
//!
//! Lox source is treated as bytes for the purposes of scanning: every token
//! delimiter and every grammar character is ASCII, so byte-indexed slicing
//! into the original `&str` is sound and avoids the cost of a `Vec<char>`.
//! Non-ASCII bytes are only legal *inside* string literals, where their
//! exact UTF-8 encoding is preserved verbatim by slicing the source.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::error::LoxError;
use crate::token::{Literal, Token, TokenType};

static KEYWORDS: LazyLock<HashMap<&'static str, TokenType>> = LazyLock::new(|| {
    [
        ("and", TokenType::And),
        ("class", TokenType::Class),
        ("else", TokenType::Else),
        ("false", TokenType::False),
        ("for", TokenType::For),
        ("fun", TokenType::Fun),
        ("if", TokenType::If),
        ("nil", TokenType::Nil),
        ("or", TokenType::Or),
        ("print", TokenType::Print),
        ("return", TokenType::Return),
        ("super", TokenType::Super),
        ("this", TokenType::This),
        ("true", TokenType::True),
        ("var", TokenType::Var),
        ("while", TokenType::While),
    ]
    .into_iter()
    .collect()
});

/// Scan `source` into tokens, returning any lexical errors alongside.
///
/// The token vector always ends with [`TokenType::Eof`], even when
/// `errors` is non-empty.
#[must_use]
pub fn scan(source: &str) -> (Vec<Token>, Vec<LoxError>) {
    let mut s = State::new(source);
    while !s.at_end() {
        s.start = s.current;
        scan_one(&mut s);
    }
    s.push_token(TokenType::Eof, "", None);
    (s.tokens, s.errors)
}

struct State<'src> {
    src: &'src str,
    bytes: &'src [u8],
    tokens: Vec<Token>,
    errors: Vec<LoxError>,
    start: usize,
    current: usize,
    line: usize,
}

impl<'src> State<'src> {
    const fn new(src: &'src str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            tokens: Vec::new(),
            errors: Vec::new(),
            start: 0,
            current: 0,
            line: 1,
        }
    }

    const fn at_end(&self) -> bool {
        self.current >= self.bytes.len()
    }

    fn bump(&mut self) -> u8 {
        let b = self.bytes[self.current];
        self.current += 1;
        b
    }

    fn peek(&self) -> u8 {
        self.bytes.get(self.current).copied().unwrap_or(0)
    }

    fn peek_next(&self) -> u8 {
        self.bytes.get(self.current + 1).copied().unwrap_or(0)
    }

    fn take(&mut self, expected: u8) -> bool {
        if self.peek() == expected {
            self.current += 1;
            true
        } else {
            false
        }
    }

    fn lexeme(&self) -> &'src str {
        &self.src[self.start..self.current]
    }

    fn push_token(
        &mut self,
        ttype: TokenType,
        lexeme: impl Into<String>,
        literal: Option<Literal>,
    ) {
        self.tokens
            .push(Token::new(ttype, lexeme, literal, self.line));
    }

    fn record_error(&mut self, message: &str) {
        self.errors.push(LoxError::scan(self.line, message));
    }
}

/// Consume one token (or whitespace, or a comment, or record one error).
fn scan_one(s: &mut State<'_>) {
    let b = s.bump();
    match b {
        b'(' => s.push_token(TokenType::LeftParen, s.lexeme(), None),
        b')' => s.push_token(TokenType::RightParen, s.lexeme(), None),
        b'{' => s.push_token(TokenType::LeftBrace, s.lexeme(), None),
        b'}' => s.push_token(TokenType::RightBrace, s.lexeme(), None),
        b',' => s.push_token(TokenType::Comma, s.lexeme(), None),
        b'.' => s.push_token(TokenType::Dot, s.lexeme(), None),
        b'-' => s.push_token(TokenType::Minus, s.lexeme(), None),
        b'+' => s.push_token(TokenType::Plus, s.lexeme(), None),
        b';' => s.push_token(TokenType::Semicolon, s.lexeme(), None),
        b'*' => s.push_token(TokenType::Star, s.lexeme(), None),
        b'!' => emit_two_char(s, b'=', TokenType::BangEqual, TokenType::Bang),
        b'=' => emit_two_char(s, b'=', TokenType::EqualEqual, TokenType::Equal),
        b'<' => emit_two_char(s, b'=', TokenType::LessEqual, TokenType::Less),
        b'>' => emit_two_char(s, b'=', TokenType::GreaterEqual, TokenType::Greater),
        b'/' => slash_or_comment(s),
        b' ' | b'\r' | b'\t' => {}
        b'\n' => s.line += 1,
        b'"' => string_literal(s),
        d if d.is_ascii_digit() => number_literal(s),
        a if is_ident_start(a) => identifier_or_keyword(s),
        _ => s.record_error("Unexpected character."),
    }
}

fn emit_two_char(s: &mut State<'_>, second: u8, both: TokenType, single: TokenType) {
    let ttype = if s.take(second) { both } else { single };
    s.push_token(ttype, s.lexeme(), None);
}

fn slash_or_comment(s: &mut State<'_>) {
    if s.take(b'/') {
        while !s.at_end() && s.peek() != b'\n' {
            s.current += 1;
        }
    } else {
        s.push_token(TokenType::Slash, s.lexeme(), None);
    }
}

fn string_literal(s: &mut State<'_>) {
    while !s.at_end() && s.peek() != b'"' {
        if s.peek() == b'\n' {
            s.line += 1;
        }
        s.bump();
    }
    if s.at_end() {
        s.record_error("Unterminated string.");
        return;
    }
    s.bump(); // closing quote
    let lexeme = s.lexeme();
    // Strip the surrounding quotes for the literal value.
    let value = &s.src[s.start + 1..s.current - 1];
    s.push_token(
        TokenType::String,
        lexeme,
        Some(Literal::String(value.to_owned())),
    );
}

fn number_literal(s: &mut State<'_>) {
    while s.peek().is_ascii_digit() {
        s.current += 1;
    }
    // Fractional part — only valid when `.` is immediately followed by a
    // digit. `123.` instead lexes as NUMBER(123) DOT.
    if s.peek() == b'.' && s.peek_next().is_ascii_digit() {
        s.current += 1;
        while s.peek().is_ascii_digit() {
            s.current += 1;
        }
    }
    let lexeme = s.lexeme();
    let value: f64 = lexeme
        .parse()
        .expect("scanned digit sequence is always valid f64");
    s.push_token(TokenType::Number, lexeme, Some(Literal::Number(value)));
}

fn identifier_or_keyword(s: &mut State<'_>) {
    while is_ident_continue(s.peek()) {
        s.current += 1;
    }
    let lexeme = s.lexeme();
    let ttype = KEYWORDS
        .get(lexeme)
        .copied()
        .unwrap_or(TokenType::Identifier);
    s.push_token(ttype, lexeme, None);
}

const fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

const fn is_ident_continue(b: u8) -> bool {
    is_ident_start(b) || b.is_ascii_digit()
}
