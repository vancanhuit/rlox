//! Lox values — shared between the AST (`Expr::Literal`) and the upcoming
//! interpreter's runtime values.
//!
//! `Display` matches the formatting used by the upstream chap06/chap07 tests:
//! whole numbers render with a trailing `.0`, fractions render naturally, and
//! strings are unquoted. The interpreter (Phase 5) will layer its own
//! `stringify` on top to match the book's user-facing print formatting (which
//! strips the trailing `.0`).

use std::fmt;
use std::rc::Rc;

use crate::callable::{Callable, LoxInstance};

/// A Lox value: produced by literal expressions and by the interpreter.
#[derive(Debug, Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
    /// A callable function or class (chapter 10/12).
    Callable(Callable),
    /// A live class instance (chapter 12). Identity-based equality via
    /// `Rc::ptr_eq` matches jlox's behaviour for instance comparisons.
    Instance(Rc<LoxInstance>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Nil, Self::Nil) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            #[allow(clippy::float_cmp)]
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Callable(a), Self::Callable(b)) => a == b,
            (Self::Instance(a), Self::Instance(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
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
            Self::Callable(Callable::Native(n)) => write!(f, "<native fn {}>", n.name),
            Self::Callable(Callable::Function(func)) => {
                write!(f, "<fn {}>", func.decl.name.lexeme)
            }
            Self::Callable(Callable::Class(c)) => write!(f, "<class {}>", c.name),
            Self::Instance(inst) => write!(f, "{} instance", inst.class.name),
        }
    }
}
