//! Lexically scoped variable environment (chapter 8, reworked in chapter 10
//! around `Rc<RefCell<Scope>>` parent pointers so closures can outlive
//! their declaring scope).
//!
//! An `Environment` is a thin handle (`Rc`) over a `Scope { values, parent }`
//! cell. Cloning an `Environment` is a refcount bump; mutation goes through
//! interior mutability so all the read/write methods take `&self`.
//!
//! Lookup walks from the innermost scope outward via the `parent` chain;
//! `define` always adds to the current scope; `assign` walks outward and
//! errors at the global root if the name is unknown.
//!
//! # Why the rework was needed
//!
//! Chapter 8 used a `Vec<HashMap>` stack inside the interpreter because no
//! value escaped its declaring scope. Chapter 10 introduces functions, and
//! a function captures the environment in which it was *defined*; that
//! environment must remain reachable after the surrounding block exits.
//! `Rc<RefCell<Scope>>` is the textbook solution and matches jlox's
//! `Environment(enclosing)` constructor.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::error::LoxError;
use crate::token::Token;
use crate::value::Value;

/// A handle to a variable scope. Cloning is cheap (refcount bump) and
/// shared mutation goes through interior `RefCell`.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    inner: Rc<RefCell<Scope>>,
}

#[derive(Debug, Default)]
struct Scope {
    values: HashMap<String, Value>,
    /// `None` for the global scope, `Some(parent)` for any nested scope.
    parent: Option<Environment>,
}

impl Environment {
    /// Construct a fresh global environment with no parent.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a child environment whose parent is `self`. Callers typically
    /// use this when entering a `{ ... }` block, when binding a function's
    /// parameters, or when capturing a closure.
    #[must_use]
    pub fn child(&self) -> Self {
        Self {
            inner: Rc::new(RefCell::new(Scope {
                values: HashMap::new(),
                parent: Some(self.clone()),
            })),
        }
    }

    /// Define a variable in this scope. Re-declaration in the same scope
    /// shadows the previous binding (matches jlox `Environment.define`).
    pub fn define(&self, name: impl Into<String>, value: Value) {
        self.inner.borrow_mut().values.insert(name.into(), value);
    }

    /// Look up a variable, walking outward through parent scopes.
    ///
    /// # Errors
    ///
    /// Returns a runtime error if the variable is undefined.
    pub fn get(&self, name: &Token) -> Result<Value, LoxError> {
        // Try the current scope first.
        if let Some(v) = self.inner.borrow().values.get(&name.lexeme) {
            return Ok(v.clone());
        }
        // Drop the borrow before recursing so a deeply nested chain
        // doesn't accumulate live `Ref` borrows.
        let parent = self.inner.borrow().parent.clone();
        match parent {
            Some(p) => p.get(name),
            None => Err(LoxError::runtime(
                name,
                format!("Undefined variable '{}'.", name.lexeme),
            )),
        }
    }

    /// Assign to an existing variable; walks outward, never creates a new
    /// binding. Returns the assigned value so callers can use assignment
    /// as an expression.
    ///
    /// # Errors
    ///
    /// Returns a runtime error if the variable is undefined.
    pub fn assign(&self, name: &Token, value: Value) -> Result<Value, LoxError> {
        {
            let mut scope = self.inner.borrow_mut();
            if scope.values.contains_key(&name.lexeme) {
                scope.values.insert(name.lexeme.clone(), value.clone());
                return Ok(value);
            }
        }
        let parent = self.inner.borrow().parent.clone();
        match parent {
            Some(p) => p.assign(name, value),
            None => Err(LoxError::runtime(
                name,
                format!("Undefined variable '{}'.", name.lexeme),
            )),
        }
    }
}
