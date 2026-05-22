//! Chapter 25 — Closures.
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
//! global reads, and global assignment. Chapter 22 adds lexical block
//! scopes and stack-slot locals. Chapter 23 compiles structured
//! control flow to raw forward and backward jumps. Chapter 24 adds
//! first-class functions, calls, `return`, and native functions.
//! Chapter 25 adds closures and upvalues for captured locals.
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
use crate::heap::{Function, Heap};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum Precedence {
    None = 0,
    Assignment = 1, // =
    Or = 2,         // or
    And = 3,        // and
    Equality = 4,   // == !=
    Comparison = 5, // < > <= >=
    Term = 6,       // + -
    Factor = 7,     // * /
    Unary = 8,      // ! -
    Call = 9,       // . ()
}

impl Precedence {
    /// One precedence level higher than `self`. Binary operators recurse
    /// at `next_higher()` so the same operator at the same level is
    /// left-associative.
    const fn next_higher(self) -> Self {
        match self {
            Self::None => Self::Assignment,
            Self::Assignment => Self::Or,
            Self::Or => Self::And,
            Self::And => Self::Equality,
            Self::Equality => Self::Comparison,
            Self::Comparison => Self::Term,
            Self::Term => Self::Factor,
            Self::Factor => Self::Unary,
            Self::Unary | Self::Call => Self::Call,
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
    /// Parses a local/global variable read or assignment.
    Variable,
    /// Parses a function call argument list.
    Call,
    /// Parses short-circuiting logical `and`.
    And,
    /// Parses short-circuiting logical `or`.
    Or,
}

#[derive(Debug, Clone, Copy)]
struct ParseRule {
    prefix: ParseFn,
    infix: ParseFn,
    precedence: Precedence,
}

#[derive(Debug, Clone)]
struct Local {
    name: Token,
    /// `None` while its initializer is compiling. A read during that
    /// window is the chapter-22 self-initializer compile error.
    depth: Option<usize>,
    /// True when a nested function captures this local.
    is_captured: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UpvalueRef {
    index: u8,
    is_local: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FunctionType {
    Script,
    Function,
}

#[derive(Debug)]
struct CompileState {
    chunk: Chunk,
    locals: Vec<Local>,
    scope_depth: usize,
    function_type: FunctionType,
    function_arity: u8,
    upvalues: Vec<UpvalueRef>,
}

impl CompileState {
    fn script() -> Self {
        Self {
            chunk: Chunk::new(),
            locals: Vec::new(),
            scope_depth: 0,
            function_type: FunctionType::Script,
            function_arity: 0,
            upvalues: Vec::new(),
        }
    }

    fn function(name: &Token) -> Self {
        Self {
            chunk: Chunk::new(),
            locals: vec![Local {
                name: Token::new(TokenType::Identifier, "", None, name.line),
                depth: Some(0),
                is_captured: false,
            }],
            scope_depth: 0,
            function_type: FunctionType::Function,
            function_arity: 0,
            upvalues: Vec::new(),
        }
    }
}

/// Pratt rule lookup. Mirrors clox's static `rules[]` array.
const fn rule_for(t: TokenType) -> ParseRule {
    match t {
        TokenType::LeftParen => ParseRule {
            prefix: ParseFn::Grouping,
            infix: ParseFn::Call,
            precedence: Precedence::Call,
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
        // Chapter 23 — short-circuiting logical operators.
        TokenType::And => ParseRule {
            prefix: ParseFn::None,
            infix: ParseFn::And,
            precedence: Precedence::And,
        },
        TokenType::Or => ParseRule {
            prefix: ParseFn::None,
            infix: ParseFn::Or,
            precedence: Precedence::Or,
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
    /// Locals currently alive on the VM stack, in stack-slot order.
    locals: Vec<Local>,
    /// Current lexical block depth. Top-level globals live at depth 0
    /// and are not tracked in `locals`.
    scope_depth: usize,
    /// Whether the current compilation target is top-level script code
    /// or a user-declared function body.
    function_type: FunctionType,
    /// Number of parameters in the current function target.
    function_arity: u8,
    /// Upvalues captured by the current function target.
    upvalues: Vec<UpvalueRef>,
    /// Compiler states for lexically enclosing function bodies. The
    /// active state stays in the direct fields above for minimal churn
    /// with the chapter-24 compiler; this stack lets chapter 25 resolve
    /// captured locals through nested function boundaries.
    enclosing: Vec<CompileState>,
    errors: Vec<LoxError>,
    /// While in panic mode, errors are discarded until we synchronise.
    panic_mode: bool,
}

impl<'src, 'heap> Compiler<'src, 'heap> {
    fn new(source: &'src str, heap: &'heap mut Heap) -> Self {
        let state = CompileState::script();
        Self {
            scanner: Scanner::new(source),
            previous: None,
            current: None,
            chunk: state.chunk,
            heap,
            locals: state.locals,
            scope_depth: state.scope_depth,
            function_type: state.function_type,
            function_arity: state.function_arity,
            upvalues: state.upvalues,
            enclosing: Vec::new(),
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

    fn take_state(&mut self) -> CompileState {
        CompileState {
            chunk: std::mem::take(&mut self.chunk),
            locals: std::mem::take(&mut self.locals),
            scope_depth: self.scope_depth,
            function_type: self.function_type,
            function_arity: self.function_arity,
            upvalues: std::mem::take(&mut self.upvalues),
        }
    }

    fn replace_state(&mut self, state: CompileState) -> CompileState {
        let old = self.take_state();
        self.set_state(state);
        old
    }

    fn set_state(&mut self, state: CompileState) {
        self.chunk = state.chunk;
        self.locals = state.locals;
        self.scope_depth = state.scope_depth;
        self.function_type = state.function_type;
        self.function_arity = state.function_arity;
        self.upvalues = state.upvalues;
    }

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

    fn emit_return(&mut self) {
        self.emit_op(OpCode::Nil);
        self.emit_op(OpCode::Return);
    }

    fn emit_jump(&mut self, op: OpCode) -> usize {
        self.emit_op(op);
        self.emit_byte(0xff);
        self.emit_byte(0xff);
        self.chunk.len() - 2
    }

    fn patch_jump(&mut self, offset: usize) {
        let jump = self.chunk.len() - offset - 2;
        if jump > u16::MAX as usize {
            self.error_at_previous("Too much code to jump over.");
            return;
        }

        let bytes = u16::try_from(jump)
            .expect("jump was checked against u16::MAX")
            .to_be_bytes();
        self.chunk.code[offset] = bytes[0];
        self.chunk.code[offset + 1] = bytes[1];
    }

    fn emit_loop(&mut self, loop_start: usize) {
        self.emit_op(OpCode::Loop);
        let offset = self.chunk.len() - loop_start + 2;
        if offset > u16::MAX as usize {
            self.error_at_previous("Loop body too large.");
            return;
        }

        let bytes = u16::try_from(offset)
            .expect("loop offset was checked against u16::MAX")
            .to_be_bytes();
        self.emit_byte(bytes[0]);
        self.emit_byte(bytes[1]);
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
    /// precedence rung.
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
            ParseFn::Call => self.call(),
            ParseFn::And => self.and(),
            ParseFn::Or => self.or(),
        }
    }

    // ---- grammar productions --------------------------------------------

    fn declaration(&mut self) {
        if self.match_token(TokenType::Fun) {
            self.fun_declaration();
        } else if self.match_token(TokenType::Var) {
            self.var_declaration();
        } else {
            self.statement();
        }

        if self.panic_mode {
            self.synchronize();
        }
    }

    fn statement(&mut self) {
        if self.match_token(TokenType::For) {
            self.for_statement();
        } else if self.match_token(TokenType::If) {
            self.if_statement();
        } else if self.match_token(TokenType::Print) {
            self.print_statement();
        } else if self.match_token(TokenType::Return) {
            self.return_statement();
        } else if self.match_token(TokenType::While) {
            self.while_statement();
        } else if self.match_token(TokenType::LeftBrace) {
            self.begin_scope();
            self.block();
            self.end_scope();
        } else {
            self.expression_statement();
        }
    }

    fn fun_declaration(&mut self) {
        let global = self.parse_variable("Expect function name.");
        let name = self.previous_token().clone();
        self.mark_initialized();
        self.function(&name);
        self.define_variable(global);
    }

    fn function(&mut self, name: &Token) {
        let outer = self.replace_state(CompileState::function(name));
        self.enclosing.push(outer);
        self.begin_scope();

        self.consume(TokenType::LeftParen, "Expect '(' after function name.");
        if !self.check(TokenType::RightParen) {
            loop {
                if self.function_arity == u8::MAX {
                    self.error_at_current("Can't have more than 255 parameters.");
                } else {
                    self.function_arity += 1;
                }
                let param = self.parse_variable("Expect parameter name.");
                self.define_variable(param);

                if !self.match_token(TokenType::Comma) {
                    break;
                }
            }
        }
        self.consume(TokenType::RightParen, "Expect ')' after parameters.");
        self.consume(TokenType::LeftBrace, "Expect '{' before function body.");
        self.block();
        self.emit_return();

        let inner = self.take_state();
        let arity = inner.function_arity;
        let upvalues = inner.upvalues;
        let chunk = inner.chunk;
        let upvalue_count =
            u8::try_from(upvalues.len()).expect("upvalues are capped at 256 entries");
        let outer = self
            .enclosing
            .pop()
            .expect("function compiler must have an enclosing state");
        self.set_state(outer);

        let handle = self.heap.alloc_function(Function::with_upvalues(
            Some(name.lexeme.clone()),
            arity,
            upvalue_count,
            chunk,
        ));
        let idx = self.chunk.add_constant(Value::Obj(handle));
        self.emit_op_with_byte(OpCode::Closure, idx);
        for upvalue in upvalues {
            self.emit_byte(u8::from(upvalue.is_local));
            self.emit_byte(upvalue.index);
        }
    }

    fn for_statement(&mut self) {
        self.begin_scope();

        self.consume(TokenType::LeftParen, "Expect '(' after 'for'.");
        if self.match_token(TokenType::Semicolon) {
            // No initializer.
        } else if self.match_token(TokenType::Var) {
            self.var_declaration();
        } else {
            self.expression_statement();
        }

        let mut loop_start = self.chunk.len();
        let mut exit_jump = None;
        if !self.match_token(TokenType::Semicolon) {
            self.expression();
            self.consume(TokenType::Semicolon, "Expect ';' after loop condition.");

            exit_jump = Some(self.emit_jump(OpCode::JumpIfFalse));
            self.emit_op(OpCode::Pop);
        }

        if !self.match_token(TokenType::RightParen) {
            let body_jump = self.emit_jump(OpCode::Jump);
            let increment_start = self.chunk.len();
            self.expression();
            self.emit_op(OpCode::Pop);
            self.consume(TokenType::RightParen, "Expect ')' after for clauses.");

            self.emit_loop(loop_start);
            loop_start = increment_start;
            self.patch_jump(body_jump);
        }

        self.statement();
        self.emit_loop(loop_start);

        if let Some(jump) = exit_jump {
            self.patch_jump(jump);
            self.emit_op(OpCode::Pop);
        }

        self.end_scope();
    }

    fn if_statement(&mut self) {
        self.consume(TokenType::LeftParen, "Expect '(' after 'if'.");
        self.expression();
        self.consume(TokenType::RightParen, "Expect ')' after condition.");

        let then_jump = self.emit_jump(OpCode::JumpIfFalse);
        self.emit_op(OpCode::Pop);
        self.statement();

        let else_jump = self.emit_jump(OpCode::Jump);
        self.patch_jump(then_jump);
        self.emit_op(OpCode::Pop);

        if self.match_token(TokenType::Else) {
            self.statement();
        }
        self.patch_jump(else_jump);
    }

    fn while_statement(&mut self) {
        let loop_start = self.chunk.len();
        self.consume(TokenType::LeftParen, "Expect '(' after 'while'.");
        self.expression();
        self.consume(TokenType::RightParen, "Expect ')' after condition.");

        let exit_jump = self.emit_jump(OpCode::JumpIfFalse);
        self.emit_op(OpCode::Pop);
        self.statement();
        self.emit_loop(loop_start);

        self.patch_jump(exit_jump);
        self.emit_op(OpCode::Pop);
    }

    fn block(&mut self) {
        while !self.check(TokenType::RightBrace) && !self.check(TokenType::Eof) {
            self.declaration();
        }
        self.consume(TokenType::RightBrace, "Expect '}' after block.");
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

    fn return_statement(&mut self) {
        if self.function_type == FunctionType::Script {
            self.error_at_previous("Can't return from top-level code.");
        }

        if self.match_token(TokenType::Semicolon) {
            self.emit_op(OpCode::Nil);
        } else {
            self.expression();
            self.consume(TokenType::Semicolon, "Expect ';' after return value.");
        }
        self.emit_op(OpCode::Return);
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
        self.declare_variable(&name);
        if self.scope_depth > 0 {
            return 0;
        }
        self.identifier_constant(&name)
    }

    fn identifier_constant(&mut self, name: &Token) -> u8 {
        let handle = self.heap.alloc_string(name.lexeme.clone());
        self.chunk.add_constant(Value::Obj(handle))
    }

    fn define_variable(&mut self, global: u8) {
        if self.scope_depth > 0 {
            self.mark_initialized();
            return;
        }
        self.emit_op_with_byte(OpCode::DefineGlobal, global);
    }

    fn and(&mut self) {
        let end_jump = self.emit_jump(OpCode::JumpIfFalse);
        self.emit_op(OpCode::Pop);
        self.parse_precedence(Precedence::And);
        self.patch_jump(end_jump);
    }

    fn or(&mut self) {
        let else_jump = self.emit_jump(OpCode::JumpIfFalse);
        let end_jump = self.emit_jump(OpCode::Jump);

        self.patch_jump(else_jump);
        self.emit_op(OpCode::Pop);

        self.parse_precedence(Precedence::Or);
        self.patch_jump(end_jump);
    }

    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    fn end_scope(&mut self) {
        self.scope_depth -= 1;
        while self
            .locals
            .last()
            .is_some_and(|local| local.depth.unwrap_or(usize::MAX) > self.scope_depth)
        {
            let op = if self
                .locals
                .last()
                .expect("loop condition ensures a local exists")
                .is_captured
            {
                OpCode::CloseUpvalue
            } else {
                OpCode::Pop
            };
            self.emit_op(op);
            self.locals.pop();
        }
    }

    fn declare_variable(&mut self, name: &Token) {
        if self.scope_depth == 0 {
            return;
        }

        let duplicate = self
            .locals
            .iter()
            .rev()
            .take_while(|local| local.depth.is_none_or(|depth| depth >= self.scope_depth))
            .any(|local| local.name.lexeme == name.lexeme);
        if duplicate {
            self.error_at_previous("Already a variable with this name in this scope.");
        }

        self.add_local(name.clone());
    }

    fn add_local(&mut self, name: Token) {
        if self.locals.len() > u8::MAX as usize {
            self.error_at_previous("Too many local variables in function.");
            return;
        }
        self.locals.push(Local {
            name,
            depth: None,
            is_captured: false,
        });
    }

    fn mark_initialized(&mut self) {
        if self.scope_depth == 0 {
            return;
        }
        if let Some(local) = self.locals.last_mut()
            && local.depth.is_none()
        {
            local.depth = Some(self.scope_depth);
        }
    }

    fn resolve_local(&mut self, name: &Token) -> Option<u8> {
        let found = self
            .locals
            .iter()
            .enumerate()
            .rev()
            .find(|(_, local)| local.name.lexeme == name.lexeme)
            .map(|(slot, local)| (slot, local.depth.is_none()));

        let (slot, is_uninitialized) = found?;
        if is_uninitialized {
            self.error_at_previous("Can't read local variable in its own initializer.");
        }
        Some(u8::try_from(slot).expect("locals are capped at 256 slots"))
    }

    fn resolve_upvalue(&mut self, name: &Token) -> Option<u8> {
        if self.enclosing.is_empty() {
            return None;
        }
        self.resolve_upvalue_for_state(self.enclosing.len(), name)
    }

    fn resolve_upvalue_for_state(&mut self, state_index: usize, name: &Token) -> Option<u8> {
        if state_index == 0 {
            return None;
        }

        let enclosing_index = state_index - 1;
        if let Some(local) = self.resolve_local_in_enclosing(enclosing_index, name) {
            self.enclosing[enclosing_index].locals[usize::from(local)].is_captured = true;
            return Some(self.add_upvalue_to_state(state_index, local, true));
        }

        let upvalue = self.resolve_upvalue_for_state(enclosing_index, name)?;
        Some(self.add_upvalue_to_state(state_index, upvalue, false))
    }

    fn resolve_local_in_enclosing(&mut self, state_index: usize, name: &Token) -> Option<u8> {
        let found = self.enclosing[state_index]
            .locals
            .iter()
            .enumerate()
            .rev()
            .find(|(_, local)| local.name.lexeme == name.lexeme)
            .map(|(slot, local)| (slot, local.depth.is_none()));

        let (slot, is_uninitialized) = found?;
        if is_uninitialized {
            self.error_at_previous("Can't read local variable in its own initializer.");
        }
        Some(u8::try_from(slot).expect("locals are capped at 256 slots"))
    }

    fn add_upvalue_to_state(&mut self, state_index: usize, index: u8, is_local: bool) -> u8 {
        let existing = self
            .upvalues_for_state(state_index)
            .iter()
            .position(|upvalue| upvalue.index == index && upvalue.is_local == is_local);
        if let Some(existing) = existing {
            return u8::try_from(existing).expect("upvalues are capped at 256 entries");
        }

        if self.upvalues_for_state(state_index).len() > u8::MAX as usize {
            self.error_at_previous("Too many closure variables in function.");
            return 0;
        }

        self.upvalues_for_state_mut(state_index)
            .push(UpvalueRef { index, is_local });
        u8::try_from(self.upvalues_for_state(state_index).len() - 1)
            .expect("upvalues are capped at 256 entries")
    }

    fn upvalues_for_state(&self, state_index: usize) -> &[UpvalueRef] {
        if state_index == self.enclosing.len() {
            &self.upvalues
        } else {
            &self.enclosing[state_index].upvalues
        }
    }

    fn upvalues_for_state_mut(&mut self, state_index: usize) -> &mut Vec<UpvalueRef> {
        if state_index == self.enclosing.len() {
            &mut self.upvalues
        } else {
            &mut self.enclosing[state_index].upvalues
        }
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
        let (get_op, set_op, arg) = if let Some(local) = self.resolve_local(name) {
            (OpCode::GetLocal, OpCode::SetLocal, local)
        } else if let Some(upvalue) = self.resolve_upvalue(name) {
            (OpCode::GetUpvalue, OpCode::SetUpvalue, upvalue)
        } else {
            (
                OpCode::GetGlobal,
                OpCode::SetGlobal,
                self.identifier_constant(name),
            )
        };

        if can_assign && self.match_token(TokenType::Equal) {
            self.expression();
            self.emit_op_with_byte(set_op, arg);
        } else {
            self.emit_op_with_byte(get_op, arg);
        }
    }

    fn call(&mut self) {
        let arg_count = self.argument_list();
        self.emit_op_with_byte(OpCode::Call, arg_count);
    }

    fn argument_list(&mut self) -> u8 {
        let mut count = 0_u8;
        if !self.check(TokenType::RightParen) {
            loop {
                if count == u8::MAX {
                    self.error_at_current("Can't have more than 255 arguments.");
                }
                self.expression();
                count = count.saturating_add(1);

                if !self.match_token(TokenType::Comma) {
                    break;
                }
            }
        }
        self.consume(TokenType::RightParen, "Expect ')' after arguments.");
        count
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
                | TokenType::Return
                | TokenType::RightBrace => return,
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
        // Runtime installs the chapter-24 `clock` native, then
        // concatenation allocates its result string.
        assert_eq!(heap.len(), pre_objs + 2);
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

    #[test]
    fn block_scopes_shadow_and_restore_globals() {
        let src = r#"
            var a = "global";
            {
                var a = "block";
                print a;
            }
            print a;
        "#;
        assert_eq!(run(src).0, "block\nglobal\n");
    }

    #[test]
    fn nested_local_scopes_shadow_and_restore_outer_locals() {
        let src = r#"
            {
                var a = "outer";
                {
                    var a = "inner";
                    print a;
                }
                print a;
            }
        "#;
        assert_eq!(run(src).0, "inner\nouter\n");
    }

    #[test]
    fn local_assignment_updates_stack_slot() {
        let src = r"
            {
                var a = 1;
                a = a + 1;
                print a;
                print a = 3;
                print a;
            }
        ";
        assert_eq!(run(src).0, "2\n3\n3\n");
    }

    #[test]
    fn locals_are_popped_at_scope_end() {
        let src = r#"
            var a = "outer";
            {
                var b = "inner";
                print b;
            }
            print a;
        "#;
        assert_eq!(run(src).0, "inner\nouter\n");
    }

    #[test]
    fn local_redeclaration_in_same_scope_is_compile_error() {
        let errs = compile_err("{ var a = 1; var a = 2; }");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. }
                    if message == "Already a variable with this name in this scope."
            )),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn local_can_shadow_same_name_in_nested_scope() {
        assert_eq!(
            run("{ var a = 1; { var a = 2; print a; } print a; }").0,
            "2\n1\n"
        );
    }

    #[test]
    fn local_initializer_cannot_read_own_name() {
        let errs = compile_err("{ var a = a; }");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. }
                    if message == "Can't read local variable in its own initializer."
            )),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn missing_block_close_reports_parse_error() {
        let errs = compile_err("{ print 1;");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Expect '}' after block."
            )),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn compiler_emits_local_opcodes_for_block_variables() {
        let (chunk, _) = compile_ok("{ var a = 1; print a; a = 2; print a; }");
        assert!(chunk.code.contains(&(OpCode::GetLocal as u8)));
        assert!(chunk.code.contains(&(OpCode::SetLocal as u8)));
    }

    #[test]
    fn if_else_executes_only_selected_branch() {
        let src = r#"
            if (true) print "then";
            if (false) print "bad";
            if (false) print "bad"; else print "else";
            if (nil) print "bad"; else print "nil falsey";
            if ("truthy") print "string truthy";
        "#;
        assert_eq!(run(src).0, "then\nelse\nnil falsey\nstring truthy\n");
    }

    #[test]
    fn logical_and_or_short_circuit_and_return_operand_values() {
        let src = r#"
            var a = "start";
            print false and (a = "bad");
            print a;
            print true or (a = "bad");
            print a;
            print nil or "fallback";
            print "left" and "right";
        "#;
        assert_eq!(run(src).0, "false\nstart\ntrue\nstart\nfallback\nright\n");
    }

    #[test]
    fn logical_precedence_matches_lox() {
        assert_eq!(eval("false or true and false"), "false\n");
        assert_eq!(eval("(false or true) and false"), "false\n");
        assert_eq!(eval("true or false and false"), "true\n");
    }

    #[test]
    fn while_loop_iterates_until_condition_is_false() {
        let src = r"
            var i = 0;
            while (i < 3) {
                print i;
                i = i + 1;
            }
        ";
        assert_eq!(run(src).0, "0\n1\n2\n");
    }

    #[test]
    fn while_condition_may_start_false() {
        assert_eq!(run("while (false) print 1; print 2;").0, "2\n");
    }

    #[test]
    fn for_loop_runs_initializer_condition_increment_and_body() {
        let src = r"
            for (var i = 0; i < 3; i = i + 1) {
                print i;
            }
        ";
        assert_eq!(run(src).0, "0\n1\n2\n");
    }

    #[test]
    fn for_loop_accepts_omitted_clauses() {
        let src = r"
            var i = 0;
            for (; i < 3;) {
                print i;
                i = i + 1;
            }
        ";
        assert_eq!(run(src).0, "0\n1\n2\n");
    }

    #[test]
    fn for_initializer_variable_is_scoped_to_loop() {
        let err = run_err("for (var i = 0; i < 1; i = i + 1) {} print i;");
        let VmError::Runtime { message, line } = err;
        assert_eq!(message, "Undefined variable 'i'.");
        assert_eq!(line, 1);
    }

    #[test]
    fn control_flow_requires_expected_punctuation() {
        let errs = compile_err("if true) print 1;");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Expect '(' after 'if'."
            )),
            "errors were: {errs:?}"
        );

        let errs = compile_err("while (true print 1;");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Expect ')' after condition."
            )),
            "errors were: {errs:?}"
        );

        let errs = compile_err("for (var i = 0; i < 3 i = i + 1) print i;");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Expect ';' after loop condition."
            )),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn compiler_emits_jump_and_loop_opcodes_for_control_flow() {
        let (chunk, _) = compile_ok(
            r"
            if (true) print 1; else print 2;
            while (false) print 3;
            ",
        );
        assert!(chunk.code.contains(&(OpCode::Jump as u8)));
        assert!(chunk.code.contains(&(OpCode::JumpIfFalse as u8)));
        assert!(chunk.code.contains(&(OpCode::Loop as u8)));
    }

    #[test]
    fn function_declaration_and_call_execute_body() {
        let src = r#"
            fun sayHi() {
                print "hi";
            }
            sayHi();
        "#;
        assert_eq!(run(src).0, "hi\n");
    }

    #[test]
    fn functions_accept_parameters_and_return_values() {
        let src = r"
            fun add(a, b, c) {
                return a + b + c;
            }
            print add(1, 2, 3);
        ";
        assert_eq!(run(src).0, "6\n");
    }

    #[test]
    fn functions_without_explicit_return_return_nil() {
        let src = r"
            fun noop() {}
            print noop();
        ";
        assert_eq!(run(src).0, "nil\n");
    }

    #[test]
    fn recursive_functions_use_call_frames() {
        let src = r"
            fun fact(n) {
                if (n <= 1) return 1;
                return n * fact(n - 1);
            }
            print fact(5);
        ";
        assert_eq!(run(src).0, "120\n");
    }

    #[test]
    fn function_values_display_by_name() {
        assert_eq!(run("fun f() {} print f;").0, "<fn f>\n");
    }

    #[test]
    fn local_functions_can_be_called_inside_their_scope() {
        let src = r"
            {
                fun double(n) {
                    return n + n;
                }
                print double(4);
            }
        ";
        assert_eq!(run(src).0, "8\n");
    }

    #[test]
    fn native_clock_is_available_as_a_global_function() {
        assert_eq!(run("print clock() >= 0;").0, "true\n");
    }

    #[test]
    fn call_arity_mismatch_is_runtime_error() {
        let VmError::Runtime { message, line } = run_err("fun f(a, b) {} f(1);");
        assert_eq!(message, "Expected 2 arguments but got 1.");
        assert_eq!(line, 1);
    }

    #[test]
    fn calling_non_callable_value_is_runtime_error() {
        let VmError::Runtime { message, line } = run_err("1();");
        assert_eq!(message, "Can only call functions and classes.");
        assert_eq!(line, 1);
    }

    #[test]
    fn return_is_not_allowed_at_top_level() {
        let errs = compile_err("return 1;");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Can't return from top-level code."
            )),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn function_declarations_require_expected_punctuation() {
        let errs = compile_err("fun f { print 1; }");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Expect '(' after function name."
            )),
            "errors were: {errs:?}"
        );

        let errs = compile_err("fun f(a, ) {}");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                LoxError::Parse { message, .. } if message == "Expect parameter name."
            )),
            "errors were: {errs:?}"
        );
    }

    #[test]
    fn compiler_emits_call_opcode_for_function_calls() {
        let (chunk, _) = compile_ok("fun f() {} f();");
        assert!(chunk.code.contains(&(OpCode::Call as u8)));
    }

    #[test]
    fn closure_captures_nearest_enclosing_local() {
        let src = r#"
            var x = "global";
            fun outer() {
                var x = "outer";
                fun inner() {
                    print x;
                }
                inner();
            }
            outer();
        "#;
        assert_eq!(run(src).0, "outer\n");
    }

    #[test]
    fn closure_keeps_local_alive_after_return() {
        let src = r#"
            fun makeClosure() {
                var local = "local";
                fun closure() {
                    print local;
                }
                return closure;
            }

            var closure = makeClosure();
            closure();
        "#;
        assert_eq!(run(src).0, "local\n");
    }

    #[test]
    fn closures_capture_variables_not_values() {
        let src = r#"
            var globalSet;
            var globalGet;

            fun main() {
                var a = "initial";
                fun set() { a = "updated"; }
                fun get() { print a; }
                globalSet = set;
                globalGet = get;
            }

            main();
            globalSet();
            globalGet();
        "#;
        assert_eq!(run(src).0, "updated\n");
    }

    #[test]
    fn nested_closures_capture_through_intermediate_function() {
        let src = r#"
            fun outer() {
                var a = "outer";
                fun middle() {
                    fun inner() {
                        print a;
                    }
                    return inner;
                }
                return middle;
            }

            var middle = outer();
            var inner = middle();
            inner();
        "#;
        assert_eq!(run(src).0, "outer\n");
    }

    #[test]
    fn closed_block_local_survives_after_scope_exits() {
        let src = r#"
            var global;
            {
                var a = "block";
                fun f() {
                    print a;
                }
                global = f;
            }
            global();
        "#;
        assert_eq!(run(src).0, "block\n");
    }

    #[test]
    fn captured_local_assignment_is_shared_between_calls() {
        let src = r"
            fun makeCounter() {
                var i = 0;
                fun count() {
                    i = i + 1;
                    print i;
                }
                return count;
            }

            var counter = makeCounter();
            counter();
            counter();
        ";
        assert_eq!(run(src).0, "1\n2\n");
    }

    #[test]
    fn local_recursive_function_captures_own_binding() {
        let src = r"
            {
                fun fib(n) {
                    if (n <= 1) return n;
                    return fib(n - 2) + fib(n - 1);
                }
                print fib(6);
            }
        ";
        assert_eq!(run(src).0, "8\n");
    }

    #[test]
    fn compiler_emits_closure_and_upvalue_opcodes_for_captures() {
        let (chunk, heap) = compile_ok(
            r#"
            fun outer() {
                var a = "capture";
                fun inner() {
                    print a;
                }
                return inner;
            }
            "#,
        );

        assert!(chunk.code.contains(&(OpCode::Closure as u8)));
        let function = chunk.constants.iter().find_map(|value| {
            let Value::Obj(handle) = value else {
                return None;
            };
            let function = heap.as_function(*handle)?;
            (function.display_name() == "outer").then_some(function)
        });
        let outer = function.expect("outer function constant exists");
        assert!(outer.chunk.code.contains(&(OpCode::Closure as u8)));
        let inner = outer.chunk.constants.iter().find_map(|value| {
            let Value::Obj(handle) = value else {
                return None;
            };
            let function = heap.as_function(*handle)?;
            (function.display_name() == "inner").then_some(function)
        });
        assert!(
            inner
                .expect("inner function constant exists")
                .chunk
                .code
                .contains(&(OpCode::GetUpvalue as u8))
        );
    }

    #[test]
    fn compiler_emits_close_upvalue_for_captured_block_locals() {
        let (chunk, _) = compile_ok(
            r#"
            var global;
            {
                var a = "block";
                fun f() { print a; }
                global = f;
            }
            "#,
        );
        assert!(chunk.code.contains(&(OpCode::CloseUpvalue as u8)));
    }
}
