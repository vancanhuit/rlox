# rlox

A Rust port of the tree-walk Lox interpreter from
[*Crafting Interpreters*](https://craftinginterpreters.com/a-tree-walk-interpreter.html)
— written primarily as a vehicle for learning idiomatic Rust (edition 2024,
let-chains, `LazyLock`, hand-rolled error types, etc.).

## Status

**Milestone 1 in progress** — chapters 4–7 of the book (scanner, hand-written
recursive-descent parser, expression evaluator). Statements, variables, control
flow, functions, resolver and classes (chapters 8–13) are deferred to later
milestones.

| Phase | Module(s)                    | Status |
| ----- | ---------------------------- | ------ |
| 0     | Cargo + GitHub scaffolding   | in PR  |
| 1     | `token`, `error`             | todo   |
| 2     | `scanner`                    | todo   |
| 3     | `ast` (+ pretty-printer)     | todo   |
| 4     | `parser`                     | todo   |
| 5     | `interpreter`                | todo   |
| 6     | `lib.rs` / `main.rs` (CLI)   | todo   |
| 7     | README polish, CONTRIBUTING  | todo   |

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

The binary is wired up in Phase 6. Once it lands:

```sh
cargo run --quiet            # REPL
cargo run --quiet -- file.lox # run a script
```

## Testing approach

- **Test-Driven Development** every phase — Red → Green → Refactor.
- **Unit tests** live next to the code under `#[cfg(test)] mod tests`.
- **Integration tests** under [`tests/`](tests/) drive the public library API.
- Reference cases (inputs + expected outputs) are transcribed from
  [munificent/craftinginterpreters/test](https://github.com/munificent/craftinginterpreters/tree/master/test);
  we re-encode them as native Rust `#[test]` functions rather than running the
  upstream `.lox` files directly.

## Repository conventions

- **One branch + PR per phase.** Branch naming `phase/<n>-<slug>` (e.g.
  `phase/2-scanner`).
- **Conventional Commits** are enforced on PR titles only (squash-merge means
  the title becomes the single commit on `main`). See
  [`.github/workflows/pr-title.yaml`](.github/workflows/pr-title.yaml).
- **Branch protection** for `main` is managed via a GitHub repository ruleset
  committed at [`.github/rulesets/main.json`](.github/rulesets/main.json):
  squash-merge only, linear history, all CI checks required, no force pushes
  or deletions, conversation resolution required.
- **CI** workflows under [`.github/workflows/`](.github/workflows/):
  - `ci.yaml` — `cargo fmt --check`, `cargo clippy -- -D warnings`,
    `cargo test --all-targets --locked`.
  - `coverage-and-audit.yaml` — `cargo-llvm-cov` (uploaded to Codecov) and
    `cargo audit --deny warnings`.
  - `pr-title.yaml` — Conventional Commits validation.

## License

[MIT](LICENSE) — to be added.

## Acknowledgements

- Robert Nystrom for the wonderful book and its
  [test corpus](https://github.com/munificent/craftinginterpreters/tree/master/test).
