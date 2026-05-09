//! Lexically scoped variable environment (chapter 8).
//!
//! Implemented as a stack of hash maps: the bottom is the global scope, each
//! `{ ... }` block pushes a fresh scope, exiting the block pops it. This is
//! the simplest design that supports chapter 8's nested-block semantics.
//!
//! Chapter 10 will need to share a captured environment between a closure
//! and its enclosing scope, at which point this module will be reworked
//! around `Rc<RefCell<Scope>>` parent pointers. For chapters 8–9 the stack
//! suffices because no value outlives its declaring scope.

use std::collections::HashMap;

use crate::error::LoxError;
use crate::token::Token;
use crate::value::Value;

/// A stack of scopes. The first element is the global scope and is always
/// present; pushing creates a new innermost scope, popping discards it.
#[derive(Debug, Default, Clone)]
pub struct Environment {
    scopes: Vec<HashMap<String, Value>>,
}

impl Environment {
    /// Construct an environment containing only the global scope.
    #[must_use]
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    /// Push a fresh innermost scope (entering a `{ ... }` block).
    pub fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pop the innermost scope. The global scope is never popped.
    pub fn pop(&mut self) {
        debug_assert!(self.scopes.len() > 1, "must not pop the global scope");
        self.scopes.pop();
    }

    /// Define a variable in the innermost scope. Re-declaration in the same
    /// scope shadows the previous binding (matches jlox `Environment.define`).
    pub fn define(&mut self, name: impl Into<String>, value: Value) {
        let scope = self
            .scopes
            .last_mut()
            .expect("environment always has a global scope");
        scope.insert(name.into(), value);
    }

    /// Look up a variable, searching from innermost to outermost.
    ///
    /// # Errors
    ///
    /// Returns a runtime error if no scope contains a binding for `name`.
    pub fn get(&self, name: &Token) -> Result<Value, LoxError> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(&name.lexeme) {
                return Ok(v.clone());
            }
        }
        Err(LoxError::runtime(
            name,
            format!("Undefined variable '{}'.", name.lexeme),
        ))
    }

    /// Assign to an existing variable (innermost wins). Does not create a
    /// new binding; assignment to an undeclared name is a runtime error.
    ///
    /// Returns the assigned value so callers can use assignment as an
    /// expression.
    ///
    /// # Errors
    ///
    /// Returns a runtime error if no enclosing scope declares `name`.
    pub fn assign(&mut self, name: &Token, value: Value) -> Result<Value, LoxError> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(&name.lexeme) {
                scope.insert(name.lexeme.clone(), value.clone());
                return Ok(value);
            }
        }
        Err(LoxError::runtime(
            name,
            format!("Undefined variable '{}'.", name.lexeme),
        ))
    }
}
