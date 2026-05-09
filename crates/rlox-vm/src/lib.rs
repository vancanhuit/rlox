//! `rlox-vm` — a Rust port of the bytecode-VM Lox interpreter from
//! Part III of <https://craftinginterpreters.com>.
//!
//! Milestone 3 layers chapters 14–30 onto the workspace one PR at a
//! time; the `chunk` module covers chapter 14 (the on-disk encoding for
//! a unit of compiled bytecode plus an offline disassembler).
//!
//! Subsequent chapters will add:
//!
//! - chapter 15: the [`Vm`] interpreter loop
//! - chapter 16: scanning on demand
//! - chapter 17: a single-pass Pratt compiler from source to [`Chunk`]
//! - chapter 18+: full [`Value`] enum, strings, globals, locals,
//!   control flow, functions, closures, GC, and classes.
//!
//! For now the crate compiles to a library exposing only [`Chunk`],
//! [`OpCode`], [`Value`], and the [`disassembler`] helpers; the
//! end-to-end runner (and the umbrella binary's `--features vm` build)
//! lights up in PR 4 alongside chapter 17.

pub mod chunk;
pub mod disassembler;
pub mod value;

pub use chunk::{Chunk, OpCode};
pub use value::Value;
