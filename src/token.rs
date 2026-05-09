//! Lexical tokens produced by the scanner.
//!
//! The `Display` impls intentionally match the upstream Crafting Interpreters
//! test format (e.g. `NUMBER 123 123.0`, `DOT . null`, `EOF  null`) so a future
//! token-dump mode can reuse them verbatim.

use std::fmt;

/// Every kind of lexical token recognised by the Lox grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenType {
    // Single-character tokens.
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Comma,
    Dot,
    Minus,
    Plus,
    Semicolon,
    Slash,
    Star,

    // One- or two-character tokens.
    Bang,
    BangEqual,
    Equal,
    EqualEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,

    // Literals.
    Identifier,
    String,
    Number,

    // Keywords.
    And,
    Class,
    Else,
    False,
    Fun,
    For,
    If,
    Nil,
    Or,
    Print,
    Return,
    Super,
    This,
    True,
    Var,
    While,

    Eof,
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::LeftParen => "LEFT_PAREN",
            Self::RightParen => "RIGHT_PAREN",
            Self::LeftBrace => "LEFT_BRACE",
            Self::RightBrace => "RIGHT_BRACE",
            Self::Comma => "COMMA",
            Self::Dot => "DOT",
            Self::Minus => "MINUS",
            Self::Plus => "PLUS",
            Self::Semicolon => "SEMICOLON",
            Self::Slash => "SLASH",
            Self::Star => "STAR",
            Self::Bang => "BANG",
            Self::BangEqual => "BANG_EQUAL",
            Self::Equal => "EQUAL",
            Self::EqualEqual => "EQUAL_EQUAL",
            Self::Greater => "GREATER",
            Self::GreaterEqual => "GREATER_EQUAL",
            Self::Less => "LESS",
            Self::LessEqual => "LESS_EQUAL",
            Self::Identifier => "IDENTIFIER",
            Self::String => "STRING",
            Self::Number => "NUMBER",
            Self::And => "AND",
            Self::Class => "CLASS",
            Self::Else => "ELSE",
            Self::False => "FALSE",
            Self::Fun => "FUN",
            Self::For => "FOR",
            Self::If => "IF",
            Self::Nil => "NIL",
            Self::Or => "OR",
            Self::Print => "PRINT",
            Self::Return => "RETURN",
            Self::Super => "SUPER",
            Self::This => "THIS",
            Self::True => "TRUE",
            Self::Var => "VAR",
            Self::While => "WHILE",
            Self::Eof => "EOF",
        };
        f.write_str(s)
    }
}

/// A literal value attached to a token by the scanner.
///
/// Only `NUMBER` and `STRING` tokens carry a literal; the rest store `None`
/// in [`Token::literal`] (rendered as `null` in the dump format).
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Number(f64),
    String(String),
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(n) => {
                // Match jlox's `Object.toString()` for `Double`: whole numbers
                // render as `123.0`, fractions render naturally.
                if n.fract() == 0.0 && n.is_finite() {
                    write!(f, "{n:.1}")
                } else {
                    write!(f, "{n}")
                }
            }
            Self::String(s) => f.write_str(s),
        }
    }
}

/// A scanned token with source attribution.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub ttype: TokenType,
    pub lexeme: String,
    pub literal: Option<Literal>,
    pub line: usize,
}

impl Token {
    #[must_use]
    pub fn new(ttype: TokenType, lexeme: String, literal: Option<Literal>, line: usize) -> Self {
        Self {
            ttype,
            lexeme,
            literal,
            line,
        }
    }
}

impl fmt::Display for Token {
    /// Format used by the upstream chap04 scanner-dump suite:
    /// `<TYPE> <LEXEME> <LITERAL-or-`null`>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} ", self.ttype, self.lexeme)?;
        match &self.literal {
            Some(lit) => write!(f, "{lit}"),
            None => f.write_str("null"),
        }
    }
}
