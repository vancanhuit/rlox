//! Chapter 14 — Disassembler.
//!
//! An offline pretty-printer for [`Chunk`]s. The output format mirrors
//! clox's `disassembleChunk` / `disassembleInstruction` so the reference
//! traces in the book line up byte-for-byte:
//!
//! ```text
//! == test chunk ==
//! 0000  123 OP_CONSTANT         0 '1.2'
//! 0002    | OP_RETURN
//! ```
//!
//! - Column 1 is the byte offset, four-digit zero-padded.
//! - Column 2 is the source line number, right-aligned in a 4-char
//!   slot, replaced by `   |` when it matches the previous instruction.
//! - Column 3 is the opcode mnemonic, left-padded to 16 chars.
//! - Trailing operand columns are opcode-specific.
//!
//! The functions write to a generic [`fmt::Write`] sink so callers can
//! capture the output into a `String` (tests) or stream it to stderr
//! (the eventual `--debug-trace` flag in chapter 15+).

use std::fmt::{self, Write};

use crate::chunk::{Chunk, OpCode};

/// Disassemble an entire chunk, prefixed with `== <name> ==`.
///
/// # Errors
///
/// Returns any [`fmt::Error`] from `out` unchanged.
pub fn disassemble_chunk(chunk: &Chunk, name: &str, out: &mut dyn Write) -> fmt::Result {
    writeln!(out, "== {name} ==")?;
    let mut offset = 0;
    while offset < chunk.code.len() {
        offset = disassemble_instruction(chunk, offset, out)?;
    }
    Ok(())
}

/// Disassemble one instruction at `offset`, returning the offset of the
/// next instruction (so callers can drive a loop). Unknown opcodes
/// render as `?? <byte>` and advance by 1 byte; the caller decides
/// whether to keep going.
///
/// # Errors
///
/// Returns any [`fmt::Error`] from `out` unchanged.
pub fn disassemble_instruction(
    chunk: &Chunk,
    offset: usize,
    out: &mut dyn Write,
) -> Result<usize, fmt::Error> {
    write!(out, "{offset:04} ")?;

    // Source line column: collapse repeats with `|` like clox does.
    let line = chunk.lines.line_at(offset);
    let prev_line = offset.checked_sub(1).and_then(|p| chunk.lines.line_at(p));
    if offset > 0 && line == prev_line {
        write!(out, "   | ")?;
    } else {
        match line {
            Some(n) => write!(out, "{n:4} ")?,
            None => write!(out, "   ? ")?,
        }
    }

    let byte = chunk.code[offset];
    match OpCode::from_byte(byte) {
        // Constant + operand pair: 2 bytes, emits the constant value.
        Some(op @ OpCode::Constant) => constant_instruction(op, chunk, offset, out),
        // Single-byte arithmetic and control opcodes.
        Some(
            op @ (OpCode::Negate
            | OpCode::Add
            | OpCode::Subtract
            | OpCode::Multiply
            | OpCode::Divide
            | OpCode::Return),
        ) => simple_instruction(op, offset, out),
        None => {
            writeln!(out, "?? {byte:#04x}")?;
            Ok(offset + 1)
        }
    }
}

fn simple_instruction(op: OpCode, offset: usize, out: &mut dyn Write) -> Result<usize, fmt::Error> {
    writeln!(out, "{}", op.mnemonic())?;
    Ok(offset + 1)
}

fn constant_instruction(
    op: OpCode,
    chunk: &Chunk,
    offset: usize,
    out: &mut dyn Write,
) -> Result<usize, fmt::Error> {
    let idx = chunk.code[offset + 1];
    let mnemonic = op.mnemonic();
    // clox: `printf("%-16s %4d '", name, constant);` plus `printValue`.
    let value = &chunk.constants[idx as usize];
    writeln!(out, "{mnemonic:<16} {idx:4} '{value}'")?;
    Ok(offset + 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    /// The book's chapter-14 reference example, byte-for-byte.
    #[test]
    fn disassembles_book_chapter_14_example() {
        let mut chunk = Chunk::new();
        let idx = chunk.add_constant(Value::Number(1.2));
        chunk.write_op(OpCode::Constant, 123);
        chunk.write_byte(idx, 123);
        chunk.write_op(OpCode::Return, 123);

        let mut out = String::new();
        disassemble_chunk(&chunk, "test chunk", &mut out).unwrap();

        let expected = "\
== test chunk ==
0000  123 OP_CONSTANT         0 '1.2'
0002    | OP_RETURN
";
        assert_eq!(out, expected);
    }

    #[test]
    fn line_column_marks_repeats_with_pipe() {
        let mut chunk = Chunk::new();
        chunk.write_op(OpCode::Return, 5);
        chunk.write_op(OpCode::Return, 5);
        chunk.write_op(OpCode::Return, 6);

        let mut out = String::new();
        disassemble_chunk(&chunk, "lines", &mut out).unwrap();

        let expected = "\
== lines ==
0000    5 OP_RETURN
0001    | OP_RETURN
0002    6 OP_RETURN
";
        assert_eq!(out, expected);
    }

    #[test]
    fn unknown_opcode_renders_with_hex_byte_and_advances_one() {
        let mut chunk = Chunk::new();
        chunk.write_byte(0xab, 1);
        chunk.write_op(OpCode::Return, 1);

        let mut out = String::new();
        disassemble_chunk(&chunk, "bad", &mut out).unwrap();

        assert!(out.contains("?? 0xab"), "stdout was: {out}");
        // The disassembler must continue past the unknown byte and find
        // the trailing OP_RETURN, otherwise an upstream bug stalls.
        assert!(out.contains("OP_RETURN"));
    }

    #[test]
    fn disassemble_instruction_returns_next_offset() {
        let mut chunk = Chunk::new();
        let idx = chunk.add_constant(Value::Number(42.0));
        chunk.write_op(OpCode::Constant, 1);
        chunk.write_byte(idx, 1);
        chunk.write_op(OpCode::Return, 2);

        let mut out = String::new();
        let next = disassemble_instruction(&chunk, 0, &mut out).unwrap();
        // OP_CONSTANT is 2 bytes (opcode + operand).
        assert_eq!(next, 2);

        let mut out = String::new();
        let next = disassemble_instruction(&chunk, 2, &mut out).unwrap();
        // OP_RETURN is 1 byte.
        assert_eq!(next, 3);
    }
}
