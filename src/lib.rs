//! `rlox` — a Rust port of the tree-walk Lox interpreter from
//! <https://craftinginterpreters.com>.
//!
//! Milestone 1 covers chapters 4–7 (scanner, parser, expression evaluator).

pub mod ast;
pub mod error;
pub mod interpreter;
pub mod parser;
pub mod scanner;
pub mod token;
pub mod value;

pub use ast::Expr;
pub use error::{LoxError, Result};
pub use interpreter::{evaluate, stringify};
pub use parser::parse;
pub use scanner::scan;
pub use token::{Literal, Token, TokenType};
pub use value::Value;

/// Run a Lox expression source through the full pipeline:
/// scan → parse → evaluate → stringify.
///
/// On success, returns the user-facing string form of the resulting value.
/// On failure, returns every error encountered:
///
/// - Scanning collects every lexical error before reporting.
/// - A parse error short-circuits before evaluation.
/// - A runtime error short-circuits during evaluation.
pub fn run(source: &str) -> std::result::Result<String, Vec<LoxError>> {
    let (tokens, scan_errors) = scan(source);
    if !scan_errors.is_empty() {
        return Err(scan_errors);
    }
    let expr = parse(&tokens).map_err(|e| vec![e])?;
    let value = evaluate(&expr).map_err(|e| vec![e])?;
    Ok(stringify(&value))
}
