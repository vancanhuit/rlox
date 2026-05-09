//! Tree-walk interpreter for Lox expressions (chapter 7).

use crate::ast::Expr;
use crate::error::LoxError;
use crate::token::{Token, TokenType};
use crate::value::Value;

/// Lox truthiness: only `nil` and `false` are falsy.
fn is_truthy(v: &Value) -> bool {
    !matches!(v, Value::Nil | Value::Bool(false))
}

/// Lox equality: `nil` equals only `nil`; otherwise same-type comparison.
///
/// Numbers use exact `f64` equality to mirror jlox; the upstream test suite
/// skips NaN-equality for the Java port for the same reason.
#[allow(clippy::float_cmp)]
fn is_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        _ => false,
    }
}

fn need_number(op: &Token, v: &Value) -> Result<f64, LoxError> {
    match v {
        Value::Number(n) => Ok(*n),
        _ => Err(LoxError::runtime(op, "Operand must be a number.")),
    }
}

fn need_two_numbers(op: &Token, a: &Value, b: &Value) -> Result<(f64, f64), LoxError> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Ok((*x, *y)),
        _ => Err(LoxError::runtime(op, "Operands must be numbers.")),
    }
}

fn eval_unary(op: &Token, right: &Value) -> Result<Value, LoxError> {
    match op.ttype {
        TokenType::Bang => Ok(Value::Bool(!is_truthy(right))),
        TokenType::Minus => {
            let n = need_number(op, right)?;
            Ok(Value::Number(-n))
        }
        _ => unreachable!("parser only produces Bang or Minus as unary operators"),
    }
}

fn eval_binary(op: &Token, left: Value, right: Value) -> Result<Value, LoxError> {
    match op.ttype {
        TokenType::Plus => match (left, right) {
            (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
            (Value::String(a), Value::String(b)) => Ok(Value::String(a + &b)),
            _ => Err(LoxError::runtime(
                op,
                "Operands must be two numbers or two strings.",
            )),
        },
        TokenType::Minus => {
            let (a, b) = need_two_numbers(op, &left, &right)?;
            Ok(Value::Number(a - b))
        }
        TokenType::Star => {
            let (a, b) = need_two_numbers(op, &left, &right)?;
            Ok(Value::Number(a * b))
        }
        TokenType::Slash => {
            let (a, b) = need_two_numbers(op, &left, &right)?;
            Ok(Value::Number(a / b))
        }
        TokenType::Greater => {
            let (a, b) = need_two_numbers(op, &left, &right)?;
            Ok(Value::Bool(a > b))
        }
        TokenType::GreaterEqual => {
            let (a, b) = need_two_numbers(op, &left, &right)?;
            Ok(Value::Bool(a >= b))
        }
        TokenType::Less => {
            let (a, b) = need_two_numbers(op, &left, &right)?;
            Ok(Value::Bool(a < b))
        }
        TokenType::LessEqual => {
            let (a, b) = need_two_numbers(op, &left, &right)?;
            Ok(Value::Bool(a <= b))
        }
        TokenType::EqualEqual => Ok(Value::Bool(is_equal(&left, &right))),
        TokenType::BangEqual => Ok(Value::Bool(!is_equal(&left, &right))),
        _ => unreachable!(
            "parser does not produce {:?} as a binary operator",
            op.ttype
        ),
    }
}

/// Evaluate `expr` to a [`Value`] or return a runtime error.
pub fn evaluate(expr: &Expr) -> Result<Value, LoxError> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::Grouping(inner) => evaluate(inner),
        Expr::Unary { op, right } => {
            let r = evaluate(right)?;
            eval_unary(op, &r)
        }
        Expr::Binary { left, op, right } => {
            let l = evaluate(left)?;
            let r = evaluate(right)?;
            eval_binary(op, l, r)
        }
    }
}

/// Format a [`Value`] for user-facing output (matches the book's
/// `Interpreter.stringify`): whole numbers render without the trailing `.0`,
/// `nil`/`true`/`false` render as keywords, strings render unquoted.
#[must_use]
pub fn stringify(value: &Value) -> String {
    match value {
        Value::Nil => "nil".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if n.fract() == 0.0 && n.is_finite() {
                // Whole-number formatter: zero fractional digits.
                format!("{n:.0}")
            } else {
                format!("{n}")
            }
        }
        Value::String(s) => s.clone(),
    }
}
