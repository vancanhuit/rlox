//! Phase 11 — public-API tests for the static resolution pass.
//!
//! Two flavours of test live here:
//!
//! 1. *Static-error* tests that drive `resolve(stmts)` directly and
//!    assert the error message and the location token.
//! 2. *End-to-end* tests via [`rlox::run`] that exercise resolver-driven
//!    behaviour visible to programs — most importantly the chapter-11
//!    closure-capture fix.

use rlox::{LoxError, parse_program, resolve, run, scan};

/// Scan + parse a source string into a `Vec<Stmt>` for resolver-only tests.
fn parse(src: &str) -> Vec<rlox::Stmt> {
    let (tokens, errors) = scan(src);
    assert!(errors.is_empty(), "scan errors: {errors:?}");
    parse_program(&tokens).expect("parse should succeed")
}

// ---- static errors caught only by the resolver ----

#[test]
fn rejects_self_reference_in_initializer() {
    // `var a = a;` reads `a` while `a` is declared-but-not-defined.
    let stmts = parse("{ var a = a; }");
    let errs = resolve(&stmts).unwrap_err();
    assert_eq!(errs.len(), 1);
    let LoxError::Parse { message, .. } = &errs[0] else {
        panic!("expected Parse error, got {:?}", errs[0]);
    };
    assert_eq!(message, "Can't read local variable in its own initializer.");
}

#[test]
fn rejects_redeclaration_in_same_local_scope() {
    let stmts = parse("{ var a = 1; var a = 2; }");
    let errs = resolve(&stmts).unwrap_err();
    let LoxError::Parse { message, .. } = &errs[0] else {
        panic!("expected Parse error, got {:?}", errs[0]);
    };
    assert_eq!(message, "Already a variable with this name in this scope.");
}

#[test]
fn allows_redeclaration_at_global_scope() {
    // jlox semantics: globals can be silently redeclared (REPL-friendly).
    // The resolver only tracks local scopes, so `var a; var a;` at the
    // top level passes resolution.
    let stmts = parse("var a = 1; var a = 2;");
    let locals = resolve(&stmts).expect("global redeclaration is allowed");
    // No locals should have been recorded — every reference is global.
    assert!(locals.is_empty());
}

#[test]
fn rejects_return_outside_function() {
    let stmts = parse("return 1;");
    let errs = resolve(&stmts).unwrap_err();
    let LoxError::Parse { message, .. } = &errs[0] else {
        panic!("expected Parse error, got {:?}", errs[0]);
    };
    assert_eq!(message, "Can't return from top-level code.");
}

#[test]
fn return_inside_function_is_fine() {
    let stmts = parse("fun f() { return 1; }");
    resolve(&stmts).expect("return inside a function should be accepted");
}

#[test]
fn nested_function_returns_are_fine() {
    let stmts = parse("fun outer() { fun inner() { return 1; } }");
    resolve(&stmts).expect("nested function returns should be accepted");
}

#[test]
fn redeclaration_in_function_parameters_is_rejected() {
    // `fun f(a, a) {}` — two parameters with the same name share a
    // single local scope, so the resolver rejects the second one.
    let stmts = parse("fun f(a, a) {}");
    let errs = resolve(&stmts).unwrap_err();
    let LoxError::Parse { message, .. } = &errs[0] else {
        panic!("expected Parse error, got {:?}", errs[0]);
    };
    assert_eq!(message, "Already a variable with this name in this scope.");
}

#[test]
fn collects_multiple_resolver_errors_per_pass() {
    // The resolver doesn't synchronize like the parser, but it does
    // accumulate every error it encounters in a single walk.
    let stmts = parse(
        "\
fun outer() {
  return 1;
  { var a = 1; var a = 2; }
}
return 2;
",
    );
    let errs = resolve(&stmts).unwrap_err();
    // We should see at least the top-level `return` error and the
    // duplicate-`a` error.
    let has_top_level_return = errs.iter().any(|e| {
        matches!(
            e,
            LoxError::Parse { message, .. } if message == "Can't return from top-level code."
        )
    });
    let has_dup_local = errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Already a variable with this name in this scope."
    ));
    assert!(
        has_top_level_return,
        "missing top-level return error: {errs:?}"
    );
    assert!(has_dup_local, "missing duplicate-local error: {errs:?}");
}

// ---- chapter 11 motivating example: closure capture correctness ----

#[test]
fn closure_captures_outer_a_not_block_local_a() {
    // The book's reference fragment:
    //     var a = "global";
    //     {
    //       fun showA() { print a; }
    //       showA();
    //       var a = "block";
    //       showA();
    //     }
    // After the resolver pass, both `showA()` invocations must resolve
    // `a` to the *outer* (global) scope, not the inner block-local `a`.
    let src = "\
var a = \"global\";
{
  fun showA() { print a; }
  showA();
  var a = \"block\";
  showA();
}
";
    assert_eq!(run(src).unwrap(), "global\nglobal\n");
}

#[test]
fn run_surfaces_resolver_errors_from_run_to() {
    // The resolver runs as part of `run` / `run_to`, so static errors
    // surface to library callers without needing a separate driver.
    let errs = run("var x = x;").unwrap_err();
    // Globals can self-reference (no locals scope in play), so this
    // resolves but blows up at runtime as `Undefined variable 'x'.`.
    assert!(
        errs.iter().any(|e| matches!(e, LoxError::Runtime { .. })),
        "expected runtime error for global self-reference, got {errs:?}"
    );

    let errs = run("{ var x = x; }").unwrap_err();
    // Local self-reference is the static case the resolver rejects.
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Can't read local variable in its own initializer."
    )));
}

// ---- chapter 12: classes / `this` / `init` static checks ----

#[test]
fn rejects_this_outside_a_class() {
    let stmts = parse("print this;");
    let errs = resolve(&stmts).unwrap_err();
    let LoxError::Parse { message, .. } = &errs[0] else {
        panic!("expected Parse error");
    };
    assert_eq!(message, "Can't use 'this' outside of a class.");
}

#[test]
fn rejects_this_inside_top_level_function() {
    // `this` only makes sense inside a method, not a free function.
    let stmts = parse("fun f() { return this; }");
    let errs = resolve(&stmts).unwrap_err();
    let LoxError::Parse { message, .. } = &errs[0] else {
        panic!("expected Parse error");
    };
    assert_eq!(message, "Can't use 'this' outside of a class.");
}

#[test]
fn allows_this_inside_method_body() {
    let stmts = parse("class C { m() { return this; } }");
    resolve(&stmts).expect("`this` inside a method should resolve");
}

#[test]
fn rejects_returning_value_from_init() {
    let stmts = parse("class C { init() { return 1; } }");
    let errs = resolve(&stmts).unwrap_err();
    let LoxError::Parse { message, .. } = &errs[0] else {
        panic!("expected Parse error");
    };
    assert_eq!(message, "Can't return a value from an initializer.");
}

#[test]
fn allows_bare_return_in_init() {
    let stmts = parse("class C { init() { return; } }");
    resolve(&stmts).expect("bare `return;` inside init should be accepted");
}

// ---- chapter 13: inheritance / super static checks ----

#[test]
fn rejects_self_inheritance() {
    let stmts = parse("class Foo < Foo {}");
    let errs = resolve(&stmts).unwrap_err();
    let LoxError::Parse { message, .. } = &errs[0] else {
        panic!("expected Parse error");
    };
    assert_eq!(message, "A class can't inherit from itself.");
}

#[test]
fn rejects_super_outside_a_class() {
    let stmts = parse("super.method;");
    let errs = resolve(&stmts).unwrap_err();
    let LoxError::Parse { message, .. } = &errs[0] else {
        panic!("expected Parse error");
    };
    assert_eq!(message, "Can't use 'super' outside of a class.");
}

#[test]
fn rejects_super_in_class_with_no_superclass() {
    let stmts = parse("class C { m() { super.x; } }");
    let errs = resolve(&stmts).unwrap_err();
    let LoxError::Parse { message, .. } = &errs[0] else {
        panic!("expected Parse error");
    };
    assert_eq!(message, "Can't use 'super' in a class with no superclass.");
}

#[test]
fn allows_super_in_subclass_method() {
    let stmts = parse("class A {} class B < A { m() { super.m(); } }");
    resolve(&stmts).expect("`super` inside a subclass method should resolve");
}
