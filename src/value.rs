//! Lox values — shared between the AST (`Expr::Literal`) and the upcoming
//! interpreter's runtime values.
//!
//! `Display` matches the formatting used by the upstream chap06/chap07 tests:
//! whole numbers render with a trailing `.0`, fractions render naturally, and
//! strings are unquoted. The interpreter (Phase 5) will layer its own
//! `stringify` on top to match the book's user-facing print formatting (which
//! strips the trailing `.0`).

use std::fmt;

/// A Lox value: produced by literal expressions and by the interpreter.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => f.write_str("nil"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Number(n) => {
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
