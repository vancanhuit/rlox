//! Tree-walk interpreter (chapter 7) extended with statements + variable
//! scoping (chapter 8), control flow (chapter 9), and functions +
//! closures + `return` (chapter 10).
//!
//! # `return` as control flow
//!
//! jlox uses a Java exception to bubble `return` values out of the
//! interpreter and back to the call site. We use the analogous Rust
//! pattern: a private `InterpError` enum with a non-error `Return(Value)`
//! variant that the function-call site catches and converts into the
//! function's return value. Any `Return` that escapes a public boundary
//! is converted back into a real [`LoxError`].

use std::io::{self, Write};
use std::mem;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ast::{Expr, Stmt};
use crate::callable::{Callable, FunctionDecl, LoxFunction, NativeFn};
use crate::environment::Environment;
use crate::error::LoxError;
use crate::token::{Token, TokenType};
use crate::value::Value;

// ---- private control-flow plumbing ---------------------------------------

/// Internal result type for the interpreter. `Return(Value)` is a
/// non-error short-circuit caught by `call_function`.
enum InterpError {
    Runtime(LoxError),
    Return(Value),
}

impl From<LoxError> for InterpError {
    fn from(e: LoxError) -> Self {
        Self::Runtime(e)
    }
}

type InterpRes<T> = std::result::Result<T, InterpError>;

/// Convert any `InterpError` that escapes a public boundary back into a
/// `LoxError`. A bare `return` outside a function is a runtime error;
/// the chapter-11 resolver will reject this statically before it can
/// reach the interpreter.
fn into_lox_error(e: InterpError) -> LoxError {
    match e {
        InterpError::Runtime(err) => err,
        InterpError::Return(_) => LoxError::Runtime {
            line: 0,
            message: "Can't return from top-level code.".to_string(),
        },
    }
}

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
        // Callables use identity equality (`Rc::ptr_eq`); see callable.rs.
        (Value::Callable(x), Value::Callable(y)) => x == y,
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
/// Function calls work — a temporary [`Interpreter`] backed by
/// [`io::sink`] is constructed under the hood, so any `print` side
/// effects are silently discarded.
///
/// # Errors
///
/// Returns a runtime error on type mismatches or undefined variables.
pub fn evaluate(expr: &Expr) -> Result<Value, LoxError> {
    let env = Environment::new();
    evaluate_in(expr, &env)
}

/// Evaluate `expr` against a caller-supplied [`Environment`]. Public so
/// external tests and downstream chapters can drive expression evaluation
/// with a populated scope.
///
/// # Errors
///
/// Returns a runtime error on type mismatches or undefined variables.
pub fn evaluate_in(expr: &Expr, env: &Environment) -> Result<Value, LoxError> {
    let mut sink = io::sink();
    let mut interp = Interpreter::with_environment(&mut sink, env.clone());
    interp.eval(expr).map_err(into_lox_error)
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
        Value::Callable(Callable::Native(n)) => format!("<native fn {}>", n.name),
        Value::Callable(Callable::Function(f)) => format!("<fn {}>", f.decl.name.lexeme),
    }
}

/// Stateful tree-walker. Owns the variable [`Environment`] and a writer
/// that receives `print` output. Construct with [`Interpreter::new`] and
/// drive with [`Interpreter::interpret`].
pub struct Interpreter<'w> {
    /// Always-reachable global scope. Native functions live here; user
    /// `fun` declarations at the top level land here too. Currently unread
    /// at runtime because the parent-pointer chain already terminates at
    /// the globals — chapter 11 (resolver) will use this directly.
    #[allow(dead_code)]
    globals: Environment,
    /// The currently-active scope. Equal to `globals` outside any block /
    /// function call; otherwise a child of some ancestor scope.
    env: Environment,
    out: &'w mut dyn Write,
}

impl<'w> Interpreter<'w> {
    /// Create an interpreter that writes `print` output to `out`. Native
    /// functions (e.g. `clock`) are registered in the global scope.
    pub fn new(out: &'w mut dyn Write) -> Self {
        let globals = Environment::new();
        register_natives(&globals);
        let env = globals.clone();
        Self { globals, env, out }
    }

    /// Construct an interpreter with externally supplied globals (used by
    /// [`evaluate_in`] to thread caller state into expression
    /// evaluation).
    fn with_environment(out: &'w mut dyn Write, env: Environment) -> Self {
        let globals = env.clone();
        Self { globals, env, out }
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
            self.execute(s).map_err(into_lox_error)?;
        }
        Ok(())
    }

    fn execute(&mut self, stmt: &Stmt) -> InterpRes<()> {
        match stmt {
            Stmt::Expression(e) => {
                self.eval(e)?;
                Ok(())
            }
            Stmt::Print(e) => {
                let v = self.eval(e)?;
                let _ = writeln!(self.out, "{}", stringify(&v));
                Ok(())
            }
            Stmt::Var { name, initializer } => {
                let value = match initializer {
                    Some(e) => self.eval(e)?,
                    None => Value::Nil,
                };
                self.env.define(name.lexeme.clone(), value);
                Ok(())
            }
            Stmt::Block(stmts) => {
                let child = self.env.child();
                self.execute_block(stmts, child)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                if is_truthy(&self.eval(condition)?) {
                    self.execute(then_branch)
                } else if let Some(alt) = else_branch {
                    self.execute(alt)
                } else {
                    Ok(())
                }
            }
            Stmt::While { condition, body } => {
                while is_truthy(&self.eval(condition)?) {
                    self.execute(body)?;
                }
                Ok(())
            }
            Stmt::Function { name, params, body } => {
                let decl = Rc::new(FunctionDecl {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                });
                let func = LoxFunction {
                    decl,
                    // Capture the environment in effect at *declaration*
                    // time — closures see surrounding bindings even
                    // after the declaring scope exits.
                    closure: self.env.clone(),
                };
                self.env.define(
                    name.lexeme.clone(),
                    Value::Callable(Callable::Function(Rc::new(func))),
                );
                Ok(())
            }
            Stmt::Return { value, .. } => {
                let v = match value {
                    Some(e) => self.eval(e)?,
                    None => Value::Nil,
                };
                Err(InterpError::Return(v))
            }
        }
    }

    /// Execute `stmts` in `env`, restoring the previous environment even
    /// if execution short-circuits. Function bodies and explicit blocks
    /// share this entry point.
    fn execute_block(&mut self, stmts: &[Stmt], env: Environment) -> InterpRes<()> {
        let prev = mem::replace(&mut self.env, env);
        let result = stmts.iter().try_for_each(|s| self.execute(s));
        self.env = prev;
        result
    }

    fn eval(&mut self, expr: &Expr) -> InterpRes<Value> {
        match expr {
            Expr::Literal(v) => Ok(v.clone()),
            Expr::Grouping(inner) => self.eval(inner),
            Expr::Unary { op, right } => {
                let r = self.eval(right)?;
                Ok(eval_unary(op, &r)?)
            }
            Expr::Binary { left, op, right } => {
                let l = self.eval(left)?;
                let r = self.eval(right)?;
                Ok(eval_binary(op, l, r)?)
            }
            Expr::Variable(name) => Ok(self.env.get(name)?),
            Expr::Assign { name, value } => {
                let v = self.eval(value)?;
                Ok(self.env.assign(name, v)?)
            }
            Expr::Logical { left, op, right } => {
                let l = self.eval(left)?;
                match op.ttype {
                    TokenType::Or if is_truthy(&l) => Ok(l),
                    TokenType::And if !is_truthy(&l) => Ok(l),
                    TokenType::Or | TokenType::And => self.eval(right),
                    _ => unreachable!(
                        "parser does not produce {:?} as a logical operator",
                        op.ttype
                    ),
                }
            }
            Expr::Call {
                callee,
                paren,
                arguments,
            } => {
                let callee_value = self.eval(callee)?;
                let mut args = Vec::with_capacity(arguments.len());
                for a in arguments {
                    args.push(self.eval(a)?);
                }
                self.call(&callee_value, paren, args)
            }
        }
    }

    fn call(&mut self, callee: &Value, paren: &Token, args: Vec<Value>) -> InterpRes<Value> {
        let Value::Callable(callable) = callee else {
            return Err(LoxError::runtime(paren, "Can only call functions and classes.").into());
        };
        let arity = callable.arity();
        if args.len() != arity {
            return Err(LoxError::runtime(
                paren,
                format!("Expected {} arguments but got {}.", arity, args.len()),
            )
            .into());
        }
        match callable {
            Callable::Native(n) => Ok((n.func)(&args)?),
            Callable::Function(f) => self.call_function(f, args),
        }
    }

    fn call_function(&mut self, func: &LoxFunction, args: Vec<Value>) -> InterpRes<Value> {
        let env = func.closure.child();
        for (param, arg) in func.decl.params.iter().zip(args) {
            env.define(param.lexeme.clone(), arg);
        }
        match self.execute_block(&func.decl.body, env) {
            Ok(()) => Ok(Value::Nil),
            Err(InterpError::Return(v)) => Ok(v),
            Err(other) => Err(other),
        }
    }
}

// ---- native functions ----------------------------------------------------

fn register_natives(globals: &Environment) {
    let clock = NativeFn {
        name: "clock".to_string(),
        arity: 0,
        func: clock_native,
    };
    globals.define("clock", Value::Callable(Callable::Native(Rc::new(clock))));
}

/// `clock()` — seconds since the Unix epoch as a Lox `Number`. Matches
/// jlox's reference implementation; precision is whatever `f64` allows.
///
/// The `Result` wrap is required by the [`NativeFn::func`] signature
/// (other natives may legitimately fail), even though `clock` itself
/// can't.
#[allow(clippy::unnecessary_wraps)]
fn clock_native(_args: &[Value]) -> Result<Value, LoxError> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    Ok(Value::Number(secs))
}
