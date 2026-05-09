//! `rlox` — a Rust port of the tree-walk Lox interpreter from
//! <https://craftinginterpreters.com>.
//!
//! Milestone 1 covers chapters 4–7 (scanner, parser, expression evaluator).

pub mod ast;
pub mod error;
pub mod scanner;
pub mod token;
pub mod value;

pub use ast::Expr;
pub use error::{LoxError, Result};
pub use scanner::scan;
pub use token::{Literal, Token, TokenType};
pub use value::Value;
