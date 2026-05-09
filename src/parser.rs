//! Pratt parser for Lox expressions (chapter 6) extended with statements,
//! variable declarations, and assignment (chapter 8), control flow and
//! short-circuit logical operators (chapter 9), and function declarations,
//! calls, and `return` (chapter 10). `for` loops are desugared at parse
//! time into the existing `Block` + `While` nodes.

use std::rc::Rc;

use crate::ast::{Expr, FunctionDecl, Stmt};
use crate::error::LoxError;
use crate::token::{Literal, Token, TokenType};
use crate::value::Value;

type Bp = u8;

struct Pos<'t> {
    toks: &'t [Token],
    i: usize,
}

impl<'t> Pos<'t> {
    fn new(toks: &'t [Token]) -> Self {
        Self { toks, i: 0 }
    }
    fn head(&self) -> &'t Token {
        &self.toks[self.i]
    }
    fn previous(&self) -> &'t Token {
        &self.toks[self.i - 1]
    }
    fn eof(&self) -> bool {
        self.head().ttype == TokenType::Eof
    }
    fn bump(&mut self) {
        // Saturate at the trailing EOF token so callers like `synchronize`
        // can advance defensively without ever indexing past the end of
        // the token slice.
        if !self.eof() {
            self.i += 1;
        }
    }
    fn check(&self, t: TokenType) -> bool {
        !self.eof() && self.head().ttype == t
    }
    /// Consume the head token if it has type `t` and return whether we did.
    /// Mirrors `Iterator::next_if` for `TokenType`.
    fn eat(&mut self, t: TokenType) -> bool {
        if self.check(t) {
            self.bump();
            true
        } else {
            false
        }
    }
}

fn prefix_bp(t: TokenType) -> Option<Bp> {
    if matches!(t, TokenType::Bang | TokenType::Minus) {
        // Bumped to 15 to stay above the new factor rung at 11/12.
        Some(15)
    } else {
        None
    }
}

fn infix_bp(t: TokenType) -> Option<(Bp, Bp)> {
    // Lox precedence, lowest binding to highest. Each rung leaves a gap so
    // the prefix unary at 15 has room to live above the highest infix.
    let pair = match t {
        TokenType::Or => (1, 2),
        TokenType::And => (3, 4),
        TokenType::EqualEqual | TokenType::BangEqual => (5, 6),
        TokenType::Greater | TokenType::GreaterEqual | TokenType::Less | TokenType::LessEqual => {
            (7, 8)
        }
        TokenType::Plus | TokenType::Minus => (9, 10),
        TokenType::Slash | TokenType::Star => (11, 12),
        _ => return None,
    };
    Some(pair)
}

fn lit_value(lit: Option<&Literal>) -> Value {
    match lit {
        Some(Literal::Number(n)) => Value::Number(*n),
        Some(Literal::String(s)) => Value::String(s.clone()),
        _ => unreachable!("scanner emits a literal for every NUMBER/STRING token"),
    }
}

fn expect(p: &mut Pos<'_>, want: TokenType, msg: impl Into<String>) -> Result<(), LoxError> {
    if !p.eof() && p.head().ttype == want {
        p.bump();
        Ok(())
    } else {
        Err(LoxError::parse(p.head(), msg))
    }
}

fn parse_atom(p: &mut Pos<'_>) -> Result<Expr, LoxError> {
    let tok = p.head();
    let value = match tok.ttype {
        TokenType::False => Value::Bool(false),
        TokenType::True => Value::Bool(true),
        TokenType::Nil => Value::Nil,
        TokenType::Number | TokenType::String => lit_value(tok.literal.as_ref()),
        TokenType::Identifier => {
            let name = tok.clone();
            p.bump();
            return Ok(Expr::Variable(name));
        }
        TokenType::LeftParen => {
            p.bump();
            let inner = assignment(p)?;
            expect(p, TokenType::RightParen, "Expect ')' after expression.")?;
            return Ok(Expr::Grouping(Box::new(inner)));
        }
        _ => return Err(LoxError::parse(tok, "Expect expression.")),
    };
    p.bump();
    Ok(Expr::Literal(value))
}

fn parse_bp(p: &mut Pos<'_>, min_bp: Bp) -> Result<Expr, LoxError> {
    let head_ty = p.head().ttype;
    let mut lhs = if let Some(rbp) = prefix_bp(head_ty) {
        let op = p.head().clone();
        p.bump();
        let right = parse_bp(p, rbp)?;
        Expr::Unary {
            op,
            right: Box::new(right),
        }
    } else {
        parse_atom(p)?
    };

    loop {
        // Postfix call: `(` immediately after a primary/call expression.
        // Binds tighter than any infix, so it's checked before infix_bp.
        if p.check(TokenType::LeftParen) {
            p.bump();
            let arguments = call_arguments(p)?;
            let paren = p.head().clone();
            expect(p, TokenType::RightParen, "Expect ')' after arguments.")?;
            lhs = Expr::Call {
                callee: Box::new(lhs),
                paren,
                arguments,
            };
            continue;
        }

        let Some((lbp, rbp)) = infix_bp(p.head().ttype) else {
            break;
        };
        if lbp < min_bp {
            break;
        }
        let op = p.head().clone();
        p.bump();
        let rhs = parse_bp(p, rbp)?;
        // `and` / `or` go into a dedicated AST variant because they
        // short-circuit and return the operand value (not a coerced bool).
        lhs = if matches!(op.ttype, TokenType::And | TokenType::Or) {
            Expr::Logical {
                left: Box::new(lhs),
                op,
                right: Box::new(rhs),
            }
        } else {
            Expr::Binary {
                left: Box::new(lhs),
                op,
                right: Box::new(rhs),
            }
        };
    }
    Ok(lhs)
}

/// Parse a comma-separated argument list, stopping at the closing `)`.
/// Lox caps arguments at 255 to mirror jlox's compatibility with the
/// upcoming bytecode VM (chapters 14+); we report but don't fail on the
/// 256th argument so the rest of the program still parses.
fn call_arguments(p: &mut Pos<'_>) -> Result<Vec<Expr>, LoxError> {
    let mut args = Vec::new();
    if !p.check(TokenType::RightParen) {
        loop {
            if args.len() >= 255 {
                // Diagnostic only — we still consume the argument so
                // synchronization doesn't break further down.
                return Err(LoxError::parse(
                    p.head(),
                    "Can't have more than 255 arguments.",
                ));
            }
            args.push(assignment(p)?);
            if !p.eat(TokenType::Comma) {
                break;
            }
        }
    }
    Ok(args)
}

/// Parse an assignment expression. Assignment is right-associative and has
/// lower precedence than every binary operator, so we delegate to the Pratt
/// engine for the LHS, then optionally fold a trailing `= rhs` into an
/// `Expr::Assign` after validating that the LHS is an l-value (a bare
/// `Variable`). Anything else produces a parse error at the `=` token.
fn assignment(p: &mut Pos<'_>) -> Result<Expr, LoxError> {
    let expr = parse_bp(p, 0)?;
    if p.check(TokenType::Equal) {
        let equals = p.head().clone();
        p.bump();
        let value = assignment(p)?;
        return match expr {
            Expr::Variable(name) => Ok(Expr::Assign {
                name,
                value: Box::new(value),
            }),
            _ => Err(LoxError::parse(&equals, "Invalid assignment target.")),
        };
    }
    Ok(expr)
}

/// Parse a single Lox expression — entry point used by chapter 7's
/// expression-only tests. For programs use [`parse_program`].
///
/// # Errors
///
/// Returns the first parse error encountered. Trailing tokens after a
/// complete expression are reported as `Expect end of expression.`.
pub fn parse(tokens: &[Token]) -> Result<Expr, LoxError> {
    let mut p = Pos::new(tokens);
    let expr = assignment(&mut p)?;
    if !p.eof() {
        return Err(LoxError::parse(p.head(), "Expect end of expression."));
    }
    Ok(expr)
}

// --- statements (chapter 8) ---

fn declaration(p: &mut Pos<'_>) -> Result<Stmt, LoxError> {
    if p.eat(TokenType::Fun) {
        return function(p, "function");
    }
    if p.eat(TokenType::Var) {
        return var_declaration(p);
    }
    statement(p)
}

/// Parse a function declaration. `kind` is "function" for top-level
/// `fun` declarations; chapter 12 will reuse this with kind = "method".
fn function(p: &mut Pos<'_>, kind: &str) -> Result<Stmt, LoxError> {
    if !p.check(TokenType::Identifier) {
        return Err(LoxError::parse(p.head(), format!("Expect {kind} name.")));
    }
    let name = p.head().clone();
    p.bump();
    expect(
        p,
        TokenType::LeftParen,
        format!("Expect '(' after {kind} name."),
    )?;
    let mut params: Vec<Token> = Vec::new();
    if !p.check(TokenType::RightParen) {
        loop {
            if params.len() >= 255 {
                return Err(LoxError::parse(
                    p.head(),
                    "Can't have more than 255 parameters.",
                ));
            }
            if !p.check(TokenType::Identifier) {
                return Err(LoxError::parse(p.head(), "Expect parameter name."));
            }
            params.push(p.head().clone());
            p.bump();
            if !p.eat(TokenType::Comma) {
                break;
            }
        }
    }
    expect(p, TokenType::RightParen, "Expect ')' after parameters.")?;
    expect(
        p,
        TokenType::LeftBrace,
        format!("Expect '{{' before {kind} body."),
    )?;
    let body = block(p)?;
    Ok(Stmt::Function(Rc::new(FunctionDecl { name, params, body })))
}

fn var_declaration(p: &mut Pos<'_>) -> Result<Stmt, LoxError> {
    if !p.check(TokenType::Identifier) {
        return Err(LoxError::parse(p.head(), "Expect variable name."));
    }
    let name = p.head().clone();
    p.bump();
    let initializer = if p.eat(TokenType::Equal) {
        Some(assignment(p)?)
    } else {
        None
    };
    expect(
        p,
        TokenType::Semicolon,
        "Expect ';' after variable declaration.",
    )?;
    Ok(Stmt::Var { name, initializer })
}

fn statement(p: &mut Pos<'_>) -> Result<Stmt, LoxError> {
    if p.eat(TokenType::If) {
        return if_statement(p);
    }
    if p.eat(TokenType::While) {
        return while_statement(p);
    }
    if p.eat(TokenType::For) {
        return for_statement(p);
    }
    if p.eat(TokenType::Print) {
        return print_statement(p);
    }
    if p.check(TokenType::Return) {
        return return_statement(p);
    }
    if p.eat(TokenType::LeftBrace) {
        return Ok(Stmt::Block(block(p)?));
    }
    expression_statement(p)
}

fn return_statement(p: &mut Pos<'_>) -> Result<Stmt, LoxError> {
    let keyword = p.head().clone();
    p.bump();
    let value = if p.check(TokenType::Semicolon) {
        None
    } else {
        Some(assignment(p)?)
    };
    expect(p, TokenType::Semicolon, "Expect ';' after return value.")?;
    Ok(Stmt::Return { keyword, value })
}

fn if_statement(p: &mut Pos<'_>) -> Result<Stmt, LoxError> {
    expect(p, TokenType::LeftParen, "Expect '(' after 'if'.")?;
    let condition = assignment(p)?;
    expect(p, TokenType::RightParen, "Expect ')' after if condition.")?;
    let then_branch = Box::new(statement(p)?);
    // The `else` keyword binds to the *nearest* preceding `if` because
    // `statement` is called recursively — the textbook dangling-else
    // resolution falls out of the recursive descent for free.
    let else_branch = if p.eat(TokenType::Else) {
        Some(Box::new(statement(p)?))
    } else {
        None
    };
    Ok(Stmt::If {
        condition,
        then_branch,
        else_branch,
    })
}

fn while_statement(p: &mut Pos<'_>) -> Result<Stmt, LoxError> {
    expect(p, TokenType::LeftParen, "Expect '(' after 'while'.")?;
    let condition = assignment(p)?;
    expect(p, TokenType::RightParen, "Expect ')' after condition.")?;
    let body = Box::new(statement(p)?);
    Ok(Stmt::While { condition, body })
}

/// Parse a `for` loop and *desugar* it on the spot:
///
/// ```text
/// for (init; cond; incr) body
/// ⇒
/// {
///   init
///   while (cond) {
///     body
///     incr;
///   }
/// }
/// ```
///
/// Each of `init`, `cond`, `incr` is optional; an omitted condition
/// defaults to `true`. This matches jlox `Parser.forStatement` and means
/// the interpreter never grows a dedicated `For` node.
fn for_statement(p: &mut Pos<'_>) -> Result<Stmt, LoxError> {
    expect(p, TokenType::LeftParen, "Expect '(' after 'for'.")?;

    let initializer: Option<Stmt> = if p.eat(TokenType::Semicolon) {
        None
    } else if p.eat(TokenType::Var) {
        Some(var_declaration(p)?)
    } else {
        Some(expression_statement(p)?)
    };

    let condition: Expr = if p.check(TokenType::Semicolon) {
        Expr::Literal(Value::Bool(true))
    } else {
        assignment(p)?
    };
    expect(p, TokenType::Semicolon, "Expect ';' after loop condition.")?;

    let increment: Option<Expr> = if p.check(TokenType::RightParen) {
        None
    } else {
        Some(assignment(p)?)
    };
    expect(p, TokenType::RightParen, "Expect ')' after for clauses.")?;

    let mut body = statement(p)?;

    // body = { body; increment; }
    if let Some(incr) = increment {
        body = Stmt::Block(vec![body, Stmt::Expression(incr)]);
    }

    // body = while (cond) body
    body = Stmt::While {
        condition,
        body: Box::new(body),
    };

    // body = { initializer; body }
    if let Some(init) = initializer {
        body = Stmt::Block(vec![init, body]);
    }

    Ok(body)
}

fn print_statement(p: &mut Pos<'_>) -> Result<Stmt, LoxError> {
    let value = assignment(p)?;
    expect(p, TokenType::Semicolon, "Expect ';' after value.")?;
    Ok(Stmt::Print(value))
}

fn expression_statement(p: &mut Pos<'_>) -> Result<Stmt, LoxError> {
    let expr = assignment(p)?;
    expect(p, TokenType::Semicolon, "Expect ';' after expression.")?;
    Ok(Stmt::Expression(expr))
}

fn block(p: &mut Pos<'_>) -> Result<Vec<Stmt>, LoxError> {
    let mut stmts = Vec::new();
    while !p.check(TokenType::RightBrace) && !p.eof() {
        stmts.push(declaration(p)?);
    }
    expect(p, TokenType::RightBrace, "Expect '}' after block.")?;
    Ok(stmts)
}

/// Skip tokens until we reach a likely statement boundary — either just past
/// a `;` or sitting on a keyword that starts a new statement. Used after a
/// parse error so [`parse_program`] can report multiple errors per run.
fn synchronize(p: &mut Pos<'_>) {
    p.bump();
    while !p.eof() {
        if p.previous().ttype == TokenType::Semicolon {
            return;
        }
        match p.head().ttype {
            TokenType::Class
            | TokenType::Fun
            | TokenType::Var
            | TokenType::For
            | TokenType::If
            | TokenType::While
            | TokenType::Print
            | TokenType::Return => return,
            _ => p.bump(),
        }
    }
}

/// Parse a token stream into a list of top-level statements (a program).
///
/// On parse failure this collects every error encountered, calling
/// [`synchronize`] between failures so a single mistake doesn't suppress
/// later diagnostics.
///
/// # Errors
///
/// Returns the accumulated parse errors when at least one statement
/// failed to parse.
pub fn parse_program(tokens: &[Token]) -> Result<Vec<Stmt>, Vec<LoxError>> {
    let mut p = Pos::new(tokens);
    let mut stmts = Vec::new();
    let mut errors = Vec::new();
    while !p.eof() {
        match declaration(&mut p) {
            Ok(s) => stmts.push(s),
            Err(e) => {
                errors.push(e);
                synchronize(&mut p);
            }
        }
    }
    if errors.is_empty() {
        Ok(stmts)
    } else {
        Err(errors)
    }
}
