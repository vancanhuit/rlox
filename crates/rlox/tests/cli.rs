//! Smoke tests for the compiled `rlox` umbrella binary.
//!
//! Library-level tests for the tree-walk pipeline live in
//! `crates/rlox-tree/tests/run.rs`; this file only exercises the
//! end-to-end CLI (exit codes, stdout/stderr formatting, `--help` /
//! `--version` flags).

use std::fs;
use std::process::Command;

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

// ---- tree-walk-only smoke tests ----
//
// These exercise full Lox programs (statements, `print`, string
// concatenation runtime errors) that the bytecode VM doesn't yet
// support — chapter 21 (*Globals*) is when statements arrive on
// the VM side. Feature-gated to `tree` only.

#[cfg(feature = "tree")]
#[test]
fn binary_runs_a_script_file_and_prints_result() {
    let path = write_lox("ok", "print (5 - (3 - 1)) + -1;");
    let out = Command::new(rlox_bin()).arg(&path).output().unwrap();
    fs::remove_file(&path).ok();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "2");
}

#[cfg(feature = "tree")]
#[test]
fn binary_exits_65_on_compile_error() {
    let path = write_lox("compile_err", "print (1 + 2;");
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

#[cfg(feature = "tree")]
#[test]
fn binary_exits_70_on_runtime_error() {
    let path = write_lox("runtime_err", r#"print 1 + "x";"#);
    let out = Command::new(rlox_bin()).arg(&path).output().unwrap();
    fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(70));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Operands must be two numbers or two strings."),
        "stderr was: {stderr}"
    );
}

// ---- vm-only smoke tests ----
//
// At chapter 17 the VM build only handles a single Lox expression
// per file/line and writes its numeric value to stdout. Once chapter
// 21 lands these will broaden to match the tree-walk equivalents.

#[cfg(feature = "vm")]
#[test]
fn vm_binary_evaluates_an_expression_file() {
    let path = write_lox("vm_ok", "(5 - (3 - 1)) + -1");
    let out = Command::new(rlox_bin()).arg(&path).output().unwrap();
    fs::remove_file(&path).ok();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "2");
}

#[cfg(feature = "vm")]
#[test]
fn vm_binary_exits_65_on_compile_error() {
    let path = write_lox("vm_compile_err", "(1 + 2");
    let out = Command::new(rlox_bin()).arg(&path).output().unwrap();
    fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(65));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Expect ')' after expression."),
        "stderr was: {stderr}"
    );
}

#[cfg(feature = "vm")]
#[test]
fn vm_binary_evaluates_chapter17_reference_fragment() {
    // The book's signature `1 + 2 * 3 = 7` example.
    let path = write_lox("vm_ref", "1 + 2 * 3");
    let out = Command::new(rlox_bin()).arg(&path).output().unwrap();
    fs::remove_file(&path).ok();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "7");
}
