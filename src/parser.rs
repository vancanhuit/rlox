//! Pratt parser for Lox expressions (chapter 6).

use crate::ast::Expr;
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
    fn eof(&self) -> bool {
        self.head().ttype == TokenType::Eof
    }
    fn bump(&mut self) {
        self.i += 1;
    }
}

fn prefix_bp(t: TokenType) -> Option<Bp> {
    if matches!(t, TokenType::Bang | TokenType::Minus) {
        Some(13)
    } else {
        None
    }
}

fn infix_bp(t: TokenType) -> Option<(Bp, Bp)> {
    let pair = match t {
        TokenType::EqualEqual | TokenType::BangEqual => (3, 4),
        TokenType::Greater | TokenType::GreaterEqual | TokenType::Less | TokenType::LessEqual => {
            (5, 6)
        }
        TokenType::Plus | TokenType::Minus => (7, 8),
        TokenType::Slash | TokenType::Star => (9, 10),
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

fn expect(p: &mut Pos<'_>, want: TokenType, msg: &str) -> Result<(), LoxError> {
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
        TokenType::LeftParen => {
            p.bump();
            let inner = parse_bp(p, 0)?;
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

    while let Some((lbp, rbp)) = infix_bp(p.head().ttype) {
        if lbp < min_bp {
            break;
        }
        let op = p.head().clone();
        p.bump();
        let rhs = parse_bp(p, rbp)?;
        lhs = Expr::Binary {
            left: Box::new(lhs),
            op,
            right: Box::new(rhs),
        };
    }
    Ok(lhs)
}

/// Parse a token stream produced by [`crate::scan`] into an expression AST.
pub fn parse(tokens: &[Token]) -> Result<Expr, LoxError> {
    let mut p = Pos::new(tokens);
    let expr = parse_bp(&mut p, 0)?;
    if !p.eof() {
        return Err(LoxError::parse(p.head(), "Expect end of expression."));
    }
    Ok(expr)
}
