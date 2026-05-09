//! `rlox` — command-line entry point.
//!
//! Usage:
//!
//! ```text
//! rlox [script]
//! ```
//!
//! With no arguments, drops into a line-oriented REPL. With one argument,
//! reads the file as a Lox expression, evaluates it, and exits.
//!
//! Exit codes (matching jlox / `EX_DATAERR` and `EX_SOFTWARE`):
//!
//! - `0` — success
//! - `64` — usage error (too many args)
//! - `65` — compile error (scan / parse)
//! - `70` — runtime error

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::ExitCode;

use rlox::{LoxError, run};

const EX_USAGE: u8 = 64;
const EX_DATAERR: u8 = 65;
const EX_SOFTWARE: u8 = 70;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.len() {
        0 => run_prompt(),
        1 => run_file(Path::new(&args[0])),
        _ => {
            eprintln!("Usage: rlox [script]");
            ExitCode::from(EX_USAGE)
        }
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
    match run(&source) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(errors) => {
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

        match run(trimmed) {
            Ok(output) => {
                writeln!(stdout, "{output}").ok();
            }
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
