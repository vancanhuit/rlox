//! Phase 8 — public-API tests for the [`Environment`] scope stack.

use rlox::{Environment, LoxError, Token, TokenType, Value};

/// Build a synthetic `Identifier` token at line 1. The interpreter only
/// reads `lexeme` and uses `line` for error reporting, so the rest can be
/// any sentinel value.
fn ident(name: &str) -> Token {
    Token::new(TokenType::Identifier, name.to_string(), None, 1)
}

#[test]
fn define_and_get_round_trip_in_global_scope() {
    let mut env = Environment::new();
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
    let mut env = Environment::new();
    env.define("a", Value::Number(1.0));
    let returned = env.assign(&ident("a"), Value::Number(2.0)).unwrap();
    assert_eq!(returned, Value::Number(2.0));
    assert_eq!(env.get(&ident("a")).unwrap(), Value::Number(2.0));
}

#[test]
fn assign_to_undefined_is_runtime_error() {
    let mut env = Environment::new();
    let err = env.assign(&ident("a"), Value::Nil).unwrap_err();
    let LoxError::Runtime { message, .. } = err else {
        panic!("expected Runtime error");
    };
    assert_eq!(message, "Undefined variable 'a'.");
}

#[test]
fn nested_scope_shadows_outer_binding_until_popped() {
    let mut env = Environment::new();
    env.define("a", Value::String("outer".into()));

    env.push();
    env.define("a", Value::String("inner".into()));
    assert_eq!(env.get(&ident("a")).unwrap(), Value::String("inner".into()));
    env.pop();

    assert_eq!(env.get(&ident("a")).unwrap(), Value::String("outer".into()));
}

#[test]
fn assign_in_inner_scope_walks_outward_to_outer_binding() {
    let mut env = Environment::new();
    env.define("a", Value::Number(1.0));

    env.push();
    // No `a` in this scope, so `assign` walks up and updates the outer one.
    env.assign(&ident("a"), Value::Number(99.0)).unwrap();
    env.pop();

    assert_eq!(env.get(&ident("a")).unwrap(), Value::Number(99.0));
}

#[test]
fn redefining_in_same_scope_overwrites_binding() {
    // jlox semantics: `var a = 1; var a = 2;` in the same scope replaces
    // the binding rather than erroring.
    let mut env = Environment::new();
    env.define("a", Value::Number(1.0));
    env.define("a", Value::Number(2.0));
    assert_eq!(env.get(&ident("a")).unwrap(), Value::Number(2.0));
}
