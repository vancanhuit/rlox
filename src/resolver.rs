//! Static resolution pass (chapter 11). Walks the AST after parsing and
//! before interpretation to:
//!
//! 1. Pre-compute the lexical depth (number of enclosing local scopes) of
//!    every variable read or write — recorded in a [`Locals`] map keyed
//!    by `Expr` address. The interpreter then performs `O(1)` lookup at
//!    that exact depth instead of walking the parent chain.
//! 2. Statically reject several semantic errors the chapter-10 runtime
//!    can't catch cleanly:
//!    - `var a = a;` — referencing a variable in its own initializer.
//!    - `var a; var a;` — re-declaring a name in the same local block.
//!    - `return ...;` outside any function body.
//! 3. Fix the closure-capture bug from chapter 11's motivating example
//!    (a function that reads `a` should *not* see a sibling `var a`
//!    declared after the function but in the same enclosing block).
//!
//! The resolver only tracks **local** scopes. Globals are resolved at
//! interpret time via the existing parent-pointer chain.

use std::collections::HashMap;

use crate::ast::{Expr, Stmt};
use crate::error::LoxError;
use crate::token::Token;

/// Lexical-depth map keyed by `Expr` address (cast to `usize` so the
/// internal pointer never escapes a safe interface). The same `&[Stmt]`
/// must be passed to both the resolver and the interpreter for the
/// addresses to be valid.
pub type Locals = HashMap<usize, usize>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FunctionKind {
    None,
    Function,
}

/// Compute the address-keyed depth used by the interpreter. The cast
/// ladder `&Expr -> *const Expr -> usize` is what `clippy` prefers over
/// `as usize` directly, and it's what makes the pointer identity
/// readable as a stable map key.
fn expr_key(e: &Expr) -> usize {
    std::ptr::from_ref::<Expr>(e) as usize
}

/// Run the resolver over a program. Returns the [`Locals`] map on
/// success, or every static error encountered.
///
/// # Errors
///
/// Returns the accumulated parse-style errors (using [`LoxError::Parse`])
/// when at least one resolver check failed.
pub fn resolve(stmts: &[Stmt]) -> std::result::Result<Locals, Vec<LoxError>> {
    let mut r = Resolver::new();
    r.resolve_stmts(stmts);
    if r.errors.is_empty() {
        Ok(r.locals)
    } else {
        Err(r.errors)
    }
}

struct Resolver {
    /// Stack of local scopes. Each entry maps a name to whether it is
    /// fully *defined* (`true`) or merely *declared* — i.e. its
    /// initializer is currently being resolved (`false`). The latter is
    /// what lets us catch `var a = a;`.
    scopes: Vec<HashMap<String, bool>>,
    locals: Locals,
    current_function: FunctionKind,
    errors: Vec<LoxError>,
}

impl Resolver {
    fn new() -> Self {
        Self {
            scopes: Vec::new(),
            locals: HashMap::new(),
            current_function: FunctionKind::None,
            errors: Vec::new(),
        }
    }

    fn resolve_stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.resolve_stmt(s);
        }
    }

    fn resolve_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expression(e) | Stmt::Print(e) => self.resolve_expr(e),
            Stmt::Var { name, initializer } => {
                self.declare(name);
                if let Some(init) = initializer {
                    self.resolve_expr(init);
                }
                self.define(name);
            }
            Stmt::Block(stmts) => {
                self.begin_scope();
                self.resolve_stmts(stmts);
                self.end_scope();
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.resolve_expr(condition);
                self.resolve_stmt(then_branch);
                if let Some(alt) = else_branch {
                    self.resolve_stmt(alt);
                }
            }
            Stmt::While { condition, body } => {
                self.resolve_expr(condition);
                self.resolve_stmt(body);
            }
            Stmt::Function(decl) => {
                // The function's name binds in the surrounding scope
                // *before* its body is resolved, so recursion can refer
                // to it by name.
                self.declare(&decl.name);
                self.define(&decl.name);
                self.resolve_function(&decl.params, &decl.body, FunctionKind::Function);
            }
            Stmt::Return { keyword, value } => {
                if self.current_function == FunctionKind::None {
                    self.errors.push(LoxError::parse(
                        keyword,
                        "Can't return from top-level code.",
                    ));
                }
                if let Some(v) = value {
                    self.resolve_expr(v);
                }
            }
        }
    }

    fn resolve_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal(_) => {}
            Expr::Grouping(inner) => self.resolve_expr(inner),
            Expr::Unary { right, .. } => self.resolve_expr(right),
            Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
                self.resolve_expr(left);
                self.resolve_expr(right);
            }
            Expr::Call {
                callee, arguments, ..
            } => {
                self.resolve_expr(callee);
                for a in arguments {
                    self.resolve_expr(a);
                }
            }
            Expr::Variable(name) => {
                if let Some(scope) = self.scopes.last()
                    && scope.get(&name.lexeme) == Some(&false)
                {
                    self.errors.push(LoxError::parse(
                        name,
                        "Can't read local variable in its own initializer.",
                    ));
                }
                self.resolve_local(expr, name);
            }
            Expr::Assign { name, value } => {
                self.resolve_expr(value);
                self.resolve_local(expr, name);
            }
        }
    }

    fn resolve_local(&mut self, expr: &Expr, name: &Token) {
        // Walk inner-to-outer; the depth recorded is the number of
        // scopes we crossed to find the binding (0 = innermost).
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            if scope.contains_key(&name.lexeme) {
                let depth = self.scopes.len() - 1 - i;
                self.locals.insert(expr_key(expr), depth);
                return;
            }
        }
        // Not found in any local scope — assume global; the interpreter
        // will fall back to its globals environment.
    }

    fn resolve_function(&mut self, params: &[Token], body: &[Stmt], kind: FunctionKind) {
        let enclosing = std::mem::replace(&mut self.current_function, kind);
        self.begin_scope();
        for p in params {
            self.declare(p);
            self.define(p);
        }
        self.resolve_stmts(body);
        self.end_scope();
        self.current_function = enclosing;
    }

    fn begin_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn end_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &Token) {
        let Some(scope) = self.scopes.last_mut() else {
            return;
        };
        if scope.contains_key(&name.lexeme) {
            self.errors.push(LoxError::parse(
                name,
                "Already a variable with this name in this scope.",
            ));
        }
        scope.insert(name.lexeme.clone(), false);
    }

    fn define(&mut self, name: &Token) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.lexeme.clone(), true);
        }
    }
}

/// Map an `Expr` reference to its resolver key. Used by the interpreter
/// to look up the depth recorded for a variable reference.
#[must_use]
pub fn lookup_key(expr: &Expr) -> usize {
    expr_key(expr)
}
