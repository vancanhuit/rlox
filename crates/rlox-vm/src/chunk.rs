//! Chapter 14 — Chunks of Bytecode.
//!
//! A [`Chunk`] is a unit of compiled Lox bytecode plus the metadata
//! needed to disassemble or trace it: the linear `code` byte stream,
//! the `constants` pool addressed by `OP_CONSTANT`, and per-instruction
//! source-line numbers stored in run-length-encoded form.
//!
//! Reference: clox `chunk.h` / `chunk.c`. Differences from clox:
//!
//! - `OpCode` is a Rust `enum` rather than a C `enum`/macro pair, so the
//!   compiler enforces exhaustive matches and `#[repr(u8)]` keeps the
//!   on-disk encoding identical to the book.
//! - Line numbers use a [`LineRle`] vector of `(line, run_length)`
//!   pairs (clox's "challenge" RLE), which costs `O(log instructions)`
//!   to look up but keeps the table tiny for tight loops.
//! - The constant pool is a `Vec<Value>` with a `u8` index, matching
//!   clox's chapter-14 limit of 256 constants per chunk. Chapter 14
//!   challenge: lift to `OP_CONSTANT_LONG` with a 24-bit index — left
//!   for a later phase if needed.

use crate::value::Value;

/// Bytecode operations recognised by the VM.
///
/// `#[repr(u8)]` makes the discriminant the on-disk encoding. The
/// numeric values are deliberately stable: `OP_RETURN` is opcode `0` so
/// a freshly-zeroed `Chunk::code` would default to "halt", matching
/// clox's defensive layout.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpCode {
    /// `OP_CONSTANT <idx>` — push `chunk.constants[idx]` onto the stack.
    Constant = 0x01,
    /// `OP_RETURN` — return from the current function (or end the
    /// top-level program in chapter 14, which has no functions yet).
    Return = 0x02,
}

impl OpCode {
    /// Decode a raw opcode byte. Returns `None` for unknown values so
    /// the disassembler can render `?? <byte>` instead of panicking on
    /// malformed input.
    #[must_use]
    pub const fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::Constant),
            0x02 => Some(Self::Return),
            _ => None,
        }
    }

    /// Human-readable mnemonic, matching clox's `disassembleInstruction`.
    #[must_use]
    pub const fn mnemonic(self) -> &'static str {
        match self {
            Self::Constant => "OP_CONSTANT",
            Self::Return => "OP_RETURN",
        }
    }
}

/// Line-number table compressed with simple run-length encoding.
///
/// Each entry is `(source_line, run_length)`: a run of `run_length`
/// consecutive bytecode bytes shares `source_line`. The chapter 14
/// challenge calls for this layout because tight loops emit thousands
/// of bytes from one source line and naïve `Vec<usize>` parallel to
/// `code` is wasteful.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LineRle {
    runs: Vec<(usize, usize)>,
}

impl LineRle {
    /// Record one more byte at `line`, extending the current run if it
    /// matches the most recently recorded line.
    pub fn push(&mut self, line: usize) {
        match self.runs.last_mut() {
            Some((prev_line, len)) if *prev_line == line => *len += 1,
            _ => self.runs.push((line, 1)),
        }
    }

    /// Look up the source line for `byte_offset` into the bytecode.
    /// Returns `None` if `byte_offset` is past the recorded length.
    #[must_use]
    pub fn line_at(&self, byte_offset: usize) -> Option<usize> {
        let mut cursor = 0;
        for &(line, len) in &self.runs {
            if byte_offset < cursor + len {
                return Some(line);
            }
            cursor += len;
        }
        None
    }

    /// Total number of bytes covered by the table.
    #[must_use]
    pub fn len(&self) -> usize {
        self.runs.iter().map(|&(_, n)| n).sum()
    }

    /// `true` when no bytes have been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}

/// A compiled unit of Lox bytecode.
///
/// Chapter 14 keeps a `Chunk` simple: a flat byte stream, a constant
/// pool, and a side table from byte offset to source line. Later
/// chapters will hang more metadata off it (e.g. function names in
/// chapter 24).
#[derive(Debug, Default, Clone)]
pub struct Chunk {
    /// Linear byte-encoded instruction stream.
    pub code: Vec<u8>,
    /// Constant pool, addressed by a `u8` operand (max 256 entries).
    pub constants: Vec<Value>,
    /// Run-length-encoded source-line table parallel to `code`.
    pub lines: LineRle,
}

impl Chunk {
    /// A new, empty chunk.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a raw byte to the bytecode stream and tag it with `line`.
    ///
    /// Use this for opcode operands; for opcodes themselves prefer the
    /// type-safe [`Self::write_op`].
    pub fn write_byte(&mut self, byte: u8, line: usize) {
        self.code.push(byte);
        self.lines.push(line);
    }

    /// Append an opcode to the bytecode stream.
    pub fn write_op(&mut self, op: OpCode, line: usize) {
        self.write_byte(op as u8, line);
    }

    /// Append `value` to the constant pool and return its `u8` index.
    ///
    /// # Panics
    ///
    /// Panics if the constant pool already holds 256 entries; the
    /// chapter 14 encoding can't address more than that. Chapter 14's
    /// challenge problem (`OP_CONSTANT_LONG`) lifts the limit but is
    /// out of scope for this PR.
    pub fn add_constant(&mut self, value: Value) -> u8 {
        let idx = self.constants.len();
        assert!(
            idx < u8::MAX as usize + 1,
            "constant pool exceeds OP_CONSTANT's u8 operand range; \
             OP_CONSTANT_LONG (chapter 14 challenge) not yet implemented",
        );
        self.constants.push(value);
        u8::try_from(idx).expect("bounds-checked above")
    }

    /// Number of bytes in the bytecode stream.
    #[must_use]
    pub fn len(&self) -> usize {
        self.code.len()
    }

    /// `true` when no bytecode has been written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcode_round_trips_through_byte() {
        for op in [OpCode::Constant, OpCode::Return] {
            assert_eq!(OpCode::from_byte(op as u8), Some(op));
        }
    }

    #[test]
    fn opcode_from_byte_rejects_unknown() {
        assert_eq!(OpCode::from_byte(0xff), None);
    }

    #[test]
    fn write_op_appends_one_byte_and_records_line() {
        let mut c = Chunk::new();
        c.write_op(OpCode::Return, 7);
        assert_eq!(c.code, vec![OpCode::Return as u8]);
        assert_eq!(c.lines.line_at(0), Some(7));
    }

    #[test]
    fn write_byte_extends_an_existing_line_run() {
        // OP_CONSTANT + operand on the same source line should compress
        // into a single (line, len=2) run.
        let mut c = Chunk::new();
        let idx = c.add_constant(Value::Number(1.2));
        c.write_op(OpCode::Constant, 123);
        c.write_byte(idx, 123);
        assert_eq!(c.lines.line_at(0), Some(123));
        assert_eq!(c.lines.line_at(1), Some(123));
        // Internal: only one RLE run was recorded.
        assert_eq!(c.lines.runs, vec![(123, 2)]);
    }

    #[test]
    fn line_rle_distinguishes_runs() {
        let mut t = LineRle::default();
        t.push(1);
        t.push(1);
        t.push(2);
        t.push(3);
        t.push(3);
        assert_eq!(t.runs, vec![(1, 2), (2, 1), (3, 2)]);
        assert_eq!(t.line_at(0), Some(1));
        assert_eq!(t.line_at(1), Some(1));
        assert_eq!(t.line_at(2), Some(2));
        assert_eq!(t.line_at(3), Some(3));
        assert_eq!(t.line_at(4), Some(3));
        assert_eq!(t.line_at(5), None);
    }

    #[test]
    fn add_constant_returns_increasing_indices() {
        let mut c = Chunk::new();
        assert_eq!(c.add_constant(Value::Number(1.0)), 0);
        assert_eq!(c.add_constant(Value::Number(2.0)), 1);
        assert_eq!(c.add_constant(Value::Number(3.0)), 2);
        assert_eq!(c.constants.len(), 3);
    }

    #[test]
    #[should_panic(expected = "OP_CONSTANT's u8 operand range")]
    fn add_constant_panics_past_u8_max() {
        let mut c = Chunk::new();
        for i in 0..=256u32 {
            c.add_constant(Value::Number(f64::from(i)));
        }
    }

    #[test]
    fn opcode_mnemonic_matches_clox() {
        assert_eq!(OpCode::Constant.mnemonic(), "OP_CONSTANT");
        assert_eq!(OpCode::Return.mnemonic(), "OP_RETURN");
    }
}
