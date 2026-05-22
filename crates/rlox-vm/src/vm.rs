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
//!   Lox itself imposes no statically-known upper bound now that
//!   recursion and nested function calls exist, so a growable stack is
//!   simpler and still cheap.
//! - The instruction pointer is a `usize` index into `chunk.code`
//!   instead of a raw `*const u8`. The borrow checker objects to the
//!   pointer-walking style and the index version compiles to the same
//!   code under release optimisations.
//! - `OP_PRINT` writes to a configurable [`std::io::Write`] sink instead
//!   of directly to stdout, so tests and embedders can capture output.
//! - clox's `DEBUG_TRACE_EXECUTION` macro is replaced by a runtime
//!   trace sink: see [`Vm::interpret_with_trace`].
//!
//! Stack underflow is a compiler bug, not a runtime error. The VM
//! [`assert!`]s instead of returning a `Result`, mirroring clox's
//! "we don't recover from a malformed chunk" stance.

use std::collections::HashMap;
use std::fmt::{self, Write as FmtWrite};
use std::io::Write as IoWrite;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::chunk::{Chunk, OpCode};
use crate::disassembler::disassemble_instruction;
use crate::heap::{Handle, Heap, NativeFunction, Obj};
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
/// cleared between programs. Callers that reuse a VM must also reuse
/// the same [`Heap`] because global values may contain object handles
/// into that heap.
#[derive(Debug, Default)]
pub struct Vm {
    /// Operand stack. Drained on `OP_RETURN`.
    stack: Vec<Value>,
    /// Global variables, keyed by identifier text. This intentionally
    /// uses Rust's standard hash table instead of clox's custom chapter
    /// 20 table.
    globals: HashMap<String, Value>,
    /// Active call frames. A script frame borrows the chunk passed to
    /// `interpret`; function frames point at heap-allocated function
    /// objects.
    frames: Vec<CallFrame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameFunction {
    Script,
    Function(Handle),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CallFrame {
    function: FrameFunction,
    ip: usize,
    slot_start: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Continue,
    Done,
}

impl Vm {
    /// A fresh VM with an empty stack.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `chunk` to completion, writing any `print` output to `out`.
    ///
    /// # Errors
    ///
    /// Returns [`VmError::Runtime`] for runtime faults such as type
    /// errors, undefined globals, or output errors.
    ///
    /// # Panics
    ///
    /// Panics on malformed bytecode (stack underflow, dangling operand,
    /// unknown opcode, missing terminating `OP_RETURN`). These are
    /// compiler bugs, not runtime errors, and matching clox we abort
    /// rather than try to recover.
    pub fn interpret(
        &mut self,
        chunk: &Chunk,
        heap: &mut Heap,
        out: &mut dyn IoWrite,
    ) -> VmResult<()> {
        self.run(chunk, heap, out, None::<&mut String>)
    }

    /// Like [`Self::interpret`], but write a clox-style execution trace
    /// to `trace`. Each step prints the current stack contents on one
    /// line and the disassembled instruction on the next, matching the
    /// book's `DEBUG_TRACE_EXECUTION` output.
    ///
    /// # Errors
    ///
    /// Same as [`Self::interpret`].
    pub fn interpret_with_trace<W: FmtWrite>(
        &mut self,
        chunk: &Chunk,
        heap: &mut Heap,
        out: &mut dyn IoWrite,
        trace: &mut W,
    ) -> VmResult<()> {
        self.run(chunk, heap, out, Some(trace))
    }

    fn run<W: FmtWrite>(
        &mut self,
        chunk: &Chunk,
        heap: &mut Heap,
        out: &mut dyn IoWrite,
        mut trace: Option<&mut W>,
    ) -> VmResult<()> {
        self.ensure_natives(heap);

        // Reset the stack between programs so a `Vm` can be reused. We
        // reuse the allocation rather than reallocating.
        self.stack.clear();
        self.frames.clear();
        self.frames.push(CallFrame {
            function: FrameFunction::Script,
            ip: 0,
            slot_start: 0,
        });

        loop {
            let frame_index = self
                .frames
                .len()
                .checked_sub(1)
                .expect("VM must always have an active call frame");
            if let Some(out) = trace.as_deref_mut() {
                let ip = self.frames[frame_index].ip;
                let frame_chunk = self.frame_chunk(chunk, heap, frame_index);
                trace_step(frame_chunk, ip, &self.stack, out);
            }

            // The opcode lives at `op_ip`; chapter 18 introduces runtime
            // errors which need to attribute themselves to the *opcode's*
            // source line, not the operand byte that follows.
            let op_ip = self.frames[frame_index].ip;
            let line = self.frame_line(chunk, heap, frame_index, op_ip);
            let byte = self.read_byte(chunk, heap, frame_index);
            let op = OpCode::from_byte(byte)
                .unwrap_or_else(|| panic!("unknown opcode {byte:#04x} at offset {op_ip} in chunk"));

            if self.execute_op(op, chunk, heap, out, frame_index, line)? == Step::Done {
                return Ok(());
            }
        }
    }

    fn execute_op(
        &mut self,
        op: OpCode,
        chunk: &Chunk,
        heap: &mut Heap,
        out: &mut dyn IoWrite,
        frame_index: usize,
        line: usize,
    ) -> VmResult<Step> {
        match op {
            OpCode::Constant => {
                let value = self.read_constant(chunk, heap, frame_index);
                self.push(value);
            }
            OpCode::Pop => {
                self.pop();
            }
            OpCode::Print => {
                let value = self.pop();
                writeln!(out, "{}", value.display(heap))
                    .map_err(|e| Self::runtime_error_at(line, &e.to_string()))?;
            }
            OpCode::DefineGlobal => {
                let name = self.read_string_constant(chunk, heap, frame_index);
                self.globals.insert(name, self.peek(0));
                self.pop();
            }
            OpCode::GetGlobal => self.get_global(chunk, heap, frame_index, line)?,
            OpCode::SetGlobal => self.set_global(chunk, heap, frame_index, line)?,
            OpCode::GetLocal => {
                let slot = usize::from(self.read_byte(chunk, heap, frame_index));
                let absolute = self.frames[frame_index].slot_start + slot;
                self.push(self.stack[absolute]);
            }
            OpCode::SetLocal => {
                let slot = usize::from(self.read_byte(chunk, heap, frame_index));
                let absolute = self.frames[frame_index].slot_start + slot;
                self.stack[absolute] = self.peek(0);
            }
            OpCode::Jump => {
                let jump = usize::from(self.read_short(chunk, heap, frame_index));
                self.frames[frame_index].ip += jump;
            }
            OpCode::JumpIfFalse => {
                let jump = usize::from(self.read_short(chunk, heap, frame_index));
                if !self.peek(0).is_truthy() {
                    self.frames[frame_index].ip += jump;
                }
            }
            OpCode::Loop => {
                let jump = usize::from(self.read_short(chunk, heap, frame_index));
                self.frames[frame_index].ip -= jump;
            }
            OpCode::Call => {
                let arg_count = self.read_byte(chunk, heap, frame_index);
                let callee = self.peek(usize::from(arg_count));
                self.call_value(callee, arg_count, heap, line)?;
            }
            OpCode::Nil => self.push(Value::Nil),
            OpCode::True => self.push(Value::Bool(true)),
            OpCode::False => self.push(Value::Bool(false)),
            OpCode::Not => {
                let v = self.pop();
                self.push(Value::Bool(!v.is_truthy()));
            }
            OpCode::Equal => {
                let b = self.pop();
                let a = self.pop();
                self.push(Value::Bool(values_equal(a, b, heap)));
            }
            OpCode::Greater => self.binary_cmp(line, |a, b| a > b)?,
            OpCode::Less => self.binary_cmp(line, |a, b| a < b)?,
            OpCode::Negate => self.negate(line)?,
            OpCode::Add => self.op_add(line, heap)?,
            OpCode::Subtract => self.binary_arith(line, |a, b| a - b)?,
            OpCode::Multiply => self.binary_arith(line, |a, b| a * b)?,
            OpCode::Divide => self.binary_arith(line, |a, b| a / b)?,
            OpCode::Return => return Ok(self.return_from_frame(frame_index)),
        }
        Ok(Step::Continue)
    }

    fn push(&mut self, v: Value) {
        self.stack.push(v);
    }

    fn pop(&mut self) -> Value {
        self.stack
            .pop()
            .expect("stack underflow: malformed bytecode (compiler bug)")
    }

    fn peek(&self, distance: usize) -> Value {
        self.stack[self.stack.len() - 1 - distance]
    }

    fn get_global(
        &mut self,
        chunk: &Chunk,
        heap: &Heap,
        frame_index: usize,
        line: usize,
    ) -> VmResult<()> {
        let name = self.read_string_constant(chunk, heap, frame_index);
        let Some(value) = self.globals.get(&name).copied() else {
            return Err(Self::runtime_error_at(
                line,
                &format!("Undefined variable '{name}'."),
            ));
        };
        self.push(value);
        Ok(())
    }

    fn set_global(
        &mut self,
        chunk: &Chunk,
        heap: &Heap,
        frame_index: usize,
        line: usize,
    ) -> VmResult<()> {
        let name = self.read_string_constant(chunk, heap, frame_index);
        if !self.globals.contains_key(&name) {
            return Err(Self::runtime_error_at(
                line,
                &format!("Undefined variable '{name}'."),
            ));
        }
        self.globals.insert(name, self.peek(0));
        Ok(())
    }

    fn negate(&mut self, line: usize) -> VmResult<()> {
        let Value::Number(n) = self.pop() else {
            return Err(Self::runtime_error_at(line, "Operand must be a number."));
        };
        self.push(Value::Number(-n));
        Ok(())
    }

    fn return_from_frame(&mut self, frame_index: usize) -> Step {
        if self.frames[frame_index].function == FrameFunction::Script {
            self.frames.clear();
            return Step::Done;
        }

        let result = self.pop();
        let slot_start = self.frames[frame_index].slot_start;
        self.frames.pop();
        self.stack.truncate(slot_start);
        self.push(result);
        Step::Continue
    }

    fn frame_chunk<'a>(
        &self,
        script_chunk: &'a Chunk,
        heap: &'a Heap,
        frame_index: usize,
    ) -> &'a Chunk {
        match self.frames[frame_index].function {
            FrameFunction::Script => script_chunk,
            FrameFunction::Function(handle) => {
                &heap
                    .as_function(handle)
                    .expect("call frame points at a non-function object")
                    .chunk
            }
        }
    }

    fn frame_line(
        &self,
        script_chunk: &Chunk,
        heap: &Heap,
        frame_index: usize,
        op_ip: usize,
    ) -> usize {
        self.frame_chunk(script_chunk, heap, frame_index)
            .lines
            .line_at(op_ip)
            .unwrap_or(0)
    }

    fn read_byte(&mut self, script_chunk: &Chunk, heap: &Heap, frame_index: usize) -> u8 {
        let ip = self.frames[frame_index].ip;
        let byte = self.frame_chunk(script_chunk, heap, frame_index).code[ip];
        self.frames[frame_index].ip = ip + 1;
        byte
    }

    fn read_constant(&mut self, script_chunk: &Chunk, heap: &Heap, frame_index: usize) -> Value {
        let idx = usize::from(self.read_byte(script_chunk, heap, frame_index));
        self.frame_chunk(script_chunk, heap, frame_index).constants[idx]
    }

    fn read_string_constant(
        &mut self,
        script_chunk: &Chunk,
        heap: &Heap,
        frame_index: usize,
    ) -> String {
        let value = self.read_constant(script_chunk, heap, frame_index);
        let Value::Obj(handle) = value else {
            panic!("global name operand does not reference a string object");
        };
        heap.as_str(handle)
            .expect("global name operand references non-string object")
            .to_owned()
    }

    fn read_short(&mut self, script_chunk: &Chunk, heap: &Heap, frame_index: usize) -> u16 {
        let high = u16::from(self.read_byte(script_chunk, heap, frame_index));
        let low = u16::from(self.read_byte(script_chunk, heap, frame_index));
        (high << 8) | low
    }

    fn ensure_natives(&mut self, heap: &mut Heap) {
        if self.globals.contains_key("clock") {
            return;
        }

        let handle = heap.alloc_native(NativeFunction {
            name: "clock",
            arity: 0,
            function: clock_native,
        });
        self.globals.insert("clock".to_string(), Value::Obj(handle));
    }

    fn call_value(
        &mut self,
        callee: Value,
        arg_count: u8,
        heap: &mut Heap,
        line: usize,
    ) -> VmResult<()> {
        let Value::Obj(handle) = callee else {
            return Err(Self::runtime_error_at(
                line,
                "Can only call functions and classes.",
            ));
        };

        match heap.get(handle) {
            Obj::Function(function) => {
                if arg_count != function.arity {
                    return Err(Self::arity_error(line, function.arity, arg_count));
                }
                self.frames.push(CallFrame {
                    function: FrameFunction::Function(handle),
                    ip: 0,
                    slot_start: self.stack.len() - usize::from(arg_count) - 1,
                });
                Ok(())
            }
            Obj::Native(native) => {
                if arg_count != native.arity {
                    return Err(Self::arity_error(line, native.arity, arg_count));
                }
                let callee_index = self.stack.len() - usize::from(arg_count) - 1;
                let result = (native.function)(&self.stack[callee_index + 1..]);
                self.stack.truncate(callee_index);
                self.push(result);
                Ok(())
            }
            Obj::Str(_) => Err(Self::runtime_error_at(
                line,
                "Can only call functions and classes.",
            )),
        }
    }

    fn arity_error(line: usize, expected: u8, actual: u8) -> VmError {
        Self::runtime_error_at(
            line,
            &format!("Expected {expected} arguments but got {actual}."),
        )
    }

    /// Chapter 19 `OP_ADD`: dispatches on the operand types so the
    /// same opcode covers `1 + 2` (numeric addition) and `"a" + "b"`
    /// (string concatenation). Anything else is a runtime error
    /// matching clox: `Operands must be two numbers or two strings.`
    fn op_add(&mut self, line: usize, heap: &mut Heap) -> VmResult<()> {
        let b = self.pop();
        let a = self.pop();
        match (a, b) {
            (Value::Number(a), Value::Number(b)) => {
                self.push(Value::Number(a + b));
                Ok(())
            }
            (Value::Obj(ah), Value::Obj(bh)) => {
                // Only strings concatenate. Functions and native
                // functions fall through to the same operand-type error
                // clox uses for mixed string/non-string pairs.
                let concat = match (heap.get(ah), heap.get(bh)) {
                    (Obj::Str(a), Obj::Str(b)) => {
                        let mut concat = String::with_capacity(a.len() + b.len());
                        concat.push_str(a);
                        concat.push_str(b);
                        Some(concat)
                    }
                    _ => None,
                };
                let Some(concat) = concat else {
                    return Err(Self::runtime_error_at(
                        line,
                        "Operands must be two numbers or two strings.",
                    ));
                };
                let h = heap.alloc_string(concat);
                self.push(Value::Obj(h));
                Ok(())
            }
            _ => Err(Self::runtime_error_at(
                line,
                "Operands must be two numbers or two strings.",
            )),
        }
    }

    /// Binary numeric arithmetic (`-`, `*`, `/`) that returns a number.
    /// `+` has its own helper because chapter 19 overloads it for
    /// strings.
    fn binary_arith<F: Fn(f64, f64) -> f64>(&mut self, line: usize, op: F) -> VmResult<()> {
        // `b` is popped first because it was pushed last (right-hand operand).
        let b = self.pop();
        let a = self.pop();
        let (Value::Number(a), Value::Number(b)) = (a, b) else {
            return Err(Self::runtime_error_at(line, "Operands must be numbers."));
        };
        self.push(Value::Number(op(a, b)));
        Ok(())
    }

    /// Binary numeric comparison that returns a boolean.
    fn binary_cmp<F: Fn(f64, f64) -> bool>(&mut self, line: usize, op: F) -> VmResult<()> {
        let b = self.pop();
        let a = self.pop();
        let (Value::Number(a), Value::Number(b)) = (a, b) else {
            return Err(Self::runtime_error_at(line, "Operands must be numbers."));
        };
        self.push(Value::Bool(op(a, b)));
        Ok(())
    }

    /// Build a runtime error attributed to `line`.
    fn runtime_error_at(line: usize, message: &str) -> VmError {
        VmError::Runtime {
            line,
            message: message.to_string(),
        }
    }
}

fn clock_native(_args: &[Value]) -> Value {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64());
    Value::Number(seconds)
}

/// Format a single trace step into `out`, mimicking clox's
/// `DEBUG_TRACE_EXECUTION` block:
///
/// ```text
///           [ 1.2 ][ 3.4 ]
/// 0000  123 OP_ADD
/// ```
fn trace_step(chunk: &Chunk, ip: usize, stack: &[Value], out: &mut dyn FmtWrite) {
    // The trace's stack rendering uses the bare Display (not the
    // heap-aware adapter), so any object handle prints as `<obj#N>`.
    // The book's chapter-15 trace tests still pass because they only
    // exercise numeric values; heap-aware output is tested through the
    // public execution surfaces.
    let _ = trace_inner(chunk, ip, stack, out);
}

fn trace_inner(chunk: &Chunk, ip: usize, stack: &[Value], out: &mut dyn FmtWrite) -> fmt::Result {
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

    /// Helper: build a chunk that prints `n` and immediately returns.
    fn print_constant(n: f64) -> Chunk {
        let mut c = Chunk::new();
        let idx = c.add_constant(Value::Number(n));
        c.write_op(OpCode::Constant, 1);
        c.write_byte(idx, 1);
        c.write_op(OpCode::Print, 1);
        c.write_op(OpCode::Return, 1);
        c
    }

    fn run_chunk(c: &Chunk, heap: &mut Heap) -> String {
        let mut vm = Vm::new();
        let mut out = Vec::new();
        vm.interpret(c, heap, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn print_pops_top_of_stack() {
        let c = print_constant(42.0);
        let mut heap = Heap::new();
        assert_eq!(run_chunk(&c, &mut heap), "42\n");
    }

    #[test]
    fn negate_flips_sign_of_top() {
        let mut c = Chunk::new();
        let idx = c.add_constant(Value::Number(1.2));
        c.write_op(OpCode::Constant, 1);
        c.write_byte(idx, 1);
        c.write_op(OpCode::Negate, 1);
        c.write_op(OpCode::Print, 1);
        c.write_op(OpCode::Return, 1);

        let mut heap = Heap::new();
        assert_eq!(run_chunk(&c, &mut heap), "-1.2\n");
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
        c.write_op(OpCode::Print, 1);
        c.write_op(OpCode::Return, 1);

        let mut heap = Heap::new();
        assert_eq!(run_chunk(&c, &mut heap), "3\n");
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
        c.write_op(OpCode::Print, 1);
        c.write_op(OpCode::Return, 1);

        let mut heap = Heap::new();
        assert_eq!(run_chunk(&c, &mut heap), "13\n");
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
        c.write_op(OpCode::Print, 1);
        c.write_op(OpCode::Return, 1);

        let mut heap = Heap::new();
        assert_eq!(run_chunk(&c, &mut heap), "inf\n");
    }

    /// Chapter 15's signature trace output. Expected text is the
    /// clox-format trace: stack on one line, disassembly on the next,
    /// for every instruction including the final `OP_RETURN`.
    #[test]
    fn debug_trace_emits_clox_format() {
        let c = print_constant(1.2);

        let mut trace = String::new();
        let mut vm = Vm::new();
        let mut heap = Heap::new();
        let mut out = Vec::new();
        vm.interpret_with_trace(&c, &mut heap, &mut out, &mut trace)
            .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "1.2\n");

        // Built with explicit `\n`s rather than a multi-line string
        // literal so the trailing whitespace on the empty-stack line
        // (10 leading spaces, no `[ ... ]`) is unambiguous.
        let expected = concat!(
            "          \n",
            "0000    1 OP_CONSTANT         0 '1.2'\n",
            "          [ 1.2 ]\n",
            "0002    | OP_PRINT\n",
            "          \n",
            "0003    | OP_RETURN\n",
        );
        assert_eq!(trace, expected);
    }

    #[test]
    fn vm_can_be_reused_across_chunks() {
        let mut vm = Vm::new();
        let mut heap = Heap::new();
        let mut out = Vec::new();
        vm.interpret(&print_constant(1.0), &mut heap, &mut out)
            .unwrap();
        vm.interpret(&print_constant(2.0), &mut heap, &mut out)
            .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "1\n2\n");
        // Stack must have been drained between programs.
        assert!(vm.stack.is_empty());
    }

    #[test]
    fn empty_chunk_with_only_return_exits_cleanly() {
        let mut c = Chunk::new();
        c.write_op(OpCode::Return, 1);
        let mut vm = Vm::new();
        let mut heap = Heap::new();
        let mut out = Vec::new();
        vm.interpret(&c, &mut heap, &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    #[should_panic(expected = "stack underflow")]
    fn pop_on_empty_stack_panics_on_underflow() {
        let mut c = Chunk::new();
        c.write_op(OpCode::Pop, 1);
        c.write_op(OpCode::Return, 1);
        let mut vm = Vm::new();
        let mut heap = Heap::new();
        let mut out = Vec::new();
        let _ = vm.interpret(&c, &mut heap, &mut out);
    }

    #[test]
    fn local_get_and_set_read_stack_slots() {
        let mut c = Chunk::new();
        let one = c.add_constant(Value::Number(1.0));
        let two = c.add_constant(Value::Number(2.0));
        c.write_op(OpCode::Constant, 1);
        c.write_byte(one, 1);
        c.write_op(OpCode::Constant, 1);
        c.write_byte(two, 1);
        c.write_op(OpCode::SetLocal, 1);
        c.write_byte(0, 1);
        c.write_op(OpCode::Pop, 1);
        c.write_op(OpCode::GetLocal, 1);
        c.write_byte(0, 1);
        c.write_op(OpCode::Print, 1);
        c.write_op(OpCode::Pop, 1);
        c.write_op(OpCode::Return, 1);

        let mut heap = Heap::new();
        assert_eq!(run_chunk(&c, &mut heap), "2\n");
    }

    #[test]
    fn jump_skips_forward_by_u16_operand() {
        let mut c = Chunk::new();
        let one = c.add_constant(Value::Number(1.0));
        c.write_op(OpCode::Jump, 1);
        c.write_byte(0, 1);
        c.write_byte(3, 1);
        c.write_op(OpCode::Constant, 1);
        c.write_byte(one, 1);
        c.write_op(OpCode::Print, 1);
        c.write_op(OpCode::Return, 1);

        let mut heap = Heap::new();
        assert_eq!(run_chunk(&c, &mut heap), "");
    }

    #[test]
    fn jump_if_false_and_loop_move_instruction_pointer() {
        let mut c = Chunk::new();
        let zero = c.add_constant(Value::Number(0.0));
        let one = c.add_constant(Value::Number(1.0));
        let two = c.add_constant(Value::Number(2.0));

        c.write_op(OpCode::Constant, 1);
        c.write_byte(zero, 1);
        c.write_op(OpCode::GetLocal, 1);
        c.write_byte(0, 1);
        c.write_op(OpCode::Constant, 1);
        c.write_byte(two, 1);
        c.write_op(OpCode::Less, 1);
        c.write_op(OpCode::JumpIfFalse, 1);
        c.write_byte(0, 1);
        c.write_byte(15, 1);
        c.write_op(OpCode::Pop, 1);
        c.write_op(OpCode::GetLocal, 1);
        c.write_byte(0, 1);
        c.write_op(OpCode::Print, 1);
        c.write_op(OpCode::GetLocal, 1);
        c.write_byte(0, 1);
        c.write_op(OpCode::Constant, 1);
        c.write_byte(one, 1);
        c.write_op(OpCode::Add, 1);
        c.write_op(OpCode::SetLocal, 1);
        c.write_byte(0, 1);
        c.write_op(OpCode::Pop, 1);
        c.write_op(OpCode::Loop, 1);
        c.write_byte(0, 1);
        c.write_byte(23, 1);
        c.write_op(OpCode::Pop, 1);
        c.write_op(OpCode::Pop, 1);
        c.write_op(OpCode::Return, 1);

        let mut heap = Heap::new();
        assert_eq!(run_chunk(&c, &mut heap), "0\n1\n");
    }
}
