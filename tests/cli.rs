//! Phase 6 — public-API tests for the end-to-end `run` pipeline plus a
//! smoke test that exercises the compiled `rlox` binary with a script file.

use std::fs;
use std::process::Command;

use rlox::{LoxError, run};

// ---- library-level: rlox::run ----

#[test]
fn run_returns_stringified_value_for_valid_expression() {
    assert_eq!(run("1 + 2").unwrap(), "3");
    // Chap07 reference case from `test/expressions/evaluate.lox`.
    assert_eq!(run("(5 - (3 - 1)) + -1").unwrap(), "2");
    assert_eq!(run(r#""foo" + "bar""#).unwrap(), "foobar");
    assert_eq!(run("nil").unwrap(), "nil");
    assert_eq!(run("!false").unwrap(), "true");
}

#[test]
fn run_surfaces_scan_errors() {
    let errs = run("@1").unwrap_err();
    assert!(errs.iter().any(|e| matches!(e, LoxError::Scan { .. })));
}

#[test]
fn run_surfaces_parse_errors() {
    let errs = run("(1 + 2").unwrap_err();
    assert_eq!(errs.len(), 1);
    assert!(matches!(errs[0], LoxError::Parse { .. }));
}

#[test]
fn run_surfaces_runtime_errors() {
    let errs = run(r#"1 + "x""#).unwrap_err();
    assert_eq!(errs.len(), 1);
    let LoxError::Runtime { message, .. } = &errs[0] else {
        panic!("expected Runtime, got {:?}", errs[0]);
    };
    assert_eq!(message, "Operands must be two numbers or two strings.");
}

// ---- binary smoke tests ----

/// Path to the compiled `rlox` binary, set by Cargo for integration tests.
fn rlox_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rlox")
}

/// Write `source` to a unique tempfile and return the path.
fn write_lox(name: &str, source: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "rlox_test_{}_{}_{}.lox",
        std::process::id(),
        name,
        // Different scenarios may share a process; add nanos to disambiguate.
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&p, source).unwrap();
    p
}

#[test]
fn binary_runs_a_script_file_and_prints_result() {
    let path = write_lox("ok", "(5 - (3 - 1)) + -1");
    let out = Command::new(rlox_bin()).arg(&path).output().unwrap();
    fs::remove_file(&path).ok();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "2");
}

#[test]
fn binary_exits_65_on_compile_error() {
    let path = write_lox("compile_err", "(1 + 2");
    let out = Command::new(rlox_bin()).arg(&path).output().unwrap();
    fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(65));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Expect ')' after expression."),
        "stderr was: {stderr}"
    );
}

#[test]
fn binary_help_flag_works() {
    let out = Command::new(rlox_bin()).arg("--help").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Usage: rlox"));
    assert!(stdout.contains("--help"));
}

#[test]
fn binary_version_flag_prints_crate_version() {
    let out = Command::new(rlox_bin()).arg("--version").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("rlox "));
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn binary_exits_70_on_runtime_error() {
    let path = write_lox("runtime_err", r#"1 + "x""#);
    let out = Command::new(rlox_bin()).arg(&path).output().unwrap();
    fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(70));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Operands must be two numbers or two strings."),
        "stderr was: {stderr}"
    );
}
