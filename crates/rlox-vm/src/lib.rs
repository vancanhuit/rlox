//! `rlox-vm` — a Rust port of the bytecode-VM Lox interpreter from
//! Part III of <https://craftinginterpreters.com>.
//!
//! Milestone 3 layers chapters 14–30 onto the workspace one PR at a
//! time; the `chunk` module covers chapter 14 (the on-disk encoding for
//! a unit of compiled bytecode plus an offline disassembler).
//!
//! Implemented so far:
//!
//! - chapters 14-17: chunks, disassembly, stack VM, lazy scanning, and
//!   single-pass Pratt compilation to bytecode.
//! - chapter 18: `nil`, booleans, comparisons, truthiness, and runtime
//!   type errors.
//! - chapter 19: heap-backed strings, content equality, and `+`
//!   concatenation.
//! - chapter 21: statements, `print`, global variables, and assignment.
//! - chapter 22: block scopes and stack-slot local variables.
//! - chapter 23: `if`/`else`, `and`/`or`, `while`, and `for`.
//! - chapter 24: function declarations, calls, `return`, recursion,
//!   and the native `clock`.
//! - chapter 25: closures and upvalues for captured locals.
//! - chapter 26: safe handle-based mark-sweep garbage collection.
//! - chapter 27: classes, instances, and dynamic fields.
//!
//! The crate exposes [`Chunk`], [`OpCode`], [`Value`], the
//! [`compiler`] (single-pass Pratt over [`rlox_shared::Scanner`]), the
//! [`disassembler`] helpers, the [`Vm`] interpreter, and a top-level
//! [`run_to`] entry point. The umbrella `rlox` binary's `--features vm`
//! build wires [`run_to`] into its CLI. The current VM executes a Lox
//! program made of top-level statements, global/local declarations,
//! structured control flow, first-class functions, closures, classes,
//! instances, dynamic fields, and VM-managed object reclamation.

pub mod chunk;
pub mod compiler;
pub mod disassembler;
pub mod heap;
pub mod value;
pub mod vm;

pub use chunk::{Chunk, OpCode};
pub use compiler::compile;
pub use heap::{
    Class, Closure, Function, GcRoots, GcStats, Handle, Heap, Instance, NativeFunction, Obj,
    Upvalue, UpvalueLocation,
};
pub use rlox_shared::error::{LoxError, Result};
pub use value::Value;
pub use vm::{Vm, VmError, VmResult};

use std::io::Write;

/// Compile and execute a Lox program, writing any `print` output to `out`.
///
/// This one-shot helper creates a fresh [`Vm`] and [`Heap`], so globals
/// do not persist across calls. Use [`run_to_with`] when building a REPL
/// or embedding a long-lived VM.
///
/// # Errors
///
/// Returns every accumulated `LoxError` (scan, parse, or runtime) as a
/// `Vec`, mirroring the tree-walk crate's reporting strategy so the CLI
/// can pick a consistent exit code.
pub fn run_to(source: &str, out: &mut dyn Write) -> std::result::Result<(), Vec<LoxError>> {
    let mut vm = Vm::new();
    let mut heap = Heap::new();
    run_to_with(source, &mut vm, &mut heap, out)
}

/// Compile and execute `source` using an existing [`Vm`] and [`Heap`].
///
/// Keeping both values alive across calls preserves global bindings and
/// any object handles stored in those bindings.
pub fn run_to_with(
    source: &str,
    vm: &mut Vm,
    heap: &mut Heap,
    out: &mut dyn Write,
) -> std::result::Result<(), Vec<LoxError>> {
    let chunk = compile(source, heap)?;
    vm.interpret(&chunk, heap, out)
        .map_err(|e| vec![vm_to_lox(e)])?;
    Ok(())
}

/// Compile and execute a single expression, writing its value followed
/// by a newline to `out`.
///
/// This compatibility helper is useful for embedding and focused tests;
/// the main VM entry point follows chapter 23 and expects full Lox
/// statements such as `print expr;`, `{ var local = expr; }`, or
/// `while (condition) statement;`.
pub fn eval_expr_to(source: &str, out: &mut dyn Write) -> std::result::Result<(), Vec<LoxError>> {
    let wrapped = format!("print {source};");
    run_to(&wrapped, out)
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
