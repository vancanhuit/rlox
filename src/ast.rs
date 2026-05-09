//! Abstract syntax tree for Lox expressions (chapters 5–6 of the book).
//!
//! We use an idiomatic Rust enum + `match` rather than the book's Visitor
//! pattern. The `Display` impl produces the parenthesised, prefix-Lisp form
//! used by the upstream `test/expressions/parse.lox` reference output.

use std::fmt;

use crate::token::Token;
use crate::value::Value;

/// An expression node.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Binary {
        left: Box<Expr>,
        op: Token,
        right: Box<Expr>,
    },
    Grouping(Box<Expr>),
    Literal(Value),
    Unary {
        op: Token,
        right: Box<Expr>,
    },
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary { left, op, right } => {
                parenthesize(f, &op.lexeme, [left.as_ref(), right.as_ref()])
            }
            Self::Grouping(inner) => parenthesize(f, "group", [inner.as_ref()]),
            Self::Literal(value) => write!(f, "{value}"),
            Self::Unary { op, right } => parenthesize(f, &op.lexeme, [right.as_ref()]),
        }
    }
}

fn parenthesize<'a, I>(f: &mut fmt::Formatter<'_>, name: &str, exprs: I) -> fmt::Result
where
    I: IntoIterator<Item = &'a Expr>,
{
    f.write_str("(")?;
    f.write_str(name)?;
    for expr in exprs {
        write!(f, " {expr}")?;
    }
    f.write_str(")")
}
