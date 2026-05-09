//! Callable values (chapter 10): user-defined `LoxFunction`s and built-in
//! `NativeFn`s, surfaced uniformly through the [`Callable`] enum that
//! [`crate::value::Value::Callable`] wraps.
//!
//! Both variants are stored behind `Rc` so cloning a `Value::Callable`
//! doesn't deep-copy the AST or the captured closure environment.

use std::fmt;
use std::rc::Rc;

use crate::ast::Stmt;
use crate::environment::Environment;
use crate::error::LoxError;
use crate::token::Token;
use crate::value::Value;

/// Native functions implemented in Rust. The interpreter registers these
/// in the global scope (see `Interpreter::new`).
pub struct NativeFn {
    pub name: String,
    pub arity: usize,
    pub func: fn(&[Value]) -> Result<Value, LoxError>,
}

impl fmt::Debug for NativeFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeFn")
            .field("name", &self.name)
            .field("arity", &self.arity)
            .finish_non_exhaustive()
    }
}

/// User-defined function declaration plus the environment it captured at
/// definition time. The `decl` is shared via `Rc` so calling a function
/// many times doesn't reclone the AST.
#[derive(Debug)]
pub struct LoxFunction {
    pub decl: Rc<FunctionDecl>,
    pub closure: Environment,
}

/// The static parts of a function declaration — the AST identity that
/// makes equality cheap (`Rc::ptr_eq`).
#[derive(Debug)]
pub struct FunctionDecl {
    pub name: Token,
    pub params: Vec<Token>,
    pub body: Vec<Stmt>,
}

/// A unified callable: either a Rust-implemented native or a user
/// `LoxFunction`. Identity-based equality (`Rc::ptr_eq`) is sufficient
/// because Lox doesn't define structural function equality.
#[derive(Debug, Clone)]
pub enum Callable {
    Native(Rc<NativeFn>),
    Function(Rc<LoxFunction>),
}

impl PartialEq for Callable {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Native(a), Self::Native(b)) => Rc::ptr_eq(a, b),
            (Self::Function(a), Self::Function(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Callable {
    #[must_use]
    pub fn arity(&self) -> usize {
        match self {
            Self::Native(n) => n.arity,
            Self::Function(f) => f.decl.params.len(),
        }
    }
}
