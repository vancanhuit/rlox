# rlox

[![CI](https://github.com/vancanhuit/rlox/actions/workflows/ci.yaml/badge.svg)](https://github.com/vancanhuit/rlox/actions/workflows/ci.yaml)
[![codecov](https://codecov.io/gh/vancanhuit/rlox/graph/badge.svg)](https://codecov.io/gh/vancanhuit/rlox)

A Rust port of the tree-walk Lox interpreter from
[*Crafting Interpreters*](https://craftinginterpreters.com/a-tree-walk-interpreter.html)
— written primarily as a vehicle for learning idiomatic Rust (edition 2024,
let-chains, `LazyLock`, hand-rolled error types, Pratt parsing, idiomatic
enum-driven AST instead of the book's Visitor pattern).

## Status

**Milestone 1 complete** — chapters 4–7 of the book (scanner, hand-written
parser, expression evaluator) plus a REPL / script-runner CLI. Statements,
variables, control flow, functions, resolver and classes (chapters 8–13) are
deferred to a future milestone.

| Phase | Module(s)                       | Status |
| ----- | ------------------------------- | ------ |
| 0     | Cargo + GitHub scaffolding      | done   |
| 1     | `token`, `error`                | done   |
| 2     | `scanner`                       | done   |
| 3     | `ast`, `value` (+ pretty-print) | done   |
| 4     | `parser` (Pratt)                | done   |
| 5     | `interpreter`                   | done   |
| 6     | `lib.rs` / `main.rs` CLI        | done   |
| 7     | README polish, CONTRIBUTING     | done   |

## Requirements

- Rust **1.95** (pinned via [`rust-toolchain.toml`](rust-toolchain.toml);
  `rustup` will fetch it automatically).

## Build, test, lint

```sh
cargo build
cargo test --all-targets --locked
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
```

## Run

```sh
cargo run --quiet                    # REPL
cargo run --quiet -- path/to/file.lox  # run a script
cargo run --quiet -- --help          # clap-generated help
cargo run --quiet -- --version
```

### REPL example

```text
$ cargo run --quiet
> 1 + 2 * 3
7
> "hello, " + "world"
hello, world
> (5 - (3 - 1)) + -1
2
> ^D
```

### Library quick-start

The full pipeline is exposed at the crate root for embedding:

```rust
use rlox::{run, scan, parse, evaluate, stringify};

// One-shot:
assert_eq!(run("(1 + 2) * 3").unwrap(), "9");

// Or step through manually:
let (tokens, scan_errors) = scan("1 + 2");
assert!(scan_errors.is_empty());
let expr = parse(&tokens).unwrap();
let value = evaluate(&expr).unwrap();
assert_eq!(stringify(&value), "3");
```

### Exit codes

Matching jlox / `sysexits.h`:

| Code | Meaning                                |
| ---- | -------------------------------------- |
| 0    | success                                |
| 2    | clap usage error (unknown flag)        |
| 64   | runtime usage error (file unreadable)  |
| 65   | compile error (scan / parse)           |
| 70   | runtime error                          |

## Testing approach

- **Test-Driven Development** every phase — Red → Green → Refactor.
- **Unit tests** live next to the code under `#[cfg(test)] mod tests`.
- **Integration tests** under [`tests/`](tests/) drive the public library API.
- Reference cases (inputs + expected outputs) are transcribed from
  [munificent/craftinginterpreters/test](https://github.com/munificent/craftinginterpreters/tree/master/test);
  we re-encode them as native Rust `#[test]` functions rather than running the
  upstream `.lox` files directly.

## Repository conventions

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full workflow. In short:

- **One branch + PR per phase.** Branch naming `phase/<n>-<slug>` (e.g. `phase/2-scanner`).
- **Conventional Commits** are enforced on PR titles only (squash-merge means the title becomes the single commit on `main`). See [`.github/workflows/pr-title.yaml`](.github/workflows/pr-title.yaml).
- **Branch protection** for `main` is managed via a GitHub repository ruleset committed at [`.github/rulesets/main.json`](.github/rulesets/main.json): squash-merge only, linear history, all CI checks required, no force pushes or deletions, conversation resolution required.
- **CI** workflows under [`.github/workflows/`](.github/workflows/):
  - `ci.yaml` — `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --all-targets --locked`.
  - `coverage-and-audit.yaml` — `cargo-llvm-cov` (uploaded to Codecov) and `cargo audit --deny warnings`.
  - `pr-title.yaml` — Conventional Commits validation.

## License

[MIT](LICENSE).

## Acknowledgements

- Robert Nystrom for the wonderful book and its [test corpus](https://github.com/munificent/craftinginterpreters/tree/master/test).
