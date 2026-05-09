//! End-to-end tests for the public `rlox_tree::run` pipeline (scan +
//! parse + resolve + interpret), capturing `print` output.
//!
//! Chapter 8 turned `run` into a program runner whose return value is
//! the captured stdout (one line per executed `print`), so the original
//! "value of an expression" assertions are expressed as `print expr;`
//! programs returning `"<value>\n"`.

use rlox_tree::{LoxError, run};

#[test]
fn run_returns_captured_print_output() {
    assert_eq!(run("print 1 + 2;").unwrap(), "3\n");
    // Chap07 reference case from `test/expressions/evaluate.lox`.
    assert_eq!(run("print (5 - (3 - 1)) + -1;").unwrap(), "2\n");
    assert_eq!(run(r#"print "foo" + "bar";"#).unwrap(), "foobar\n");
    assert_eq!(run("print nil;").unwrap(), "nil\n");
    assert_eq!(run("print !false;").unwrap(), "true\n");
}

#[test]
fn run_executes_multiple_statements_in_order() {
    let out = run("print 1; print 2; print 3;").unwrap();
    assert_eq!(out, "1\n2\n3\n");
}

#[test]
fn run_returns_empty_string_for_program_with_no_prints() {
    // An expression statement runs for side effects only; no output.
    assert_eq!(run("1 + 2;").unwrap(), "");
}

#[test]
fn run_surfaces_scan_errors() {
    let errs = run("@1;").unwrap_err();
    assert!(errs.iter().any(|e| matches!(e, LoxError::Scan { .. })));
}

#[test]
fn run_surfaces_parse_errors() {
    let errs = run("print (1 + 2;").unwrap_err();
    assert!(
        errs.iter().any(|e| matches!(e, LoxError::Parse { .. })),
        "expected at least one Parse error, got {errs:?}"
    );
}

#[test]
fn run_collects_multiple_parse_errors_via_synchronize() {
    // Two malformed statements separated by `;` so `synchronize` can resume
    // after the first failure and find the second.
    let errs = run("var ;\nvar ;").unwrap_err();
    let parse_errs = errs
        .iter()
        .filter(|e| matches!(e, LoxError::Parse { .. }))
        .count();
    assert!(
        parse_errs >= 2,
        "expected >=2 parse errors, got {parse_errs}: {errs:?}"
    );
}

#[test]
fn run_surfaces_runtime_errors() {
    let errs = run(r#"print 1 + "x";"#).unwrap_err();
    assert_eq!(errs.len(), 1);
    let LoxError::Runtime { message, .. } = &errs[0] else {
        panic!("expected Runtime, got {:?}", errs[0]);
    };
    assert_eq!(message, "Operands must be two numbers or two strings.");
}

#[test]
fn run_threads_state_through_var_and_assignment_in_one_program() {
    let out = run("var a = 1; a = a + 2; print a;").unwrap();
    assert_eq!(out, "3\n");
}

#[test]
fn run_block_scopes_shadow_outer_bindings() {
    // The book's chapter 8 reference fragment for nested scopes.
    let src = "\
var a = \"global\";
{
  var a = \"block\";
  print a;
}
print a;
";
    assert_eq!(run(src).unwrap(), "block\nglobal\n");
}
