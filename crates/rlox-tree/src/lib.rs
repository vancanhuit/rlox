//! `rlox` — a Rust port of the tree-walk Lox interpreter from
//! <https://craftinginterpreters.com>.
//!
//! Milestone 1 covered chapters 4–7 (scanner, parser, expression
//! evaluator). Chapter 8 adds statements, variable declarations, and
//! lexical scoping; the public API now distinguishes between
//! expression-level entry points ([`parse`], [`evaluate`]) and
//! program-level entry points ([`parse_program`], [`Interpreter`],
//! [`run`]/[`run_to`]).

use std::io::Write;

pub mod ast;
pub mod callable;
pub mod environment;
pub mod error;
pub mod interpreter;
pub mod parser;
pub mod resolver;
pub mod scanner;
pub mod token;
pub mod value;

pub use ast::{Expr, FunctionDecl, Stmt};
pub use callable::{Callable, LoxClass, LoxFunction, LoxInstance, NativeFn};
pub use environment::Environment;
pub use error::{LoxError, Result};
pub use interpreter::{Interpreter, evaluate, evaluate_in, stringify};
pub use parser::{parse, parse_program};
pub use resolver::{Locals, resolve};
pub use scanner::scan;
pub use token::{Literal, Token, TokenType};
pub use value::Value;

/// Run a Lox program, capturing any `print` output into a returned string.
///
/// Convenient for tests and for users who want a one-shot
/// `source -> stdout` translation. For streaming output (the binary writes
/// straight to stdout) use [`run_to`] instead.
///
/// # Errors
///
/// Returns every error encountered, mirroring jlox's reporting strategy:
///
/// - Scanning collects every lexical error before reporting.
/// - Parsing collects every parse error, calling `synchronize` between
///   failures so a single mistake doesn't suppress later diagnostics.
/// - A runtime error short-circuits during interpretation.
pub fn run(source: &str) -> std::result::Result<String, Vec<LoxError>> {
    let mut buf: Vec<u8> = Vec::new();
    run_to(source, &mut buf)?;
    Ok(String::from_utf8(buf).expect("interpreter only writes UTF-8 via stringify"))
}

/// Run a Lox program, streaming `print` output to `out`.
///
/// # Errors
///
/// See [`run`] for error semantics.
pub fn run_to(source: &str, out: &mut dyn Write) -> std::result::Result<(), Vec<LoxError>> {
    let (tokens, scan_errors) = scan(source);
    if !scan_errors.is_empty() {
        return Err(scan_errors);
    }
    let stmts = parse_program(&tokens)?;
    let locals = resolve(&stmts)?;
    let mut interp = Interpreter::new(out);
    interp.merge_locals(locals);
    interp.interpret(&stmts).map_err(|e| vec![e])?;
    Ok(())
}
