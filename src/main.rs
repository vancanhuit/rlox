//! `rlox` — command-line entry point.
//!
//! Usage:
//!
//! ```text
//! rlox [script]
//! ```
//!
//! With no arguments, drops into a line-oriented REPL accepting one or
//! more `;`-terminated Lox statements per line. With one argument,
//! reads the file as a Lox program, executes it, and exits.
//!
//! Exit codes (matching jlox / `EX_DATAERR` and `EX_SOFTWARE`):
//!
//! - `0` — success
//! - `2` — clap usage error (unknown flag, too many args)
//! - `64` — runtime usage error (file unreadable)
//! - `65` — compile error (scan / parse)
//! - `70` — runtime error

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use rlox::{LoxError, run_to};

const EX_USAGE: u8 = 64;
const EX_DATAERR: u8 = 65;
const EX_SOFTWARE: u8 = 70;

/// A Rust port of the tree-walk Lox interpreter from
/// <https://craftinginterpreters.com>.
#[derive(Parser, Debug)]
#[command(name = "rlox", version, about, long_about = None)]
struct Cli {
    /// Path to a Lox script. If omitted, drops into the REPL.
    script: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.script.as_deref() {
        None => run_prompt(),
        Some(path) => run_file(path),
    }
}

fn run_file(path: &Path) -> ExitCode {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("rlox: cannot read {}: {err}", path.display());
            return ExitCode::from(EX_USAGE);
        }
    };
    let mut stdout = io::stdout().lock();
    match run_to(&source, &mut stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(errors) => {
            // Drop the lock before printing diagnostics so they don't
            // interleave with any partial program output already flushed.
            drop(stdout);
            report_errors(&errors);
            ExitCode::from(exit_code_for(&errors))
        }
    }
}

fn run_prompt() -> ExitCode {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut line = String::new();
    loop {
        write!(stdout, "> ").ok();
        stdout.flush().ok();

        line.clear();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                // EOF — print a final newline for tidy terminals and exit.
                writeln!(stdout).ok();
                return ExitCode::SUCCESS;
            }
            Ok(_) => {}
            Err(err) => {
                eprintln!("rlox: read error: {err}");
                return ExitCode::from(EX_SOFTWARE);
            }
        }

        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        if trimmed.is_empty() {
            continue;
        }

        // Each REPL line is parsed as a fresh program. Variable bindings
        // do NOT persist across lines yet; chapter 8's REPL keeps the same
        // semantics as the script runner. A persistent REPL environment
        // arrives once we reorganise around `Rc<RefCell<Environment>>` in
        // chapter 10 (closures need it anyway).
        match run_to(trimmed, &mut stdout) {
            Ok(()) => {}
            Err(errors) => report_errors(&errors),
        }
    }
}

fn report_errors(errors: &[LoxError]) {
    for err in errors {
        eprintln!("{err}");
    }
}

/// Pick the most severe exit code for a batch: runtime > compile.
fn exit_code_for(errors: &[LoxError]) -> u8 {
    if errors.iter().any(|e| matches!(e, LoxError::Runtime { .. })) {
        EX_SOFTWARE
    } else {
        EX_DATAERR
    }
}
