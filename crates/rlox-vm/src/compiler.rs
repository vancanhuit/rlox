//! Chapter 21 — Global Variables.
//!
//! A single-pass Pratt parser that consumes tokens from the lazy
//! [`rlox_shared::Scanner`] (chapter 16) and emits a [`Chunk`] of
//! bytecode. There is no intermediate AST: each grammar rule directly
//! emits opcodes as it recognises operators, mirroring clox's
//! `compiler.c`.
//!
//! Chapters 17-19 compile expression bytecode; chapter 21 grows that
//! into a top-level program made of declarations and statements,
//! including `print`, expression statements, global `var` declarations,
//! global reads, and global assignment. Locals arrive in chapter 22.
//!
//! Pratt actions are encoded as a [`ParseFn`] enum (rather than as
//! function pointers) and resolved through [`Compiler::run_parse_fn`];
//! the borrow checker is happier when dispatch goes through a `match`
//! on `&mut self` rather than through closures or `fn` items.
//!
//! Errors accumulate into a `Vec<LoxError>`. When the parser hits one,
//! it enters "panic mode" and suppresses subsequent diagnostics until
//! a statement boundary, matching clox's synchronisation strategy.

use rlox_shared::error::LoxError;
use rlox_shared::scanner::Scanner;
use rlox_shared::token::{Literal, Token, TokenType};

use crate::chunk::{Chunk, OpCode};
use crate::heap::Heap;
use crate::value::Value;

/// Compile `source` into a [`Chunk`] terminated with [`OpCode::Return`].
/// Any object constants the chunk references are allocated into `heap`;
/// callers that execute multiple chunks with persistent VM state must
/// keep using the same heap.
///
/// # Errors
///
/// Returns every accumulated `LoxError` (scan errors from the lexer
/// plus parse errors from this compiler), mirroring jlox's
/// "report-everything-then-fail" strategy.
pub fn compile(source: &str, heap: &mut Heap) -> Result<Chunk, Vec<LoxError>> {
    let mut c = Compiler::new(source, heap);
    c.advance(); // prime `current`
    while !c.match_token(TokenType::Eof) {
        c.declaration();
    }
    c.emit_op(OpCode::Return);

    if c.errors.is_empty() {
        Ok(c.chunk)
    } else {
        Err(c.errors)
    }
}

/// Pratt precedence ladder, lowest to highest. The numeric ordering
/// matters: `parse_precedence(p)` continues while the *next* token's
/// infix precedence is `>= p`.
///
/// Slots reserved for later chapters (`Or`/`And`/`Call`/`Primary`) are
/// kept commented out so the ordering remains stable as we light them up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum Precedence {
    None = 0,
    Assignment = 1, // =
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
            Self::None => Self::Assignment,
            Self::Assignment => Self::Equality,
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
    /// Parses a string literal. The string body is allocated in the
    /// compiler's [`Heap`] and an `OP_CONSTANT <idx>` instruction is
    /// emitted with a [`Value::Obj`] payload.
    String,
    /// Parses a global variable read or assignment.
    Variable,
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
        TokenType::Identifier => ParseRule {
            prefix: ParseFn::Variable,
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

struct Compiler<'src, 'heap> {
    scanner: Scanner<'src>,
    /// Last consumed token. Many emit-opcode sites need access to it
    /// after we've moved past it (e.g. `binary` looks up the operator
    /// it just matched), so we cache the last two tokens.
    previous: Option<Token>,
    /// One-token lookahead.
    current: Option<Token>,
    chunk: Chunk,
    /// Heap that backs any [`Value::Obj`] entries emitted into
    /// `chunk.constants`.
    heap: &'heap mut Heap,
    errors: Vec<LoxError>,
    /// While in panic mode, errors are discarded until we synchronise.
    panic_mode: bool,
}

impl<'src, 'heap> Compiler<'src, 'heap> {
    fn new(source: &'src str, heap: &'heap mut Heap) -> Self {
        Self {
            scanner: Scanner::new(source),
            previous: None,
            current: None,
            chunk: Chunk::new(),
            heap,
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

    fn match_token(&mut self, expected: TokenType) -> bool {
        if !self.check(expected) {
            return false;
        }
        self.advance();
        true
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
        self.emit_constant_idx(idx);
    }

    fn emit_constant_idx(&mut self, idx: u8) {
        self.emit_op(OpCode::Constant);
        self.emit_byte(idx);
    }

    fn emit_op_with_byte(&mut self, op: OpCode, byte: u8) {
        self.emit_op(op);
        self.emit_byte(byte);
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
    /// `expression()` — kicks off at assignment, the lowest active
    /// precedence rung in chapter 21.
    fn expression(&mut self) {
        self.parse_precedence(Precedence::Assignment);
    }

    /// Recursive precedence-climbing parser.
    fn parse_precedence(&mut self, prec: Precedence) {
        self.advance();

        let prefix = rule_for(self.previous_token().ttype).prefix;
        if matches!(prefix, ParseFn::None) {
            self.error_at_previous("Expect expression.");
            return;
        }
        let can_assign = prec <= Precedence::Assignment;
        self.run_parse_fn(prefix, can_assign);

        while prec <= rule_for(self.current_token().ttype).precedence {
            self.advance();
            let infix = rule_for(self.previous_token().ttype).infix;
            self.run_parse_fn(infix, can_assign);
        }

        if can_assign && self.match_token(TokenType::Equal) {
            self.error_at_previous("Invalid assignment target.");
        }
    }

    fn run_parse_fn(&mut self, f: ParseFn, can_assign: bool) {
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
            ParseFn::Variable => self.variable(can_assign),
        }
    }

    // ---- grammar productions --------------------------------------------

    fn declaration(&mut self) {
        if self.match_token(TokenType::Var) {
            self.var_declaration();
        } else {
            self.statement();
        }

        if self.panic_mode {
            self.synchronize();
        }
    }

    fn statement(&mut self) {
        if self.match_token(TokenType::Print) {
            self.print_statement();
        } else {
            self.expression_statement();
        }
    }

    fn print_statement(&mut self) {
        self.expression();
        self.consume(TokenType::Semicolon, "Expect ';' after value.");
        self.emit_op(OpCode::Print);
    }

    fn expression_statement(&mut self) {
        self.expression();
        self.consume(TokenType::Semicolon, "Expect ';' after expression.");
        self.emit_op(OpCode::Pop);
    }

    fn var_declaration(&mut self) {
        let global = self.parse_variable("Expect variable name.");
        if self.match_token(TokenType::Equal) {
            self.expression();
        } else {
            self.emit_op(OpCode::Nil);
        }
        self.consume(
            TokenType::Semicolon,
            "Expect ';' after variable declaration.",
        );
        self.define_variable(global);
    }

    fn parse_variable(&mut self, message: &str) -> u8 {
        self.consume(TokenType::Identifier, message);
        if self.previous_token().ttype != TokenType::Identifier {
            return 0;
        }
        let name = self.previous_token().clone();
        self.identifier_constant(&name)
    }

    fn identifier_constant(&mut self, name: &Token) -> u8 {
        let handle = self.heap.alloc_string(name.lexeme.clone());
        self.chunk.add_constant(Value::Obj(handle))
    }

    fn define_variable(&mut self, global: u8) {
        self.emit_op_with_byte(OpCode::DefineGlobal, global);
    }

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

    fn variable(&mut self, can_assign: bool) {
        let name = self.previous_token().clone();
        self.named_variable(&name, can_assign);
    }

    fn named_variable(&mut self, name: &Token, can_assign: bool) {
        let arg = self.identifier_constant(name);
        if can_assign && self.match_token(TokenType::Equal) {
            self.expression();
            self.emit_op_with_byte(OpCode::SetGlobal, arg);
        } else {
            self.emit_op_with_byte(OpCode::GetGlobal, arg);
        }
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

    fn synchronize(&mut self) {
        self.panic_mode = false;
        while !self.check(TokenType::Eof) {
            if self
                .previous
                .as_ref()
                .is_some_and(|t| t.ttype == TokenType::Semicolon)
            {
                return;
            }

            match self.current_token().ttype {
                TokenType::Class
                | TokenType::Fun
                | TokenType::Var
                | TokenType::For
                | TokenType::If
                | TokenType::While
                | TokenType::Print
                | TokenType::Return => return,
                _ => self.advance(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::Obj;
    use crate::vm::{Vm, VmError};

    fn compile_ok(src: &str) -> (Chunk, Heap) {
        let mut heap = Heap::new();
        let chunk = compile(src, &mut heap).expect("compile clean");
        (chunk, heap)
    }

    fn compile_err(src: &str) -> Vec<LoxError> {
        let mut heap = Heap::new();
        compile(src, &mut heap).expect_err("expected compile error")
    }

    fn run(src: &str) -> (String, Heap) {
        let (chunk, mut heap) = compile_ok(src);
        let mut vm = Vm::new();
        let mut out = Vec::new();
        vm.interpret(&chunk, &mut heap, &mut out)
            .expect("runs clean");
        (String::from_utf8(out).expect("VM prints utf-8"), heap)
    }

    fn eval(expr: &str) -> String {
        run(&format!("print {expr};")).0
    }

    fn run_err(src: &str) -> VmError {
        let (chunk, mut heap) = compile_ok(src);
        let mut vm = Vm::new();
        let mut out = Vec::new();
        vm.interpret(&chunk, &mut heap, &mut out)
            .expect_err("expected runtime error")
    }

    fn eval_err(expr: &str) -> VmError {
        run_err(&format!("print {expr};"))
    }

    #[test]
    fn expression_precedence_and_associativity() {
        assert_eq!(eval("42"), "42\n");
        assert_eq!(eval("1 + 2 * 3"), "7\n");
        assert_eq!(eval("(1 + 2) * 3"), "9\n");
        assert_eq!(eval("-2 * 3"), "-6\n");
        assert_eq!(eval("1 - 2 - 3"), "-4\n");
        assert_eq!(eval("8 / 4 / 2"), "1\n");
        assert_eq!(eval("(5 - (3 - 1)) + -1"), "2\n");
    }

    #[test]
    fn whitespace_and_comments_are_ignored() {
        let src = "// preamble\n  print 1 + 2;  // tail\n";
        assert_eq!(run(src).0, "3\n");
    }

    #[test]
    fn missing_close_paren_reports_parse_error() {
        let errs = compile_err("print (1 + 2;");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Expect ')' after expression."
            )),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn empty_source_is_an_empty_program() {
        assert_eq!(run("").0, "");
    }

    #[test]
    fn expression_statement_requires_semicolon() {
        let errs = compile_err("1 + 2");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Expect ';' after expression."
            )),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn print_statement_requires_semicolon() {
        let errs = compile_err("print 1");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Expect ';' after value."
            )),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn scan_error_surfaces_through_compiler() {
        let errs = compile_err("print 1 + @;");
        assert!(
            errs.iter().any(|e| matches!(e, LoxError::Scan { .. })),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn invalid_assignment_target_is_compile_error() {
        let errs = compile_err("a * b = 3;");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Invalid assignment target."
            )),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn panic_mode_synchronizes_at_statement_boundaries() {
        let errs = compile_err("print (;\nprint 1;\nprint ;");
        assert!(
            errs.len() <= 2,
            "expected synchronization to suppress cascades, got {}: {errs:?}",
            errs.len()
        );
    }

    #[test]
    fn nil_true_false_literals_evaluate() {
        assert_eq!(eval("nil"), "nil\n");
        assert_eq!(eval("true"), "true\n");
        assert_eq!(eval("false"), "false\n");
    }

    #[test]
    fn logical_not_uses_lox_truthiness() {
        assert_eq!(eval("!nil"), "true\n");
        assert_eq!(eval("!false"), "true\n");
        assert_eq!(eval("!true"), "false\n");
        assert_eq!(eval("!0"), "false\n");
        assert_eq!(eval("!!nil"), "false\n");
    }

    #[test]
    fn equality_is_polymorphic() {
        assert_eq!(eval("1 == 1"), "true\n");
        assert_eq!(eval("1 == 2"), "false\n");
        assert_eq!(eval("nil == nil"), "true\n");
        assert_eq!(eval("true == true"), "true\n");
        assert_eq!(eval("1 == true"), "false\n");
        assert_eq!(eval("nil == false"), "false\n");
        assert_eq!(eval("1 != 2"), "true\n");
        assert_eq!(eval("1 != 1"), "false\n");
    }

    #[test]
    fn comparison_operators_on_numbers() {
        assert_eq!(eval("1 < 2"), "true\n");
        assert_eq!(eval("2 < 1"), "false\n");
        assert_eq!(eval("1 > 2"), "false\n");
        assert_eq!(eval("3 > 1"), "true\n");
        assert_eq!(eval("2 <= 2"), "true\n");
        assert_eq!(eval("2 <= 1"), "false\n");
        assert_eq!(eval("2 >= 2"), "true\n");
        assert_eq!(eval("1 >= 2"), "false\n");
    }

    #[test]
    fn precedence_equality_below_comparison_below_term() {
        assert_eq!(eval("1 + 2 < 4 == false"), "false\n");
    }

    #[test]
    fn numeric_type_errors_report_runtime_errors() {
        let VmError::Runtime { line, message } = eval_err("-true");
        assert_eq!(message, "Operand must be a number.");
        assert_eq!(line, 1);

        let VmError::Runtime { message, .. } = eval_err("1 - nil");
        assert_eq!(message, "Operands must be numbers.");

        let VmError::Runtime { message, .. } = eval_err("nil < 1");
        assert_eq!(message, "Operands must be numbers.");
    }

    #[test]
    fn runtime_error_line_attribution_is_correct() {
        let VmError::Runtime { line, message } = run_err("print 1\n - \nnil;");
        assert_eq!(message, "Operands must be numbers.");
        assert_eq!(line, 3);
    }

    #[test]
    fn strings_evaluate_and_concatenate() {
        assert_eq!(eval("\"hello\""), "hello\n");
        assert_eq!(eval("\"\""), "\n");
        assert_eq!(eval("\"foo\" + \"bar\""), "foobar\n");
        assert_eq!(eval("\"a\" + \"b\" + \"c\""), "abc\n");
    }

    #[test]
    fn string_equality_compares_contents() {
        assert_eq!(eval("\"abc\" == \"abc\""), "true\n");
        assert_eq!(eval("\"abc\" == \"xyz\""), "false\n");
        assert_eq!(eval("\"abc\" != \"abc\""), "false\n");
        assert_eq!(eval("\"abc\" != \"xyz\""), "true\n");
        assert_eq!(eval("\"1\" == 1"), "false\n");
        assert_eq!(eval("\"true\" == true"), "false\n");
        assert_eq!(eval("\"\" == nil"), "false\n");
    }

    #[test]
    fn string_type_errors_report_runtime_errors() {
        let VmError::Runtime { message, .. } = eval_err("\"foo\" + 1");
        assert_eq!(message, "Operands must be two numbers or two strings.");

        let VmError::Runtime { message, .. } = eval_err("nil + nil");
        assert_eq!(message, "Operands must be two numbers or two strings.");

        let VmError::Runtime { message, .. } = eval_err("\"a\" < \"b\"");
        assert_eq!(message, "Operands must be numbers.");
    }

    #[test]
    fn concatenation_allocates_a_new_object() {
        let mut heap = Heap::new();
        let chunk = compile("print \"a\" + \"b\";", &mut heap).expect("compile");
        let pre_objs = heap.len();
        let mut vm = Vm::new();
        let mut out = Vec::new();
        vm.interpret(&chunk, &mut heap, &mut out).expect("runs");
        assert_eq!(String::from_utf8(out).unwrap(), "ab\n");
        assert_eq!(heap.len(), pre_objs + 1);
    }

    #[test]
    fn strings_are_truthy_via_bang() {
        assert_eq!(eval("!\"\""), "false\n");
        assert_eq!(eval("!\"x\""), "false\n");
    }

    #[test]
    fn compile_emits_string_constant_with_obj_payload() {
        let (chunk, heap) = compile_ok("print \"hi\";");
        assert_eq!(heap.len(), 1);
        assert!(chunk.constants.iter().any(|v| {
            let Value::Obj(h) = v else {
                return false;
            };
            matches!(heap.get(*h), Obj::Str(s) if s == "hi")
        }));
    }

    #[test]
    fn print_and_expression_statements_have_expected_output() {
        assert_eq!(run("print 1 + 2;\nprint 3 * 4;").0, "3\n12\n");
        assert_eq!(run("1 + 2;").0, "");
    }

    #[test]
    fn global_var_declarations_read_and_default_nil() {
        let src = r#"
            var beverage = "cafe au lait";
            var breakfast = "beignets with " + beverage;
            var unset;
            print breakfast;
            print unset;
        "#;
        assert_eq!(run(src).0, "beignets with cafe au lait\nnil\n");
    }

    #[test]
    fn global_assignment_updates_existing_binding() {
        let src = r#"
            var breakfast = "beignets";
            var beverage = "cafe au lait";
            breakfast = "beignets with " + beverage;
            print breakfast;
            print breakfast = "toast";
            print breakfast;
        "#;
        assert_eq!(run(src).0, "beignets with cafe au lait\ntoast\ntoast\n");
    }

    #[test]
    fn global_redefinition_is_allowed() {
        assert_eq!(run("var a = 1; var a = 2; print a;").0, "2\n");
    }

    #[test]
    fn undefined_global_read_is_runtime_error() {
        let VmError::Runtime { message, line } = run_err("print missing;");
        assert_eq!(message, "Undefined variable 'missing'.");
        assert_eq!(line, 1);
    }

    #[test]
    fn undefined_global_assignment_is_runtime_error() {
        let VmError::Runtime { message, line } = run_err("missing = 1;");
        assert_eq!(message, "Undefined variable 'missing'.");
        assert_eq!(line, 1);
    }

    #[test]
    fn globals_persist_when_vm_and_heap_are_reused() {
        let mut vm = Vm::new();
        let mut heap = Heap::new();
        let mut out = Vec::new();

        let first = compile("var answer = 41;", &mut heap).expect("compile first");
        vm.interpret(&first, &mut heap, &mut out)
            .expect("first runs");

        let second =
            compile("answer = answer + 1; print answer;", &mut heap).expect("compile second");
        vm.interpret(&second, &mut heap, &mut out)
            .expect("second runs");

        assert_eq!(String::from_utf8(out).unwrap(), "42\n");
    }
}
