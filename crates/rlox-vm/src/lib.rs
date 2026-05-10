//! `rlox-vm` — a Rust port of the bytecode-VM Lox interpreter from
//! Part III of <https://craftinginterpreters.com>.
//!
//! Milestone 3 layers chapters 14–30 onto the workspace one PR at a
//! time; the `chunk` module covers chapter 14 (the on-disk encoding for
//! a unit of compiled bytecode plus an offline disassembler).
//!
//! Subsequent chapters will add:
//!
//! - chapter 18+: full [`Value`] enum, strings, globals, locals,
//!   control flow, functions, closures, GC, and classes.
//!
//! At chapter 17 the crate exposes [`Chunk`], [`OpCode`], [`Value`], the
//! [`compiler`] (single-pass Pratt over [`rlox_shared::Scanner`]), the
//! [`disassembler`] helpers, the [`Vm`] interpreter, and a top-level
//! [`run_to`] entry point. The umbrella `rlox` binary's `--features vm`
//! build wires [`run_to`] into its CLI; chapter 17 evaluates a single
//! Lox *expression* and prints the result, which is enough to drive
//! the VM end-to-end. Statements + `print` arrive in chapter 21.

pub mod chunk;
pub mod compiler;
pub mod disassembler;
pub mod heap;
pub mod value;
pub mod vm;

pub use chunk::{Chunk, OpCode};
pub use compiler::compile;
pub use heap::{Handle, Heap, Obj};
pub use rlox_shared::error::{LoxError, Result};
pub use value::Value;
pub use vm::{Vm, VmError, VmResult};

use std::io::Write;

/// Compile and execute a Lox *expression*, writing the result followed
/// by a newline to `out`.
///
/// At chapter 17 the bytecode VM only handles expression-level Lox; the
/// surface that `rlox-tree` exposes for full programs (statements,
/// declarations, etc.) does not exist yet. The umbrella `rlox` binary
/// invokes this when built with `--features vm`.
///
/// # Errors
///
/// Returns every accumulated `LoxError` (scan, parse, or runtime) as a
/// `Vec`, mirroring the tree-walk crate's reporting strategy so the CLI
/// can pick a consistent exit code.
pub fn run_to(source: &str, out: &mut dyn Write) -> std::result::Result<(), Vec<LoxError>> {
    let (chunk, mut heap) = compile(source)?;
    let mut vm = Vm::new();
    let value = vm
        .interpret(&chunk, &mut heap)
        .map_err(|e| vec![vm_to_lox(e)])?;
    // Chapter 17+ renders the expression's value the same way clox's
    // `printValue` does (whole numbers as `42`, fractions natural,
    // booleans as `true`/`false`, the singleton as `nil`, strings
    // verbatim), terminated with a newline so line-oriented REPL
    // clients see one result per line.
    writeln!(out, "{}", value.display(&heap)).map_err(|e| {
        vec![LoxError::Runtime {
            line: 0,
            message: e.to_string(),
        }]
    })?;
    Ok(())
}

/// Translate a [`VmError`] into the workspace-wide [`LoxError`] surface
/// that both interpreter backends share. Runtime-error variants
/// preserve the originating source line so the umbrella binary's exit
/// reporting (`<message>\n[line N]`) lines up with the tree-walk's
/// output for the same condition.
fn vm_to_lox(err: vm::VmError) -> LoxError {
    match err {
        vm::VmError::Runtime { line, message } => LoxError::Runtime { line, message },
    }
}
