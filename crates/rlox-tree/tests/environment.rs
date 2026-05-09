//! Phase 8/10 — public-API tests for the [`Environment`] scope chain.
//!
//! Phase 10 reworked `Environment` from a `Vec<HashMap>` stack to a
//! parent-pointer chain so closures can outlive their declaring scope.
//! These tests exercise the new API: `child()` instead of `push`/`pop`,
//! and `&self` (interior-mutable) define/assign.

use rlox_tree::{Environment, LoxError, Token, TokenType, Value};

/// Build a synthetic `Identifier` token at line 1. The interpreter only
/// reads `lexeme` and uses `line` for error reporting, so the rest can be
/// any sentinel value.
fn ident(name: &str) -> Token {
    Token::new(TokenType::Identifier, name.to_string(), None, 1)
}

#[test]
fn define_and_get_round_trip_in_global_scope() {
    let env = Environment::new();
    env.define("a", Value::Number(1.0));
    assert_eq!(env.get(&ident("a")).unwrap(), Value::Number(1.0));
}

#[test]
fn get_undefined_variable_is_runtime_error() {
    let env = Environment::new();
    let err = env.get(&ident("missing")).unwrap_err();
    let LoxError::Runtime { message, .. } = err else {
        panic!("expected Runtime error");
    };
    assert_eq!(message, "Undefined variable 'missing'.");
}

#[test]
fn assign_updates_existing_binding_and_returns_value() {
    let env = Environment::new();
    env.define("a", Value::Number(1.0));
    let returned = env.assign(&ident("a"), Value::Number(2.0)).unwrap();
    assert_eq!(returned, Value::Number(2.0));
    assert_eq!(env.get(&ident("a")).unwrap(), Value::Number(2.0));
}

#[test]
fn assign_to_undefined_is_runtime_error() {
    let env = Environment::new();
    let err = env.assign(&ident("a"), Value::Nil).unwrap_err();
    let LoxError::Runtime { message, .. } = err else {
        panic!("expected Runtime error");
    };
    assert_eq!(message, "Undefined variable 'a'.");
}

#[test]
fn child_scope_shadows_outer_binding_independently() {
    let outer = Environment::new();
    outer.define("a", Value::String("outer".into()));

    let inner = outer.child();
    inner.define("a", Value::String("inner".into()));
    assert_eq!(
        inner.get(&ident("a")).unwrap(),
        Value::String("inner".into())
    );

    // The outer scope is unaffected by the inner shadow.
    assert_eq!(
        outer.get(&ident("a")).unwrap(),
        Value::String("outer".into())
    );
}

#[test]
fn assign_in_inner_scope_walks_outward_to_outer_binding() {
    let outer = Environment::new();
    outer.define("a", Value::Number(1.0));

    let inner = outer.child();
    // No `a` in the inner scope, so `assign` walks up and updates the outer.
    inner.assign(&ident("a"), Value::Number(99.0)).unwrap();

    assert_eq!(outer.get(&ident("a")).unwrap(), Value::Number(99.0));
}

#[test]
fn child_environment_outlives_its_parent_handle() {
    // Closure semantics: a function captures its declaring environment
    // and may be called long after the surrounding scope has otherwise
    // gone out of scope. `Rc<RefCell<Scope>>` keeps it alive as long as
    // any handle (including a child's parent pointer) holds a reference.
    let inner = {
        let outer = Environment::new();
        outer.define("captured", Value::Number(7.0));
        outer.child()
    };
    // `outer` has been dropped, but `inner.parent` still owns its `Rc`.
    assert_eq!(inner.get(&ident("captured")).unwrap(), Value::Number(7.0));
}

#[test]
fn redefining_in_same_scope_overwrites_binding() {
    // jlox semantics: `var a = 1; var a = 2;` in the same scope replaces
    // the binding rather than erroring.
    let env = Environment::new();
    env.define("a", Value::Number(1.0));
    env.define("a", Value::Number(2.0));
    assert_eq!(env.get(&ident("a")).unwrap(), Value::Number(2.0));
}

#[test]
fn cloned_environment_handle_shares_storage() {
    // The `Rc` makes `Environment::clone` a refcount bump rather than a
    // deep copy, so writes through one handle are visible through another.
    let a = Environment::new();
    let b = a.clone();
    a.define("x", Value::Number(1.0));
    assert_eq!(b.get(&ident("x")).unwrap(), Value::Number(1.0));
}
