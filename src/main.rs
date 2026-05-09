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
use rlox::{Interpreter, LoxError, parse_program, resolve, run_to, scan};

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
    // Persistent REPL state (chapter 10): a single `Interpreter` lives
    // for the whole session so variable bindings, function declarations,
    // and the global `clock()` survive across prompts. The interpreter
    // owns the stdout writer; the prompt itself goes to stderr to avoid
    // borrow conflicts and to keep program output cleanly separable from
    // UI chrome on redirected pipelines.
    let mut interp = Interpreter::new(&mut stdout);
    let mut line = String::new();
    let stderr = io::stderr();
    loop {
        {
            let mut prompt = stderr.lock();
            let _ = write!(prompt, "> ");
            let _ = prompt.flush();
        }

        line.clear();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                // EOF — print a final newline on stderr for tidy terminals.
                let _ = writeln!(stderr.lock());
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

        match repl_step(trimmed, &mut interp) {
            Ok(()) => {}
            Err(errors) => report_errors(&errors),
        }
    }
}

/// Scan + parse + resolve + execute a single REPL line through the
/// long-lived interpreter, returning every error it produced. The
/// resolver runs per line; depths for previously-defined functions stay
/// alive via `Rc<FunctionDecl>` inside the interpreter's stored
/// `LoxFunction` values.
fn repl_step(source: &str, interp: &mut Interpreter<'_>) -> Result<(), Vec<LoxError>> {
    let (tokens, scan_errors) = scan(source);
    if !scan_errors.is_empty() {
        return Err(scan_errors);
    }
    let stmts = parse_program(&tokens)?;
    let locals = resolve(&stmts)?;
    interp.merge_locals(locals);
    interp.interpret(&stmts).map_err(|e| vec![e])
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
