//! `rlox-shared` — front-end primitives shared by both Lox interpreter
//! backends (`rlox-tree`, `rlox-vm`).
//!
//! The shared crate currently houses:
//!
//! - [`token`] — `Token`, `TokenType`, `Literal` (the on-the-wire
//!   representation of a Lox lexeme).
//! - [`error`] — `LoxError`, a tagged union covering scan / parse /
//!   runtime errors. Both interpreters surface the same Lox-level
//!   diagnostic format, so it lives here rather than being duplicated.
//! - [`scanner`] — the lexer. Two APIs:
//!   - [`scanner::scan`] — eager: scans the whole source up-front into
//!     `(Vec<Token>, Vec<LoxError>)`. Used by `rlox-tree` (jlox-style
//!     bulk parse) and the test corpus.
//!   - [`scanner::Scanner`] — lazy iterator (`Iterator<Item =
//!     Result<Token, LoxError>>`) used by `rlox-vm`'s single-pass Pratt
//!     compiler for chapter 16 (*Scanning on Demand*).
//!
//! Both APIs share the same scanning logic via an internal `State`
//! machine, so any future scanner change automatically lights up in
//! both modes.

pub mod error;
pub mod scanner;
pub mod token;

pub use error::{LoxError, Result};
pub use scanner::{Scanner, scan};
pub use token::{Literal, Token, TokenType};
