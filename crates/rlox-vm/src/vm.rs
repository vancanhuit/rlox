//! Chapter 15 — A Virtual Machine.
//!
//! A stack-based interpreter for [`Chunk`]s. The VM walks the byte
//! stream one instruction at a time, pushing and popping [`Value`]s on
//! a runtime stack. With only [`OpCode::Constant`], the four binary
//! arithmetic ops, [`OpCode::Negate`], and [`OpCode::Return`], a chunk
//! built by hand can already evaluate any pure-numeric expression.
//!
//! Reference: clox `vm.h` / `vm.c`. Differences from clox:
//!
//! - The stack is a `Vec<Value>` rather than a fixed-size `Value[256]`.
//!   Lox itself imposes no statically-known upper bound (recursion +
//!   nested function calls land in chapter 24+), so a growable stack is
//!   simpler and still cheap.
//! - The instruction pointer is a `usize` index into `chunk.code`
//!   instead of a raw `*const u8`. The borrow checker objects to the
//!   pointer-walking style and the index version compiles to the same
//!   code under release optimisations.
//! - `OP_RETURN` *returns* the popped value through `Result<Value, _>`
//!   rather than printing to stdout. Tests that need to observe the
//!   computed value can do so directly; chapters 21+ will add an
//!   explicit `OP_PRINT` opcode for user-visible output that writes to
//!   a configurable [`std::io::Write`] sink.
//! - clox's `DEBUG_TRACE_EXECUTION` macro is replaced by a runtime
//!   trace sink: see [`Vm::interpret_with_trace`].
//!
//! Stack underflow is a compiler bug, not a runtime error. The VM
//! [`assert!`]s instead of returning a `Result`, mirroring clox's
//! "we don't recover from a malformed chunk" stance.

use std::fmt::{self, Write};

use crate::chunk::{Chunk, OpCode};
use crate::disassembler::disassemble_instruction;
use crate::heap::{Heap, Obj};
use crate::value::{Value, values_equal};

/// A runtime error surfaced by the VM.
///
/// Chapter 15 has no type errors yet — the only kind of value is
/// [`Value::Number`]. The variant is reserved here so chapters 18+
/// (where `1 + true` becomes meaningful) can grow it without
/// retrofitting callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmError {
    /// A runtime error tied to a source line. The `line` is looked up
    /// via [`crate::chunk::LineRle::line_at`] so traces can show the
    /// originating Lox source location.
    Runtime { line: usize, message: String },
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // clox-style: the message first, then a `[line N]` frame.
            // chapter-15 has no actual runtime errors, so this branch
            // is reached only by future chapters' tests.
            Self::Runtime { line, message } => write!(f, "{message}\n[line {line}]"),
        }
    }
}

impl std::error::Error for VmError {}

/// Convenience alias for the VM's [`Result`] type.
pub type VmResult<T = Value> = Result<T, VmError>;

/// The bytecode interpreter.
///
/// One [`Vm`] can execute many [`Chunk`]s back-to-back; the stack is
/// drained between programs by [`OpCode::Return`]. Holding the VM
/// across calls is the seed for chapter 21's persistent globals table
/// and chapter 24's call-frame stack.
#[derive(Debug, Default)]
pub struct Vm {
    /// Operand stack. Drained on `OP_RETURN`.
    stack: Vec<Value>,
}

impl Vm {
    /// A fresh VM with an empty stack.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `chunk` to completion, returning the value at the top of
    /// the stack when the chunk's `OP_RETURN` fires.
    ///
    /// # Errors
    ///
    /// Returns [`VmError::Runtime`] for any runtime fault. Chapter 15
    /// has none, so this is currently unreachable from a well-formed
    /// chunk; chapters 18+ will populate it.
    ///
    /// # Panics
    ///
    /// Panics on malformed bytecode (stack underflow, dangling operand,
    /// unknown opcode, missing terminating `OP_RETURN`). These are
    /// compiler bugs, not runtime errors, and matching clox we abort
    /// rather than try to recover.
    pub fn interpret(&mut self, chunk: &Chunk, heap: &mut Heap) -> VmResult<Value> {
        self.run(chunk, heap, None::<&mut String>)
    }

    /// Like [`Self::interpret`], but write a clox-style execution trace
    /// to `trace`. Each step prints the current stack contents on one
    /// line and the disassembled instruction on the next, matching the
    /// book's `DEBUG_TRACE_EXECUTION` output.
    ///
    /// # Errors
    ///
    /// Same as [`Self::interpret`].
    pub fn interpret_with_trace<W: Write>(
        &mut self,
        chunk: &Chunk,
        heap: &mut Heap,
        trace: &mut W,
    ) -> VmResult<Value> {
        self.run(chunk, heap, Some(trace))
    }

    fn run<W: Write>(
        &mut self,
        chunk: &Chunk,
        heap: &mut Heap,
        mut trace: Option<&mut W>,
    ) -> VmResult<Value> {
        // Reset the stack between programs so a `Vm` can be reused. We
        // reuse the allocation rather than reallocating.
        self.stack.clear();

        let mut ip: usize = 0;
        loop {
            if let Some(out) = trace.as_deref_mut() {
                trace_step(chunk, ip, &self.stack, out);
            }

            // The opcode lives at `op_ip`; chapter 18 introduces runtime
            // errors which need to attribute themselves to the *opcode's*
            // source line, not the operand byte that follows.
            let op_ip = ip;
            let byte = chunk.code[op_ip];
            let op = OpCode::from_byte(byte)
                .unwrap_or_else(|| panic!("unknown opcode {byte:#04x} at offset {op_ip} in chunk"));
            ip = op_ip + 1;

            match op {
                OpCode::Constant => {
                    let idx = chunk.code[ip] as usize;
                    ip += 1;
                    let value = chunk.constants[idx];
                    self.push(value);
                }
                OpCode::Nil => self.push(Value::Nil),
                OpCode::True => self.push(Value::Bool(true)),
                OpCode::False => self.push(Value::Bool(false)),
                OpCode::Not => {
                    let v = self.pop();
                    self.push(Value::Bool(!v.is_truthy()));
                }
                OpCode::Equal => {
                    // Polymorphic equality: mixed types compare unequal
                    // rather than raising. The helper dereferences
                    // object handles through the heap so two distinct
                    // allocations of the same string compare equal
                    // (chapter 21 will swap that out for pointer
                    // equality once strings are interned).
                    let b = self.pop();
                    let a = self.pop();
                    self.push(Value::Bool(values_equal(a, b, heap)));
                }
                OpCode::Greater => self.binary_cmp(chunk, op_ip, |a, b| a > b)?,
                OpCode::Less => self.binary_cmp(chunk, op_ip, |a, b| a < b)?,
                OpCode::Negate => {
                    let Value::Number(n) = self.pop() else {
                        return Err(Self::runtime_error(
                            chunk,
                            op_ip,
                            "Operand must be a number.",
                        ));
                    };
                    self.push(Value::Number(-n));
                }
                OpCode::Add => self.op_add(chunk, op_ip, heap)?,
                OpCode::Subtract => self.binary_arith(chunk, op_ip, |a, b| a - b)?,
                OpCode::Multiply => self.binary_arith(chunk, op_ip, |a, b| a * b)?,
                OpCode::Divide => self.binary_arith(chunk, op_ip, |a, b| a / b)?,
                OpCode::Return => return Ok(self.pop()),
            }
        }
    }

    fn push(&mut self, v: Value) {
        self.stack.push(v);
    }

    fn pop(&mut self) -> Value {
        self.stack
            .pop()
            .expect("stack underflow: malformed bytecode (compiler bug)")
    }

    /// Chapter 19 `OP_ADD`: dispatches on the operand types so the
    /// same opcode covers `1 + 2` (numeric addition) and `"a" + "b"`
    /// (string concatenation). Anything else is a runtime error
    /// matching clox: `Operands must be two numbers or two strings.`
    fn op_add(&mut self, chunk: &Chunk, op_ip: usize, heap: &mut Heap) -> VmResult<()> {
        let b = self.pop();
        let a = self.pop();
        match (a, b) {
            (Value::Number(a), Value::Number(b)) => {
                self.push(Value::Number(a + b));
                Ok(())
            }
            (Value::Obj(ah), Value::Obj(bh)) => {
                // Both must currently be strings — chapter 19's only
                // object kind. Other object kinds (functions, classes)
                // do not concatenate, so a future kind-mismatch falls
                // through to the same `must be two strings` error.
                match (heap.get(ah), heap.get(bh)) {
                    (Obj::Str(a), Obj::Str(b)) => {
                        let mut concat = String::with_capacity(a.len() + b.len());
                        concat.push_str(a);
                        concat.push_str(b);
                        let h = heap.alloc_string(concat);
                        self.push(Value::Obj(h));
                        Ok(())
                    }
                }
            }
            _ => Err(Self::runtime_error(
                chunk,
                op_ip,
                "Operands must be two numbers or two strings.",
            )),
        }
    }

    /// Binary numeric arithmetic (`-`, `*`, `/`) that returns a number.
    /// `+` has its own helper because chapter 19 overloads it for
    /// strings.
    fn binary_arith<F: Fn(f64, f64) -> f64>(
        &mut self,
        chunk: &Chunk,
        op_ip: usize,
        op: F,
    ) -> VmResult<()> {
        // `b` is popped first because it was pushed last (right-hand operand).
        let b = self.pop();
        let a = self.pop();
        let (Value::Number(a), Value::Number(b)) = (a, b) else {
            return Err(Self::runtime_error(
                chunk,
                op_ip,
                "Operands must be numbers.",
            ));
        };
        self.push(Value::Number(op(a, b)));
        Ok(())
    }

    /// Binary numeric comparison that returns a boolean.
    fn binary_cmp<F: Fn(f64, f64) -> bool>(
        &mut self,
        chunk: &Chunk,
        op_ip: usize,
        op: F,
    ) -> VmResult<()> {
        let b = self.pop();
        let a = self.pop();
        let (Value::Number(a), Value::Number(b)) = (a, b) else {
            return Err(Self::runtime_error(
                chunk,
                op_ip,
                "Operands must be numbers.",
            ));
        };
        self.push(Value::Bool(op(a, b)));
        Ok(())
    }

    /// Build a runtime error attributed to the source line of the
    /// instruction at `op_ip`. Associated function (no `self`) because
    /// it doesn't read the operand stack and clippy prefers it that way.
    fn runtime_error(chunk: &Chunk, op_ip: usize, message: &str) -> VmError {
        let line = chunk.lines.line_at(op_ip).unwrap_or(0);
        VmError::Runtime {
            line,
            message: message.to_string(),
        }
    }
}

/// Format a single trace step into `out`, mimicking clox's
/// `DEBUG_TRACE_EXECUTION` block:
///
/// ```text
///           [ 1.2 ][ 3.4 ]
/// 0000  123 OP_ADD
/// ```
fn trace_step(chunk: &Chunk, ip: usize, stack: &[Value], out: &mut dyn Write) {
    // The trace's stack rendering uses the bare Display (not the
    // heap-aware adapter), so any object handle prints as `<obj#N>`.
    // The book's chapter-15 trace tests still pass because they only
    // exercise numeric values; chapter 19 adds new heap-aware tests
    // through different surfaces.
    let _ = trace_inner(chunk, ip, stack, out);
}

fn trace_inner(chunk: &Chunk, ip: usize, stack: &[Value], out: &mut dyn Write) -> fmt::Result {
    // 10 spaces to align with the `0000` offset column under the
    // following disassembled instruction.
    write!(out, "          ")?;
    for v in stack {
        write!(out, "[ {v} ]")?;
    }
    writeln!(out)?;
    disassemble_instruction(chunk, ip, out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a chunk that pushes `n` and immediately returns it.
    fn return_constant(n: f64) -> Chunk {
        let mut c = Chunk::new();
        let idx = c.add_constant(Value::Number(n));
        c.write_op(OpCode::Constant, 1);
        c.write_byte(idx, 1);
        c.write_op(OpCode::Return, 1);
        c
    }

    #[test]
    fn return_pops_top_of_stack() {
        let c = return_constant(42.0);
        let mut vm = Vm::new();
        let mut heap = Heap::new();
        assert_eq!(vm.interpret(&c, &mut heap).unwrap(), Value::Number(42.0));
    }

    #[test]
    fn negate_flips_sign_of_top() {
        let mut c = Chunk::new();
        let idx = c.add_constant(Value::Number(1.2));
        c.write_op(OpCode::Constant, 1);
        c.write_byte(idx, 1);
        c.write_op(OpCode::Negate, 1);
        c.write_op(OpCode::Return, 1);

        let mut vm = Vm::new();
        let mut heap = Heap::new();
        assert_eq!(vm.interpret(&c, &mut heap).unwrap(), Value::Number(-1.2));
    }

    /// Reproduces the chapter 15 reference fragment `1 + 2 = 3`.
    #[test]
    fn add_two_constants() {
        let mut c = Chunk::new();
        let one = c.add_constant(Value::Number(1.0));
        let two = c.add_constant(Value::Number(2.0));
        c.write_op(OpCode::Constant, 1);
        c.write_byte(one, 1);
        c.write_op(OpCode::Constant, 1);
        c.write_byte(two, 1);
        c.write_op(OpCode::Add, 1);
        c.write_op(OpCode::Return, 1);

        let mut vm = Vm::new();
        let mut heap = Heap::new();
        assert_eq!(vm.interpret(&c, &mut heap).unwrap(), Value::Number(3.0));
    }

    /// `(1 + 2) * 3 - -4` exercises every arithmetic op + negation in
    /// one chunk. Stack ordering matters: the right-hand operand is
    /// pushed last and popped first, so emitting in source order yields
    /// the correct result.
    #[test]
    fn arithmetic_precedence_via_stack_ordering() {
        // Bytecode for `(1 + 2) * 3 - (-4)` evaluating to `13`.
        let mut c = Chunk::new();
        let one = c.add_constant(Value::Number(1.0));
        let two = c.add_constant(Value::Number(2.0));
        let three = c.add_constant(Value::Number(3.0));
        let four = c.add_constant(Value::Number(4.0));

        c.write_op(OpCode::Constant, 1);
        c.write_byte(one, 1);
        c.write_op(OpCode::Constant, 1);
        c.write_byte(two, 1);
        c.write_op(OpCode::Add, 1); // stack: [3]
        c.write_op(OpCode::Constant, 1);
        c.write_byte(three, 1);
        c.write_op(OpCode::Multiply, 1); // stack: [9]
        c.write_op(OpCode::Constant, 1);
        c.write_byte(four, 1);
        c.write_op(OpCode::Negate, 1); // stack: [9, -4]
        c.write_op(OpCode::Subtract, 1); // stack: [13]
        c.write_op(OpCode::Return, 1);

        let mut vm = Vm::new();
        let mut heap = Heap::new();
        assert_eq!(vm.interpret(&c, &mut heap).unwrap(), Value::Number(13.0));
    }

    #[test]
    fn divide_by_zero_yields_ieee_infinity() {
        // Lox follows IEEE-754 here: dividing a positive number by zero
        // yields `+inf` rather than a runtime error. clox does the same
        // (no NaN check on `/`).
        let mut c = Chunk::new();
        let one = c.add_constant(Value::Number(1.0));
        let zero = c.add_constant(Value::Number(0.0));
        c.write_op(OpCode::Constant, 1);
        c.write_byte(one, 1);
        c.write_op(OpCode::Constant, 1);
        c.write_byte(zero, 1);
        c.write_op(OpCode::Divide, 1);
        c.write_op(OpCode::Return, 1);

        let mut vm = Vm::new();
        let mut heap = Heap::new();
        let Value::Number(n) = vm.interpret(&c, &mut heap).unwrap() else {
            panic!("expected Number");
        };
        assert!(n.is_infinite() && n.is_sign_positive(), "got {n}");
    }

    /// Chapter 15's signature trace output. Expected text is the
    /// clox-format trace: stack on one line, disassembly on the next,
    /// for every instruction including the final `OP_RETURN`.
    #[test]
    fn debug_trace_emits_clox_format() {
        let c = return_constant(1.2);

        let mut trace = String::new();
        let mut vm = Vm::new();
        let mut heap = Heap::new();
        vm.interpret_with_trace(&c, &mut heap, &mut trace).unwrap();

        // Built with explicit `\n`s rather than a multi-line string
        // literal so the trailing whitespace on the empty-stack line
        // (10 leading spaces, no `[ ... ]`) is unambiguous.
        let expected = concat!(
            "          \n",
            "0000    1 OP_CONSTANT         0 '1.2'\n",
            "          [ 1.2 ]\n",
            "0002    | OP_RETURN\n",
        );
        assert_eq!(trace, expected);
    }

    #[test]
    fn vm_can_be_reused_across_chunks() {
        let mut vm = Vm::new();
        let mut heap = Heap::new();
        assert_eq!(
            vm.interpret(&return_constant(1.0), &mut heap).unwrap(),
            Value::Number(1.0)
        );
        assert_eq!(
            vm.interpret(&return_constant(2.0), &mut heap).unwrap(),
            Value::Number(2.0)
        );
        // Stack must have been drained between programs.
        assert!(vm.stack.is_empty());
    }

    #[test]
    #[should_panic(expected = "stack underflow")]
    fn empty_chunk_with_only_return_panics_on_underflow() {
        // OP_RETURN with no value pushed first — clearly malformed
        // bytecode. The VM's invariant is "a well-formed chunk always
        // leaves something on top of the stack before OP_RETURN", so we
        // panic rather than return a runtime error.
        let mut c = Chunk::new();
        c.write_op(OpCode::Return, 1);
        let mut vm = Vm::new();
        let mut heap = Heap::new();
        let _ = vm.interpret(&c, &mut heap);
    }
}
