//! Error type for the rlox front-end and runtime.
//!
//! Formatting follows the book's jlox conventions so output stays compatible
//! with the upstream Crafting Interpreters test corpus.

use std::error::Error;
use std::fmt;

use crate::token::{Token, TokenType};

/// Convenient `Result` alias used throughout the crate.
pub type Result<T> = std::result::Result<T, LoxError>;

/// Errors produced by the scanner, parser, or interpreter.
#[derive(Debug, Clone, PartialEq)]
pub enum LoxError {
    /// Lexical error (chapter 4). Format: `[line N] Error: <message>`.
    Scan { line: usize, message: String },

    /// Syntactic error (chapter 6). Format:
    /// `[line N] Error at '<lexeme>': <message>`, or
    /// `[line N] Error at end: <message>` when the offending token is EOF.
    Parse {
        line: usize,
        location: String,
        message: String,
    },

    /// Runtime error (chapter 7). The first stderr line is the message; the
    /// upstream test runner only checks that line, then a stack frame
    /// `[line N]` is printed below it.
    Runtime { line: usize, message: String },
}

impl LoxError {
    #[must_use]
    pub fn scan(line: usize, message: impl Into<String>) -> Self {
        Self::Scan {
            line,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn parse(token: &Token, message: impl Into<String>) -> Self {
        let location = if token.ttype == TokenType::Eof {
            " at end".to_string()
        } else {
            format!(" at '{}'", token.lexeme)
        };
        Self::Parse {
            line: token.line,
            location,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn runtime(token: &Token, message: impl Into<String>) -> Self {
        Self::Runtime {
            line: token.line,
            message: message.into(),
        }
    }

    /// Source line at which the error was detected.
    #[must_use]
    pub fn line(&self) -> usize {
        match self {
            Self::Scan { line, .. } | Self::Parse { line, .. } | Self::Runtime { line, .. } => {
                *line
            }
        }
    }
}

impl fmt::Display for LoxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scan { line, message } => write!(f, "[line {line}] Error: {message}"),
            Self::Parse {
                line,
                location,
                message,
            } => write!(f, "[line {line}] Error{location}: {message}"),
            Self::Runtime { line, message } => write!(f, "{message}\n[line {line}]"),
        }
    }
}

impl Error for LoxError {}
