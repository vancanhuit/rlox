//! Integration tests for the public `rlox_vm::run_to` entry point.
//!
//! `run_to` is also exercised end-to-end through the umbrella binary's
//! subprocess tests in `crates/rlox/tests/cli.rs`, but those run in a
//! separate process and don't contribute line coverage to this crate.
//! The tests here drive the same code path directly so coverage tooling
//! sees it.

use rlox_vm::{LoxError, run_to};

/// Helper: run `src` through the VM, return whatever it wrote to its
/// output sink.
fn run(src: &str) -> Result<String, Vec<LoxError>> {
    let mut buf: Vec<u8> = Vec::new();
    run_to(src, &mut buf)?;
    Ok(String::from_utf8(buf).expect("writer only emits utf-8"))
}

#[test]
fn run_to_writes_print_output() {
    assert_eq!(run("print 1 + 2 * 3;").unwrap(), "7\n");
}

#[test]
fn run_to_handles_grouping_and_unary() {
    assert_eq!(run("print -(1 + 2);").unwrap(), "-3\n");
}

#[test]
fn run_to_renders_whole_numbers_without_decimal() {
    // The numeric formatter follows clox's `printValue`:
    // whole numbers as `42`, fractions natural.
    assert_eq!(run("print 42;").unwrap(), "42\n");
    assert_eq!(run("print 1.5 + 0.25;").unwrap(), "1.75\n");
}

#[test]
fn run_to_propagates_parse_errors() {
    let errs = run("print (1 + 2;").expect_err("expected parse error");
    assert!(
        errs.iter().any(|e| matches!(
            e,
            LoxError::Parse { message, .. } if message == "Expect ')' after expression."
        )),
        "errors were: {errs:?}"
    );
}

#[test]
fn run_to_propagates_scan_errors() {
    let errs = run("print 1 + @;").expect_err("expected scan error");
    assert!(
        errs.iter().any(|e| matches!(e, LoxError::Scan { .. })),
        "errors were: {errs:?}"
    );
}

#[test]
fn run_to_accepts_empty_program() {
    assert_eq!(run("").unwrap(), "");
}

#[test]
fn run_to_propagates_compile_error_for_missing_semicolon() {
    let errs = run("1 2").expect_err("expected parse error");
    assert!(
        errs.iter().any(|e| matches!(
            e,
            LoxError::Parse { message, .. } if message == "Expect ';' after expression."
        )),
        "errors were: {errs:?}"
    );
}

// ---------- Chapter 19 — Strings (end-to-end through run_to) ----------

#[test]
fn run_to_renders_string_literal_verbatim() {
    // Chapter 19 strings render via the heap-aware Display so the
    // user sees the actual content, not a `<obj#N>` placeholder.
    assert_eq!(run("print \"hello\";").unwrap(), "hello\n");
}

#[test]
fn run_to_concatenates_strings() {
    assert_eq!(run("print \"foo\" + \"bar\";").unwrap(), "foobar\n");
}

#[test]
fn run_to_string_equality_renders_bool() {
    assert_eq!(run("print \"abc\" == \"abc\";").unwrap(), "true\n");
}

#[test]
fn run_to_string_plus_number_is_runtime_error() {
    let errs = run("print \"a\" + 1;").expect_err("expected runtime error");
    assert!(
        errs.iter().any(|e| matches!(
            e,
            LoxError::Runtime { message, .. }
                if message == "Operands must be two numbers or two strings."
        )),
        "errors were: {errs:?}"
    );
}

#[test]
fn run_to_handles_global_variables_and_assignment() {
    let src = r#"
        var a = "global";
        print a;
        a = a + " updated";
        print a;
    "#;
    assert_eq!(run(src).unwrap(), "global\nglobal updated\n");
}
