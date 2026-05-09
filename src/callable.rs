//! Callable values: user-defined [`LoxFunction`]s, built-in [`NativeFn`]s,
//! and \u2014 from chapter 12 \u2014 [`LoxClass`]es. Calling a class returns a
//! [`LoxInstance`]. All three are surfaced uniformly through the
//! [`Callable`] enum that [`crate::value::Value::Callable`] wraps; the
//! resulting instance lives in [`crate::value::Value::Instance`].
//!
//! Every variant is stored behind `Rc` so cloning a runtime value
//! doesn't deep-copy the AST, the captured closure environment, or the
//! per-class method table.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::ast::FunctionDecl;
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
/// definition time. The `decl` is shared via `Rc` directly with the
/// originating [`crate::ast::Stmt::Function`] node so a call doesn't clone
/// the body — the resolver-recorded `Expr` addresses inside `body` stay
/// valid for the lifetime of the function value.
///
/// `is_initializer` flags the special `init` method on a class (chapter
/// 12). The interpreter forces an initializer call to return the
/// instance regardless of whether the body falls off the end or executes
/// an explicit bare `return;`.
#[derive(Debug)]
pub struct LoxFunction {
    pub decl: Rc<FunctionDecl>,
    pub closure: Environment,
    pub is_initializer: bool,
}

impl LoxFunction {
    /// Bind `instance` as `this` inside a fresh closure environment and
    /// return the bound method. The original method is left untouched
    /// (its closure is the parent of the new scope).
    #[must_use]
    pub fn bind(self: &Rc<Self>, instance: Rc<LoxInstance>) -> Rc<Self> {
        let env = self.closure.child();
        env.define("this", Value::Instance(instance));
        Rc::new(Self {
            decl: Rc::clone(&self.decl),
            closure: env,
            is_initializer: self.is_initializer,
        })
    }
}

/// A user-defined class (chapter 12, extended in chapter 13 with single
/// inheritance). Calling the class produces an instance; if the class
/// declares an `init` method, it runs as the constructor and the
/// class's arity is the initializer's arity.
#[derive(Debug)]
pub struct LoxClass {
    pub name: String,
    pub superclass: Option<Rc<LoxClass>>,
    pub methods: HashMap<String, Rc<LoxFunction>>,
}

impl LoxClass {
    /// Look up `name` first on this class, then up the superclass chain.
    /// Returns the first match.
    #[must_use]
    pub fn find_method(&self, name: &str) -> Option<Rc<LoxFunction>> {
        if let Some(m) = self.methods.get(name) {
            return Some(Rc::clone(m));
        }
        if let Some(sc) = &self.superclass {
            return sc.find_method(name);
        }
        None
    }

    #[must_use]
    pub fn arity(&self) -> usize {
        self.find_method("init").map_or(0, |m| m.decl.params.len())
    }
}

/// A live instance of a [`LoxClass`]. Field storage uses interior
/// mutability so `instance.x = 1` works through a shared `Rc`.
#[derive(Debug)]
pub struct LoxInstance {
    pub class: Rc<LoxClass>,
    pub fields: RefCell<HashMap<String, Value>>,
}

impl LoxInstance {
    #[must_use]
    pub fn new(class: Rc<LoxClass>) -> Rc<Self> {
        Rc::new(Self {
            class,
            fields: RefCell::new(HashMap::new()),
        })
    }

    /// Resolve `name` against this instance: fields shadow methods (per
    /// jlox), and a method lookup returns a freshly-bound copy whose
    /// closure carries `this`.
    ///
    /// # Errors
    ///
    /// Returns a runtime error when neither a field nor a method exists.
    pub fn get(self: &Rc<Self>, name: &Token) -> Result<Value, LoxError> {
        if let Some(v) = self.fields.borrow().get(&name.lexeme) {
            return Ok(v.clone());
        }
        if let Some(method) = self.class.find_method(&name.lexeme) {
            let bound = method.bind(Rc::clone(self));
            return Ok(Value::Callable(Callable::Function(bound)));
        }
        Err(LoxError::runtime(
            name,
            format!("Undefined property '{}'.", name.lexeme),
        ))
    }

    /// Write a field. Lox allows defining fields on the fly, so this
    /// always succeeds; no method-name collision check.
    pub fn set(&self, name: &Token, value: Value) {
        self.fields.borrow_mut().insert(name.lexeme.clone(), value);
    }
}

/// A unified callable: either a Rust-implemented native, a user
/// `LoxFunction`, or a `LoxClass` (which acts as its own constructor).
/// Identity-based equality (`Rc::ptr_eq`) is sufficient because Lox
/// doesn't define structural function equality.
#[derive(Debug, Clone)]
pub enum Callable {
    Native(Rc<NativeFn>),
    Function(Rc<LoxFunction>),
    Class(Rc<LoxClass>),
}

impl PartialEq for Callable {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Native(a), Self::Native(b)) => Rc::ptr_eq(a, b),
            (Self::Function(a), Self::Function(b)) => Rc::ptr_eq(a, b),
            (Self::Class(a), Self::Class(b)) => Rc::ptr_eq(a, b),
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
            Self::Class(c) => c.arity(),
        }
    }
}
