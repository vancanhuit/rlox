//! Chapter 17 — Compiling Expressions.
//!
//! A single-pass Pratt parser that consumes tokens from the lazy
//! [`rlox_shared::Scanner`] (chapter 16) and emits a [`Chunk`] of
//! bytecode. There is no intermediate AST: each grammar rule directly
//! emits opcodes as it recognises operators, mirroring clox's
//! `compiler.c`.
//!
//! Only expressions are compiled in this chapter. Statements, locals,
//! and globals arrive in chapters 21–22.
//!
//! Pratt actions are encoded as a [`ParseFn`] enum (rather than as
//! function pointers) and resolved through [`Compiler::run_parse_fn`];
//! the borrow checker is happier when dispatch goes through a `match`
//! on `&mut self` rather than through closures or `fn` items.
//!
//! Errors accumulate into a `Vec<LoxError>`. When the parser hits one,
//! it enters "panic mode" and suppresses subsequent diagnostics until
//! a synchronisation point — at chapter 17 that's just `EOF`; later
//! chapters will sync on statement boundaries.

use rlox_shared::error::LoxError;
use rlox_shared::scanner::Scanner;
use rlox_shared::token::{Literal, Token, TokenType};

use crate::chunk::{Chunk, OpCode};
use crate::heap::Heap;
use crate::value::Value;

/// Compile `source` into a self-contained [`Chunk`] terminated with
/// [`OpCode::Return`], paired with the [`Heap`] that backs any object
/// constants the chunk references (chapter 19: string literals).
///
/// # Errors
///
/// Returns every accumulated `LoxError` (scan errors from the lexer
/// plus parse errors from this compiler), mirroring jlox's
/// "report-everything-then-fail" strategy.
pub fn compile(source: &str) -> Result<(Chunk, Heap), Vec<LoxError>> {
    let mut c = Compiler::new(source);
    c.advance(); // prime `current`
    c.expression();
    c.consume(TokenType::Eof, "Expect end of expression.");
    c.emit_op(OpCode::Return);

    if c.errors.is_empty() {
        Ok((c.chunk, c.heap))
    } else {
        Err(c.errors)
    }
}

/// Pratt precedence ladder, lowest to highest. The numeric ordering
/// matters: `parse_precedence(p)` continues while the *next* token's
/// infix precedence is `>= p`.
///
/// Slots reserved for chapters 21+ (`Or`/`And`/`Assignment`/`Call`/
/// `Primary`) are kept commented out so the ordering remains stable as
/// we light them up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum Precedence {
    None = 0,
    // Assignment, // =
    // Or,         // or
    // And,        // and
    Equality = 4,   // == !=
    Comparison = 5, // < > <= >=
    Term = 6,       // + -
    Factor = 7,     // * /
    Unary = 8,      // ! -
}

impl Precedence {
    /// One precedence level higher than `self`. Binary operators recurse
    /// at `next_higher()` so the same operator at the same level is
    /// left-associative.
    const fn next_higher(self) -> Self {
        match self {
            Self::None => Self::Equality,
            Self::Equality => Self::Comparison,
            Self::Comparison => Self::Term,
            Self::Term => Self::Factor,
            // Nothing in chapter 18 is higher than Unary, so saturate.
            Self::Factor | Self::Unary => Self::Unary,
        }
    }
}

/// Action a Pratt rule should take. Encoded as an enum (rather than a
/// function pointer) so dispatch can take `&mut Compiler` without
/// fighting the borrow checker over closure captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseFn {
    None,
    Grouping,
    Unary,
    Binary,
    Number,
    /// Parses `nil`, `true`, `false`. The token type discriminates which
    /// `OP_NIL` / `OP_TRUE` / `OP_FALSE` opcode is emitted.
    Literal,
    /// Parses a string literal. The string body is interned into the
    /// compiler's [`Heap`] and a `OP_CONSTANT <idx>` instruction is
    /// emitted with a [`Value::Obj`] payload.
    String,
}

#[derive(Debug, Clone, Copy)]
struct ParseRule {
    prefix: ParseFn,
    infix: ParseFn,
    precedence: Precedence,
}

/// Pratt rule lookup. Mirrors clox's static `rules[]` array.
const fn rule_for(t: TokenType) -> ParseRule {
    match t {
        TokenType::LeftParen => ParseRule {
            prefix: ParseFn::Grouping,
            infix: ParseFn::None,
            precedence: Precedence::None,
        },
        TokenType::Minus => ParseRule {
            prefix: ParseFn::Unary,
            infix: ParseFn::Binary,
            precedence: Precedence::Term,
        },
        TokenType::Plus => ParseRule {
            prefix: ParseFn::None,
            infix: ParseFn::Binary,
            precedence: Precedence::Term,
        },
        TokenType::Slash | TokenType::Star => ParseRule {
            prefix: ParseFn::None,
            infix: ParseFn::Binary,
            precedence: Precedence::Factor,
        },
        TokenType::Number => ParseRule {
            prefix: ParseFn::Number,
            infix: ParseFn::None,
            precedence: Precedence::None,
        },
        // Chapter 19 — string literals.
        TokenType::String => ParseRule {
            prefix: ParseFn::String,
            infix: ParseFn::None,
            precedence: Precedence::None,
        },
        // Chapter 18 — literals.
        TokenType::Nil | TokenType::True | TokenType::False => ParseRule {
            prefix: ParseFn::Literal,
            infix: ParseFn::None,
            precedence: Precedence::None,
        },
        // Chapter 18 — logical NOT (prefix only).
        TokenType::Bang => ParseRule {
            prefix: ParseFn::Unary,
            infix: ParseFn::None,
            precedence: Precedence::None,
        },
        // Chapter 18 — equality (lower precedence than comparison).
        TokenType::EqualEqual | TokenType::BangEqual => ParseRule {
            prefix: ParseFn::None,
            infix: ParseFn::Binary,
            precedence: Precedence::Equality,
        },
        // Chapter 18 — ordering comparisons.
        TokenType::Greater | TokenType::GreaterEqual | TokenType::Less | TokenType::LessEqual => {
            ParseRule {
                prefix: ParseFn::None,
                infix: ParseFn::Binary,
                precedence: Precedence::Comparison,
            }
        }
        // Every other token has no rule yet.
        _ => ParseRule {
            prefix: ParseFn::None,
            infix: ParseFn::None,
            precedence: Precedence::None,
        },
    }
}

struct Compiler<'src> {
    scanner: Scanner<'src>,
    /// Last consumed token. Many emit-opcode sites need access to it
    /// after we've moved past it (e.g. `binary` looks up the operator
    /// it just matched), so we cache the last two tokens.
    previous: Option<Token>,
    /// One-token lookahead.
    current: Option<Token>,
    chunk: Chunk,
    /// Heap that backs any [`Value::Obj`] entries emitted into
    /// `chunk.constants`. Handed off to the VM at the end of
    /// compilation.
    heap: Heap,
    errors: Vec<LoxError>,
    /// While in panic mode, errors are discarded until we synchronise.
    panic_mode: bool,
}

impl<'src> Compiler<'src> {
    fn new(source: &'src str) -> Self {
        Self {
            scanner: Scanner::new(source),
            previous: None,
            current: None,
            chunk: Chunk::new(),
            heap: Heap::new(),
            errors: Vec::new(),
            panic_mode: false,
        }
    }

    // ---- token plumbing -------------------------------------------------

    /// Pull the next token from the scanner. Scan errors surface as
    /// parse errors here and we keep advancing until we get a valid
    /// token (or EOF), matching clox's `errorAtCurrent` loop.
    fn advance(&mut self) {
        self.previous = self.current.take();
        loop {
            match self.scanner.next() {
                Some(Ok(token)) => {
                    self.current = Some(token);
                    return;
                }
                Some(Err(scan_err)) => {
                    // A scan error is reported but the loop continues so
                    // the parser eventually gets *some* current token.
                    self.report(scan_err);
                }
                None => {
                    // Scanner is fully drained — synthesise a final EOF
                    // so downstream code can still consult `current`.
                    let line = self.scanner.line();
                    self.current = Some(Token::new(TokenType::Eof, "", None, line));
                    return;
                }
            }
        }
    }

    /// Consume `expected` or raise `message`.
    fn consume(&mut self, expected: TokenType, message: &str) {
        if self.check(expected) {
            self.advance();
            return;
        }
        self.error_at_current(message);
    }

    fn check(&self, expected: TokenType) -> bool {
        self.current.as_ref().is_some_and(|t| t.ttype == expected)
    }

    fn previous_token(&self) -> &Token {
        self.previous
            .as_ref()
            .expect("previous_token called before any advance")
    }

    fn current_token(&self) -> &Token {
        self.current
            .as_ref()
            .expect("current_token called before initial advance")
    }

    // ---- emit -----------------------------------------------------------

    fn emit_op(&mut self, op: OpCode) {
        let line = self.previous_or_current_line();
        self.chunk.write_op(op, line);
    }

    fn emit_byte(&mut self, byte: u8) {
        let line = self.previous_or_current_line();
        self.chunk.write_byte(byte, line);
    }

    fn emit_constant(&mut self, value: Value) {
        let idx = self.chunk.add_constant(value);
        self.emit_op(OpCode::Constant);
        self.emit_byte(idx);
    }

    fn previous_or_current_line(&self) -> usize {
        // Falls back to `current` when nothing has been consumed yet
        // (the very first token of the program).
        self.previous
            .as_ref()
            .or(self.current.as_ref())
            .map_or(1, |t| t.line)
    }

    // ---- Pratt core -----------------------------------------------------

    /// Parse and emit any expression. Equivalent to clox's
    /// `expression()` — kicks off at the lowest *active* precedence
    /// rung (chapter 18: equality; chapter 21+ will lower it to
    /// `Assignment`).
    fn expression(&mut self) {
        self.parse_precedence(Precedence::Equality);
    }

    /// Recursive precedence-climbing parser.
    fn parse_precedence(&mut self, prec: Precedence) {
        self.advance();

        let prefix = rule_for(self.previous_token().ttype).prefix;
        if matches!(prefix, ParseFn::None) {
            self.error_at_previous("Expect expression.");
            return;
        }
        self.run_parse_fn(prefix);

        while prec <= rule_for(self.current_token().ttype).precedence {
            self.advance();
            let infix = rule_for(self.previous_token().ttype).infix;
            self.run_parse_fn(infix);
        }
    }

    fn run_parse_fn(&mut self, f: ParseFn) {
        match f {
            ParseFn::None => {
                unreachable!("run_parse_fn(None) — caller should have checked the rule first")
            }
            ParseFn::Grouping => self.grouping(),
            ParseFn::Unary => self.unary(),
            ParseFn::Binary => self.binary(),
            ParseFn::Number => self.number(),
            ParseFn::Literal => self.literal(),
            ParseFn::String => self.string(),
        }
    }

    // ---- grammar productions --------------------------------------------

    fn literal(&mut self) {
        match self.previous_token().ttype {
            TokenType::Nil => self.emit_op(OpCode::Nil),
            TokenType::True => self.emit_op(OpCode::True),
            TokenType::False => self.emit_op(OpCode::False),
            other => unreachable!("Pratt rule for {other:?} dispatched to literal"),
        }
    }

    fn number(&mut self) {
        // The scanner attaches a `Literal::Number` to every Number
        // token (see `rlox_shared::scanner::number_literal`), and the
        // Pratt rule for Number is the only path into `number()`, so
        // anything else is a scanner bug.
        let Some(Literal::Number(n)) = self.previous_token().literal.as_ref() else {
            unreachable!("scanner attaches Literal::Number to every Number token");
        };
        self.emit_constant(Value::Number(*n));
    }

    fn string(&mut self) {
        // The scanner has already stripped the surrounding quotes and
        // stored the raw body in `Literal::String`. Allocate it on the
        // VM heap and emit a constant referencing the new handle.
        let Some(Literal::String(s)) = self.previous_token().literal.as_ref() else {
            unreachable!("scanner attaches Literal::String to every String token");
        };
        let handle = self.heap.alloc_string(s.clone());
        self.emit_constant(Value::Obj(handle));
    }

    fn grouping(&mut self) {
        // `(` already consumed; parse the inner expression then expect `)`.
        self.expression();
        self.consume(TokenType::RightParen, "Expect ')' after expression.");
    }

    fn unary(&mut self) {
        let op_kind = self.previous_token().ttype;
        // Only the operand is bound by Unary precedence; this is what
        // makes `-a.b.c` parse as `-(a.b.c)` rather than `(-a).b.c`.
        self.parse_precedence(Precedence::Unary);
        match op_kind {
            TokenType::Minus => self.emit_op(OpCode::Negate),
            TokenType::Bang => self.emit_op(OpCode::Not),
            other => unreachable!("Pratt rule for {other:?} dispatched to unary"),
        }
    }

    fn binary(&mut self) {
        // The left operand is already on the stack; we now compile the
        // right at one precedence rung *higher* so the same operator at
        // the same level is left-associative.
        let op_kind = self.previous_token().ttype;
        let rule_prec = rule_for(op_kind).precedence;
        self.parse_precedence(rule_prec.next_higher());
        match op_kind {
            TokenType::Plus => self.emit_op(OpCode::Add),
            TokenType::Minus => self.emit_op(OpCode::Subtract),
            TokenType::Star => self.emit_op(OpCode::Multiply),
            TokenType::Slash => self.emit_op(OpCode::Divide),
            // Chapter 18: `!=`, `<=`, `>=` synthesised from `==`/`<`/`>`
            // plus `OP_NOT`, exactly mirroring clox.
            TokenType::EqualEqual => self.emit_op(OpCode::Equal),
            TokenType::BangEqual => {
                self.emit_op(OpCode::Equal);
                self.emit_op(OpCode::Not);
            }
            TokenType::Greater => self.emit_op(OpCode::Greater),
            TokenType::GreaterEqual => {
                self.emit_op(OpCode::Less);
                self.emit_op(OpCode::Not);
            }
            TokenType::Less => self.emit_op(OpCode::Less),
            TokenType::LessEqual => {
                self.emit_op(OpCode::Greater);
                self.emit_op(OpCode::Not);
            }
            other => unreachable!("Pratt rule for {other:?} dispatched to binary"),
        }
    }

    // ---- diagnostics ----------------------------------------------------

    fn error_at_current(&mut self, message: &str) {
        if self.panic_mode {
            return;
        }
        self.panic_mode = true;
        let token = self.current_token().clone();
        self.errors.push(LoxError::parse(&token, message));
    }

    fn error_at_previous(&mut self, message: &str) {
        if self.panic_mode {
            return;
        }
        self.panic_mode = true;
        let token = self.previous_token().clone();
        self.errors.push(LoxError::parse(&token, message));
    }

    fn report(&mut self, err: LoxError) {
        if self.panic_mode {
            return;
        }
        self.panic_mode = true;
        self.errors.push(err);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::Vm;

    /// Convenience: compile + run, returning the value at the top of
    /// the stack at `OP_RETURN`.
    fn eval(src: &str) -> Value {
        let (chunk, mut heap) = compile(src).expect("compile clean");
        Vm::new().interpret(&chunk, &mut heap).expect("runs clean")
    }

    #[test]
    fn compiles_and_runs_a_single_number() {
        assert_eq!(eval("42"), Value::Number(42.0));
    }

    /// Chapter 17's signature reference fragment.
    #[test]
    fn precedence_term_lower_than_factor() {
        // `1 + 2 * 3` must parse as `1 + (2 * 3) = 7`, not `(1+2)*3 = 9`.
        assert_eq!(eval("1 + 2 * 3"), Value::Number(7.0));
    }

    #[test]
    fn grouping_overrides_precedence() {
        assert_eq!(eval("(1 + 2) * 3"), Value::Number(9.0));
    }

    #[test]
    fn unary_minus_binds_tighter_than_factor() {
        // `-2 * 3` ⇒ `(-2) * 3 = -6`.
        assert_eq!(eval("-2 * 3"), Value::Number(-6.0));
    }

    #[test]
    fn left_associative_subtraction() {
        // `1 - 2 - 3` ⇒ `(1 - 2) - 3 = -4` (not `1 - (2 - 3) = 2`).
        assert_eq!(eval("1 - 2 - 3"), Value::Number(-4.0));
    }

    #[test]
    fn left_associative_division() {
        // `8 / 4 / 2` ⇒ `(8/4)/2 = 1`.
        assert_eq!(eval("8 / 4 / 2"), Value::Number(1.0));
    }

    #[test]
    fn nested_grouping() {
        assert_eq!(eval("(5 - (3 - 1)) + -1"), Value::Number(2.0));
    }

    #[test]
    fn whitespace_and_comments_are_ignored() {
        let src = "// preamble\n  1 + 2  // tail\n";
        assert_eq!(eval(src), Value::Number(3.0));
    }

    #[test]
    fn missing_close_paren_reports_parse_error() {
        let errs = compile("(1 + 2").expect_err("expected error");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Expect ')' after expression."
            )),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn empty_source_reports_parse_error() {
        let errs = compile("").expect_err("expected error");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Expect expression."
            )),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn extra_tokens_after_expression_report_parse_error() {
        // `1 2` — once `1` is consumed we expect EOF, not another number.
        let errs = compile("1 2").expect_err("expected error");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Expect end of expression."
            )),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn scan_error_surfaces_through_compiler() {
        // `@` is not a valid Lox character; the scanner emits a Scan
        // error which the compiler propagates verbatim.
        let errs = compile("1 + @").expect_err("expected error");
        assert!(
            errs.iter().any(|e| matches!(e, LoxError::Scan { .. })),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn panic_mode_suppresses_cascade() {
        // After the first parse error, follow-on errors caused by the
        // confused parser state are suppressed until synchronisation.
        let errs = compile("(((").expect_err("expected error");
        assert!(
            errs.len() <= 2,
            "expected at most 2 errors with panic-mode suppression, got {}: {errs:?}",
            errs.len()
        );
    }
}

#[cfg(test)]
mod chapter_18_tests {
    //! Chapter 18 — Types of Values: literals, comparisons, type errors.

    use super::*;
    use crate::vm::{Vm, VmError};

    fn eval(src: &str) -> Value {
        let (chunk, mut heap) = compile(src).expect("compile clean");
        Vm::new().interpret(&chunk, &mut heap).expect("runs clean")
    }

    fn eval_err(src: &str) -> VmError {
        let (chunk, mut heap) = compile(src).expect("compile clean");
        Vm::new()
            .interpret(&chunk, &mut heap)
            .expect_err("expected runtime error")
    }

    #[test]
    fn nil_true_false_literals_evaluate() {
        assert_eq!(eval("nil"), Value::Nil);
        assert_eq!(eval("true"), Value::Bool(true));
        assert_eq!(eval("false"), Value::Bool(false));
    }

    #[test]
    fn logical_not_uses_lox_truthiness() {
        // Only nil and false are falsy.
        assert_eq!(eval("!nil"), Value::Bool(true));
        assert_eq!(eval("!false"), Value::Bool(true));
        assert_eq!(eval("!true"), Value::Bool(false));
        assert_eq!(eval("!0"), Value::Bool(false)); // 0 is truthy
        assert_eq!(eval("!!nil"), Value::Bool(false));
    }

    #[test]
    fn equality_is_polymorphic() {
        assert_eq!(eval("1 == 1"), Value::Bool(true));
        assert_eq!(eval("1 == 2"), Value::Bool(false));
        assert_eq!(eval("nil == nil"), Value::Bool(true));
        assert_eq!(eval("true == true"), Value::Bool(true));
        // Mixed types are unequal rather than raising — clox semantics.
        assert_eq!(eval("1 == true"), Value::Bool(false));
        assert_eq!(eval("nil == false"), Value::Bool(false));
    }

    #[test]
    fn bang_equal_synthesizes_via_equal_plus_not() {
        assert_eq!(eval("1 != 2"), Value::Bool(true));
        assert_eq!(eval("1 != 1"), Value::Bool(false));
        assert_eq!(eval("nil != true"), Value::Bool(true));
    }

    #[test]
    fn comparison_operators_on_numbers() {
        assert_eq!(eval("1 < 2"), Value::Bool(true));
        assert_eq!(eval("2 < 1"), Value::Bool(false));
        assert_eq!(eval("1 > 2"), Value::Bool(false));
        assert_eq!(eval("3 > 1"), Value::Bool(true));
        // <= and >= synthesise via the *opposite* op + OP_NOT.
        assert_eq!(eval("2 <= 2"), Value::Bool(true));
        assert_eq!(eval("2 <= 1"), Value::Bool(false));
        assert_eq!(eval("2 >= 2"), Value::Bool(true));
        assert_eq!(eval("1 >= 2"), Value::Bool(false));
    }

    #[test]
    fn precedence_equality_below_comparison_below_term() {
        // `1 + 2 < 4 == false` should parse as `((1 + 2) < 4) == false`,
        // i.e. `(3 < 4) == false` ⇒ `true == false` ⇒ false.
        assert_eq!(eval("1 + 2 < 4 == false"), Value::Bool(false));
    }

    #[test]
    fn negate_non_number_is_runtime_error_with_line() {
        let err = eval_err("-true");
        let VmError::Runtime { line, message } = err;
        assert_eq!(message, "Operand must be a number.");
        assert_eq!(line, 1);
    }

    #[test]
    fn arithmetic_on_non_numbers_is_runtime_error() {
        // `-` (and `*`, `/`) keep the chapter-18 numeric-only contract
        // even after chapter 19 overloads `+` for string concatenation.
        let VmError::Runtime { message, .. } = eval_err("1 - nil");
        assert_eq!(message, "Operands must be numbers.");
    }

    #[test]
    fn comparison_on_non_numbers_is_runtime_error() {
        let VmError::Runtime { message, .. } = eval_err("nil < 1");
        assert_eq!(message, "Operands must be numbers.");
    }

    #[test]
    fn equality_does_not_raise_for_mixed_types() {
        // `==` is polymorphic: mixed types compare unequal rather than
        // raising. Same for `!=`.
        assert_eq!(eval("nil == 1"), Value::Bool(false));
        assert_eq!(eval("true != 1"), Value::Bool(true));
    }

    #[test]
    fn runtime_error_line_attribution_is_correct() {
        // The `-` is on line 3, so the error's line should be 3.
        // (Using `-` rather than `+` keeps the assertion's expected
        // message stable across chapter 19, which overloads `+` to
        // also accept strings.)
        let (chunk, mut heap) = compile("1\n - \nnil").expect("compile clean");
        let VmError::Runtime { line, message } =
            Vm::new().interpret(&chunk, &mut heap).expect_err("rt err");
        assert_eq!(message, "Operands must be numbers.");
        assert_eq!(line, 3);
    }
}

#[cfg(test)]
mod chapter_19_tests {
    //! Chapter 19 — Strings: literals, concatenation, content equality,
    //! type errors when `+` mixes strings and numbers.

    use super::*;
    use crate::heap::Obj;
    use crate::vm::{Vm, VmError};

    /// Compile + run, returning both the result value and the heap so
    /// the test can dereference any returned `Value::Obj`.
    fn run(src: &str) -> (Value, crate::heap::Heap) {
        let (chunk, mut heap) = compile(src).expect("compile clean");
        let v = Vm::new().interpret(&chunk, &mut heap).expect("runs clean");
        (v, heap)
    }

    fn run_err(src: &str) -> VmError {
        let (chunk, mut heap) = compile(src).expect("compile clean");
        Vm::new()
            .interpret(&chunk, &mut heap)
            .expect_err("expected runtime error")
    }

    #[test]
    fn string_literal_evaluates_to_obj_string() {
        let (v, heap) = run("\"hello\"");
        let Value::Obj(h) = v else {
            panic!("expected Obj, got {v:?}");
        };
        assert_eq!(heap.as_str(h), Some("hello"));
    }

    #[test]
    fn empty_string_literal_works() {
        let (v, heap) = run("\"\"");
        let Value::Obj(h) = v else {
            panic!("expected Obj");
        };
        assert_eq!(heap.as_str(h), Some(""));
    }

    #[test]
    fn string_concatenation_via_plus() {
        let (v, heap) = run("\"foo\" + \"bar\"");
        let Value::Obj(h) = v else {
            panic!("expected Obj");
        };
        assert_eq!(heap.as_str(h), Some("foobar"));
    }

    #[test]
    fn three_way_concatenation_is_left_associative() {
        // `("a" + "b") + "c"` — same shape as numeric `1 + 2 + 3`.
        let (v, heap) = run("\"a\" + \"b\" + \"c\"");
        let Value::Obj(h) = v else {
            panic!("expected Obj");
        };
        assert_eq!(heap.as_str(h), Some("abc"));
    }

    #[test]
    fn equal_operator_compares_string_contents() {
        // Two distinct allocations of "abc" — pre-interning, they have
        // distinct handles but `==` must still return true.
        let (v, _heap) = run("\"abc\" == \"abc\"");
        assert_eq!(v, Value::Bool(true));

        let (v, _heap) = run("\"abc\" == \"xyz\"");
        assert_eq!(v, Value::Bool(false));
    }

    #[test]
    fn bang_equal_on_strings_negates_content_equality() {
        let (v, _heap) = run("\"abc\" != \"abc\"");
        assert_eq!(v, Value::Bool(false));
        let (v, _heap) = run("\"abc\" != \"xyz\"");
        assert_eq!(v, Value::Bool(true));
    }

    #[test]
    fn equal_across_different_types_is_false_not_an_error() {
        // String vs number / bool / nil — never raises, always false.
        let (v, _) = run("\"1\" == 1");
        assert_eq!(v, Value::Bool(false));
        let (v, _) = run("\"true\" == true");
        assert_eq!(v, Value::Bool(false));
        let (v, _) = run("\"\" == nil");
        assert_eq!(v, Value::Bool(false));
    }

    #[test]
    fn plus_on_string_and_number_is_runtime_error() {
        let VmError::Runtime { message, .. } = run_err("\"foo\" + 1");
        assert_eq!(message, "Operands must be two numbers or two strings.");
    }

    #[test]
    fn plus_on_two_nils_is_runtime_error() {
        let VmError::Runtime { message, .. } = run_err("nil + nil");
        assert_eq!(message, "Operands must be two numbers or two strings.");
    }

    #[test]
    fn ordering_op_on_strings_remains_a_runtime_error() {
        // `<` / `>` are numeric-only in Lox, even after chapter 19. clox
        // produces "Operands must be numbers." — our VM matches.
        let VmError::Runtime { message, .. } = run_err("\"a\" < \"b\"");
        assert_eq!(message, "Operands must be numbers.");
    }

    #[test]
    fn concatenation_allocates_a_new_object() {
        // The result handle must differ from either operand handle —
        // chapter 19 isn't interning yet, so concat always allocates.
        let (chunk, mut heap) = compile("\"a\" + \"b\"").expect("compile");
        let pre_objs = heap.len();
        let v = Vm::new().interpret(&chunk, &mut heap).expect("runs");
        // Compile time alloc'd 2 strings; concat alloc'd 1 more.
        assert_eq!(heap.len(), pre_objs + 1);
        let Value::Obj(h) = v else {
            panic!("expected Obj");
        };
        assert_eq!(heap.as_str(h), Some("ab"));
    }

    #[test]
    fn strings_are_truthy_via_bang() {
        let (v, _) = run("!\"\"");
        assert_eq!(v, Value::Bool(false)); // empty string is truthy → !"" is false
        let (v, _) = run("!\"x\"");
        assert_eq!(v, Value::Bool(false));
    }

    #[test]
    fn compile_emits_string_constant_with_obj_payload() {
        // Cross-check between the compiler and heap: a single string
        // literal pushes one Obj into the heap and one constant into
        // the chunk.
        let (chunk, heap) = compile("\"hi\"").expect("compile");
        assert_eq!(heap.len(), 1);
        assert_eq!(chunk.constants.len(), 1);
        let Value::Obj(h) = chunk.constants[0] else {
            panic!("expected Obj constant");
        };
        match heap.get(h) {
            Obj::Str(s) => assert_eq!(s, "hi"),
        }
    }
}
