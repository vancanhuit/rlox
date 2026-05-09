# Contributing to rlox

Thanks for your interest. This is a learning project, but the conventions below
are enforced by CI so contributions stay consistent.

## Workflow

1. **Branch from `main`.** One branch and one PR per logical change. For
   milestone work the convention is `phase/<n>-<slug>` (e.g. `phase/4-parser`);
   for incidental fixes use a Conventional-Commits-style prefix
   (`fix/<slug>`, `docs/<slug>`, `ci/<slug>`).
2. **Test-Driven Development.** Every phase / fix follows
   **Red → Green → Refactor**:
   - Write the failing test first (unit in `#[cfg(test)] mod tests` or
     integration in [`tests/`](tests/)).
   - Implement the minimum production code to pass.
   - Refactor and re-run the verification commands below.
3. **Verify before pushing**:

   ```sh
   cargo fmt --all -- --check
   cargo clippy --all-targets --locked -- -D warnings
   cargo test --all-targets --locked
   ```

4. **Open a PR against `main`.** Title must follow
   [Conventional Commits](https://www.conventionalcommits.org/), e.g.
   `feat(parser): add Pratt expression parser`. The
   `Conventional Commits` workflow validates this on every push to the PR.
5. **Wait for CI.** Four checks must pass: `Lint and test`, `Coverage`,
   `Audit`, `Conventional Commits`.
6. **Squash-merge.** The repository ruleset only permits squash merges, so
   the PR title becomes the single commit on `main`. Local commit messages
   during work-in-progress are not linted — only the squashed result matters.
7. **Branch is auto-deleted** on merge.

## Conventional Commit types accepted

`feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `chore`, `ci`, `build`,
`revert`. Scopes are optional but encouraged (`feat(scanner): ...`).

## Repository structure

| Path                                              | Purpose                                  |
| ------------------------------------------------- | ---------------------------------------- |
| [`src/`](src/)                                    | Library + binary source                  |
| [`tests/`](tests/)                                | Integration tests against the public API |
| [`.github/workflows/`](.github/workflows/)        | CI pipelines (see badge in README)       |
| [`.github/rulesets/main.json`](.github/rulesets/main.json) | Branch-protection ruleset (committed for review) |
| [`codecov.yml`](codecov.yml)                      | Codecov thresholds and ignores           |
| [`rust-toolchain.toml`](rust-toolchain.toml)      | Rust 1.95 + rustfmt + clippy + llvm-tools-preview |

## Coding standards

- Edition 2024, Rust **1.95** pinned via `rust-toolchain.toml`.
- `unsafe_code = "forbid"`; `clippy::pedantic = "warn"` (a few common-sense
  allows in `Cargo.toml`).
- Idiomatic Rust over textbook Java translations: prefer enums and `match`
  over Visitor patterns; use `LazyLock` instead of `once_cell`; use
  `let ... else` and let-chains where natural.
- Integration tests should consume the public library surface only
  (`use rlox::{...};`).

## Reference test corpus

Reference cases for the scanner / parser / interpreter come from the upstream
[munificent/craftinginterpreters/test](https://github.com/munificent/craftinginterpreters/tree/master/test)
suite. We **re-encode** these as native Rust `#[test]` functions instead of
running the `.lox` files directly — keeps `cargo test` self-contained, no
submodule, no `regex` crate.

## Project goals

This is primarily a *learning* exercise. PRs that bring the implementation
closer to the book's behaviour (or to the upstream test expectations) are
very welcome; PRs that add gratuitous deviations from the book without a
strong justification are not.
