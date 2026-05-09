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
fn run_to_writes_value_with_trailing_newline() {
    // Chapter 17 emits `<value>\n` so a line-oriented REPL reader sees
    // exactly one result per line.
    assert_eq!(run("1 + 2 * 3").unwrap(), "7\n");
}

#[test]
fn run_to_handles_grouping_and_unary() {
    assert_eq!(run("-(1 + 2)").unwrap(), "-3\n");
}

#[test]
fn run_to_renders_whole_numbers_without_decimal() {
    // The chapter-17 numeric formatter follows clox's `printValue`:
    // whole numbers as `42`, fractions natural.
    assert_eq!(run("42").unwrap(), "42\n");
    assert_eq!(run("1.5 + 0.25").unwrap(), "1.75\n");
}

#[test]
fn run_to_propagates_parse_errors() {
    let errs = run("(1 + 2").expect_err("expected parse error");
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
    let errs = run("1 + @").expect_err("expected scan error");
    assert!(
        errs.iter().any(|e| matches!(e, LoxError::Scan { .. })),
        "errors were: {errs:?}"
    );
}

#[test]
fn run_to_propagates_compile_error_for_empty_input() {
    let errs = run("").expect_err("expected parse error");
    assert!(
        errs.iter().any(|e| matches!(
            e,
            LoxError::Parse { message, .. } if message == "Expect expression."
        )),
        "errors were: {errs:?}"
    );
}

#[test]
fn run_to_propagates_compile_error_for_trailing_garbage() {
    let errs = run("1 2").expect_err("expected parse error");
    assert!(
        errs.iter().any(|e| matches!(
            e,
            LoxError::Parse { message, .. } if message == "Expect end of expression."
        )),
        "errors were: {errs:?}"
    );
}
