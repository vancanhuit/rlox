//! Lexical scanner for the Lox language.
//!
//! Walks a source string and produces a [`Token`] stream. Lexical errors are
//! interleaved with tokens in source order; the EOF token is always emitted
//! last.
//!
//! Two public entry points share one underlying state machine:
//!
//! - [`scan`] (eager, jlox-style) — consumes the whole source up front and
//!   returns `(Vec<Token>, Vec<LoxError>)`. Used by `rlox-tree`'s bulk-parse
//!   front end and by the test corpus.
//! - [`Scanner`] (lazy, clox-style chapter 16) — implements
//!   `Iterator<Item = Result<Token, LoxError>>` and only advances when the
//!   consumer (e.g. the chapter 17 single-pass Pratt compiler) asks for the
//!   next token.
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

/// One unit of scanner output: either a successfully-scanned token or a
/// lexical error pinned to its source line. Both APIs surface the same
/// stream; only the framing differs.
pub type ScanEvent = Result<Token, LoxError>;

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
/// `errors` is non-empty. Internally this is a thin partition over
/// [`Scanner`]; the eager and lazy paths share one state machine.
#[must_use]
pub fn scan(source: &str) -> (Vec<Token>, Vec<LoxError>) {
    let mut tokens = Vec::new();
    let mut errors = Vec::new();
    for event in Scanner::new(source) {
        match event {
            Ok(t) => tokens.push(t),
            Err(e) => errors.push(e),
        }
    }
    (tokens, errors)
}

/// Lazy, chapter-16 streaming scanner.
///
/// Implements `Iterator<Item = ScanEvent>` so a downstream consumer
/// (e.g. `rlox-vm`'s single-pass Pratt compiler) can pull one token at
/// a time. Source-order is preserved: an error is yielded *exactly* at
/// the point it would have been recorded by the eager scanner.
///
/// The iterator yields exactly one [`TokenType::Eof`] token after the
/// last lexeme and then returns `None`.
#[derive(Debug)]
pub struct Scanner<'src> {
    state: State<'src>,
    /// Number of events already emitted. Each call to [`scan_one`]
    /// pushes either zero or one event, so a single index suffices.
    next_idx: usize,
    /// Whether we've already pushed the EOF token. Once true, the
    /// iterator emits the final EOF event then transitions to `None`.
    eof_pushed: bool,
}

impl<'src> Scanner<'src> {
    /// Build a fresh scanner over `source`. Tokens are pulled lazily
    /// via the [`Iterator`] impl.
    #[must_use]
    pub fn new(source: &'src str) -> Self {
        Self {
            state: State::new(source),
            next_idx: 0,
            eof_pushed: false,
        }
    }

    /// Source line currently being scanned. Useful for error
    /// diagnostics in callers that want to attribute their *own*
    /// failures (e.g. parse errors) to a token's line — though most
    /// callers will read [`Token::line`] directly.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.state.line
    }
}

impl Iterator for Scanner<'_> {
    type Item = ScanEvent;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Drain any event already produced by an earlier scan_one
            // call (which we'll trigger below if the buffer is empty).
            if self.next_idx < self.state.events.len() {
                let event = self.state.events[self.next_idx].clone();
                self.next_idx += 1;
                return Some(event);
            }

            if self.state.at_end() {
                if self.eof_pushed {
                    return None;
                }
                self.eof_pushed = true;
                self.state.push_token(TokenType::Eof, "", None);
                continue; // drain the freshly-pushed EOF
            }

            // Advance one step. scan_one pushes at most one event
            // (whitespace and comments push zero); the loop continues
            // until something concrete is produced or we hit EOF.
            self.state.start = self.state.current;
            scan_one(&mut self.state);
        }
    }
}

#[derive(Debug)]
struct State<'src> {
    src: &'src str,
    bytes: &'src [u8],
    /// One ordered stream of scanner output, interleaving tokens and
    /// errors at the source position they were detected. The eager
    /// [`scan`] partitions this into two `Vec`s; the lazy [`Scanner`]
    /// drains it incrementally.
    events: Vec<ScanEvent>,
    start: usize,
    current: usize,
    line: usize,
}

impl<'src> State<'src> {
    const fn new(src: &'src str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            events: Vec::new(),
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
        self.events
            .push(Ok(Token::new(ttype, lexeme, literal, self.line)));
    }

    fn record_error(&mut self, message: &str) {
        self.events.push(Err(LoxError::scan(self.line, message)));
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
