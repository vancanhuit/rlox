# rlox

[![CI](https://github.com/vancanhuit/rlox/actions/workflows/ci.yaml/badge.svg)](https://github.com/vancanhuit/rlox/actions/workflows/ci.yaml)
[![codecov](https://codecov.io/gh/vancanhuit/rlox/graph/badge.svg)](https://codecov.io/gh/vancanhuit/rlox)

A Rust port of [*Crafting Interpreters*](https://craftinginterpreters.com/) —
written primarily as a vehicle for learning idiomatic Rust (edition 2024,
let-chains, `LazyLock`, hand-rolled error types, Pratt parsing, idiomatic
enum-driven AST instead of the book's Visitor pattern).

## Status

**Milestones 1 + 2 complete; milestone 3 in flight.**

The repo is now a Cargo workspace with three crates and one umbrella binary
that picks its backend at compile time:

| Crate | Role |
| --- | --- |
| [`crates/rlox-tree`](crates/rlox-tree/) | Tree-walk interpreter (jlox port, chapters 4–13). Released as `v0.2.0`. |
| [`crates/rlox-vm`](crates/rlox-vm/) | Bytecode VM (clox port, chapters 14–30). **In progress.** |
| [`crates/rlox`](crates/rlox/) | Umbrella binary. Two mutually-exclusive Cargo features (`tree` default, `vm`) select which backend the binary links to. |

```sh
cargo build                                          # tree-walk binary (default)
cargo build --no-default-features --features vm      # bytecode VM binary
cargo install --path crates/rlox \
    --no-default-features --features vm              # install the VM
```

The `vm` feature wires up to a working CLI in PR 4 (chapter 17, *Compiling
Expressions*); earlier VM PRs deliver lower-level pieces (chunks, disassembler,
stack VM with hand-written bytecode) exercised via `cargo test -p rlox-vm`.

### Milestone 1 — chapters 4–7 (scanner, parser, expressions)

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

### Milestone 2 — chapters 8–13 (statements, classes)

| Phase | Chapter | Topic                                              | Status |
| ----- | ------- | -------------------------------------------------- | ------ |
| 8     | 8       | Statements, `var`, `print`, blocks, assignment     | done   |
| 9     | 9       | `if`/`else`, `while`, `for`, short-circuit `and`/`or` | done   |
| 10    | 10      | Functions, closures, `return`, native `clock()`    | done   |
| 11    | 11      | Resolver (static lexical-depth + diagnostics)      | done   |
| 12    | 12      | Classes, methods, properties, `this`, `init`       | done   |
| 13    | 13      | Inheritance, `super`                               | done   |

### Milestone 3 — chapters 14–30 (bytecode VM)

| Phase | Chapter | Topic                                                | Status |
| ----- | ------- | ---------------------------------------------------- | ------ |
| 14    | 14      | Chunks of Bytecode (`OpCode`, `Chunk`, RLE lines, disassembler) | in progress |
| 15    | 15      | A Virtual Machine (stack, arithmetic dispatch)       | pending |
| 16    | 16      | Scanning on Demand                                   | pending |
| 17    | 17      | Compiling Expressions (single-pass Pratt → bytecode) | pending |
| 18    | 18      | Types of Values (`Nil`/`Bool`, comparisons)          | pending |
| 19    | 19      | Strings (interned via `Heap`)                        | pending |
| 20    | 20      | Hash tables — *absorbed by `std::HashMap`*           | n/a    |
| 21    | 21      | Global Variables                                     | pending |
| 22    | 22      | Local Variables                                      | pending |
| 23    | 23      | Control flow (`if`/`while`/`for`/`and`/`or`)         | pending |
| 24    | 24      | Calls and Functions                                  | pending |
| 25    | 25      | Closures (upvalues)                                  | pending |
| 26    | 26      | Garbage Collection (safe handle-based mark-sweep)    | pending |
| 27    | 27      | Classes and Instances                                | pending |
| 28    | 28      | Methods and Initializers                             | pending |
| 29    | 29      | Superclasses                                         | pending |
| 30    | 30      | Optimization (NaN-boxing skipped — `unsafe_code = "forbid"`) | pending |

## Requirements

- Rust **1.95** (pinned via [`rust-toolchain.toml`](rust-toolchain.toml);
  `rustup` will fetch it automatically).

## Build, test, lint

```sh
cargo build                                           # umbrella binary, default features (tree-walk)
cargo test --workspace --all-targets --locked         # every crate
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Per-crate iteration (faster builds while working on one backend):

```sh
cargo test -p rlox-tree         # tree-walk only
cargo test -p rlox-vm           # bytecode VM only
```

## Run

```sh
cargo run --quiet                    # REPL
cargo run --quiet -- path/to/file.lox  # run a script
cargo run --quiet -- --help          # clap-generated help
cargo run --quiet -- --version
```

### REPL example

The REPL parses every line as a Lox program and persists state across
prompts (variable bindings, function declarations, and the global
`clock()` survive between lines):

```text
$ cargo run --quiet
> var x = 1 + 2 * 3;
> print x;
7
> fun greet(name) { print "hi, " + name; }
> greet("world");
hi, world
> ^D
```

### Library quick-start

The full pipeline is exposed at the crate root for embedding:

```rust
use rlox::{run, scan, parse_program, resolve, Interpreter};

// One-shot — `run` returns captured `print` output.
assert_eq!(
    run("var x = (1 + 2) * 3; print x;").unwrap(),
    "9\n",
);

// Or drive the four stages (scan → parse → resolve → interpret) manually:
let (tokens, scan_errors) = scan("print 1 + 2;");
assert!(scan_errors.is_empty());
let stmts  = parse_program(&tokens).unwrap();
let locals = resolve(&stmts).unwrap();
let mut buf = Vec::<u8>::new();
let mut interp = Interpreter::new(&mut buf);
interp.merge_locals(locals);
interp.interpret(&stmts).unwrap();
assert_eq!(String::from_utf8(buf).unwrap(), "3\n");
```

## Lox showcase

A few snippets you can paste into the REPL or save to a `.lox` file and run
with `cargo run --quiet -- script.lox`. They progress from the book's
chapter-8 basics to chapter-13 inheritance.

### Variables and control flow (chapters 8–9)

```lox
var n = 10;
var sum = 0;
for (var i = 1; i <= n; i = i + 1) {
  sum = sum + i;
}
print sum;          // 55

// Block scope shadows the outer binding without leaking back out.
var greeting = "outer";
{
  var greeting = "inner";
  print greeting;   // inner
}
print greeting;     // outer
```

### Short-circuit logical operators (chapter 9)

`and` / `or` return the *operand value*, not a coerced boolean — and never
evaluate the right side once the answer is known:

```lox
print "hi" or 2;    // hi
print 1 and 2;      // 2
print nil or "fallback"; // fallback
```

### Functions, recursion, and `clock` (chapter 10)

```lox
fun fib(n) {
  if (n < 2) return n;
  return fib(n - 2) + fib(n - 1);
}

var start = clock();
print fib(20);                       // 6765
print clock() - start;               // elapsed seconds
```

### Closures (chapter 10)

The book's classic `makeCounter` — `count` captures the surrounding `i`:

```lox
fun makeCounter() {
  var i = 0;
  fun count() {
    i = i + 1;
    return i;
  }
  return count;
}

var c = makeCounter();
print c();  // 1
print c();  // 2
print c();  // 3
```

### Closure-capture correctness via the resolver (chapter 11)

Re-declaring `a` after a closure captures it must *not* leak the new
binding into the closure. This is the chapter-11 motivating fragment, and
the resolver makes it work out of the box:

```lox
var a = "global";
{
  fun showA() { print a; }
  showA();          // global
  var a = "block";
  showA();          // global  (NOT "block")
}
```

### Classes, methods, `this`, and `init` (chapter 12)

```lox
class Point {
  init(x, y) {
    this.x = x;
    this.y = y;
  }
  distance(other) {
    var dx = this.x - other.x;
    var dy = this.y - other.y;
    return dx * dx + dy * dy;        // squared
  }
}

var origin = Point(0, 0);
var p      = Point(3, 4);
print p.distance(origin);            // 25
```

### Inheritance and `super` (chapter 13)

The book's canonical `super` walkthrough — `C().test()` runs `B.test`
which dispatches `super.method()` to `A.method`, *not* `B.method`:

```lox
class A { method() { print "A method"; } }

class B < A {
  method() { print "B method"; }
  test()   { super.method(); }
}

class C < B {}

C().test();  // A method
```

### Static error caught by the resolver

```lox
class Cake { init() { return 42; } }
//                    ^^^^^^^^^^ Can't return a value from an initializer.
```

The resolver flags this before any code runs, so no half-built `Cake`
escapes into the runtime.

### Exit codes

Matching jlox / `sysexits.h`:

| Code | Meaning                                |
| ---- | -------------------------------------- |
| 0    | success                                |
| 2    | clap usage error (unknown flag)        |
| 64   | runtime usage error (file unreadable)  |
| 65   | compile error (scan / parse)           |
| 70   | runtime error                          |

### Smoke tests

A Python harness drives the binary against a corpus of `.lox` programs
under [`scripts/smoke/`](scripts/smoke/), each annotated with
`// expect:` / `// expect_runtime_error:` / `// expect_compile_error:`
directives (same convention as the upstream `craftinginterpreters` test
suite). Useful as a quick end-to-end check covering happy paths,
runtime errors, and resolver-time static errors:

```sh
scripts/smoke_test.py                # build release + run every .lox
scripts/smoke_test.py --debug        # use the debug artifact
scripts/smoke_test.py --filter class # only files with "class" in the name
scripts/smoke_test.py --no-build     # reuse an existing artifact
```

The same harness runs in CI on every push and pull request as part of the
`Build (x86_64-unknown-linux-gnu)` and `Build (aarch64-unknown-linux-gnu)`
required checks, so a regression that only manifests end-to-end (REPL
prompt, exit codes, stderr formatting) blocks merges just like a unit-test
failure.

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
