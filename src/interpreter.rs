//! Tree-walk interpreter (chapter 7) extended with statements + variable
//! scoping (chapter 8) and control flow + short-circuit logical operators
//! (chapter 9).
//!
//! Public surface:
//!
//! - [`evaluate`] — evaluate a single expression in a fresh empty
//!   environment. Convenient for unit tests and for chapter-7 callers; any
//!   variable read or assignment will fail with `Undefined variable` since
//!   no bindings exist.
//! - [`Interpreter`] — stateful walker that owns an [`Environment`] and a
//!   writer for `print` output. Use [`Interpreter::interpret`] to run a
//!   program (a slice of [`Stmt`]).

use std::io::Write;

use crate::ast::{Expr, Stmt};
use crate::environment::Environment;
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

/// Evaluate `expr` in a fresh, empty [`Environment`]. Convenience used by
/// expression-only tests; variable reads / assignments fail with
/// `Undefined variable` because no bindings have been introduced.
///
/// # Errors
///
/// Returns a runtime error on type mismatches or undefined variables.
pub fn evaluate(expr: &Expr) -> Result<Value, LoxError> {
    let mut env = Environment::new();
    evaluate_in(expr, &mut env)
}

/// Evaluate `expr` against a caller-supplied [`Environment`]. Public so
/// external tests and downstream chapters can drive expression evaluation
/// with a populated scope.
///
/// # Errors
///
/// Returns a runtime error on type mismatches or undefined variables.
pub fn evaluate_in(expr: &Expr, env: &mut Environment) -> Result<Value, LoxError> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::Grouping(inner) => evaluate_in(inner, env),
        Expr::Unary { op, right } => {
            let r = evaluate_in(right, env)?;
            eval_unary(op, &r)
        }
        Expr::Binary { left, op, right } => {
            let l = evaluate_in(left, env)?;
            let r = evaluate_in(right, env)?;
            eval_binary(op, l, r)
        }
        Expr::Variable(name) => env.get(name),
        Expr::Assign { name, value } => {
            let v = evaluate_in(value, env)?;
            env.assign(name, v)
        }
        Expr::Logical { left, op, right } => {
            // Short-circuit: evaluate the left operand first, decide
            // whether to evaluate the right based on truthiness, and
            // return the *operand* itself rather than a coerced bool
            // (e.g. `nil or "x"` evaluates to `"x"`, `1 and 2` to `2`).
            let l = evaluate_in(left, env)?;
            match op.ttype {
                TokenType::Or if is_truthy(&l) => Ok(l),
                TokenType::And if !is_truthy(&l) => Ok(l),
                TokenType::Or | TokenType::And => evaluate_in(right, env),
                _ => unreachable!(
                    "parser does not produce {:?} as a logical operator",
                    op.ttype
                ),
            }
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

/// Stateful tree-walker. Owns the variable [`Environment`] and a writer
/// that receives `print` output. Construct with [`Interpreter::new`] and
/// drive with [`Interpreter::interpret`].
pub struct Interpreter<'w> {
    env: Environment,
    out: &'w mut dyn Write,
}

impl<'w> Interpreter<'w> {
    /// Create an interpreter that writes `print` output to `out`.
    pub fn new(out: &'w mut dyn Write) -> Self {
        Self {
            env: Environment::new(),
            out,
        }
    }

    /// Run a program (slice of statements), short-circuiting on the first
    /// runtime error. State (the [`Environment`]) persists across calls so
    /// the same `Interpreter` can drive a multi-line REPL.
    ///
    /// # Errors
    ///
    /// Returns the first runtime error encountered.
    pub fn interpret(&mut self, stmts: &[Stmt]) -> Result<(), LoxError> {
        for s in stmts {
            self.execute(s)?;
        }
        Ok(())
    }

    fn execute(&mut self, stmt: &Stmt) -> Result<(), LoxError> {
        match stmt {
            Stmt::Expression(e) => {
                evaluate_in(e, &mut self.env)?;
                Ok(())
            }
            Stmt::Print(e) => {
                let v = evaluate_in(e, &mut self.env)?;
                // Mirror jlox's `System.out.println`, which silently
                // discards IO errors (e.g. broken pipe).
                let _ = writeln!(self.out, "{}", stringify(&v));
                Ok(())
            }
            Stmt::Var { name, initializer } => {
                let value = match initializer {
                    Some(e) => evaluate_in(e, &mut self.env)?,
                    None => Value::Nil,
                };
                self.env.define(name.lexeme.clone(), value);
                Ok(())
            }
            Stmt::Block(stmts) => self.execute_block(stmts),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                if is_truthy(&evaluate_in(condition, &mut self.env)?) {
                    self.execute(then_branch)
                } else if let Some(alt) = else_branch {
                    self.execute(alt)
                } else {
                    Ok(())
                }
            }
            Stmt::While { condition, body } => {
                // Re-evaluate the condition every iteration; the body may
                // mutate variables it reads from.
                while is_truthy(&evaluate_in(condition, &mut self.env)?) {
                    self.execute(body)?;
                }
                Ok(())
            }
        }
    }

    fn execute_block(&mut self, stmts: &[Stmt]) -> Result<(), LoxError> {
        self.env.push();
        // `try_for_each` short-circuits on the first error, mirroring the
        // book's early-return behaviour without an explicit IIFE wrapper.
        let result = stmts.iter().try_for_each(|s| self.execute(s));
        // Always pop, even on error, so the environment isn't left
        // permanently nested after a runtime fault.
        self.env.pop();
        result
    }
}
