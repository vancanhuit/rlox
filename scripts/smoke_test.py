#!/usr/bin/env python3
"""Smoke-test the rlox interpreter against the .lox corpus under
``scripts/smoke/``.

Each program may include directive comments:

    // expect: <line>             — append <line> to expected stdout
    // expect_runtime_error: <m>  — expect exit code 70, <m> in stderr
    // expect_compile_error: <m>  — expect exit code 65, <m> in stderr

``expect:`` directives may appear multiple times; their order matters.
``expect_runtime_error:`` and ``expect_compile_error:`` use only the first
occurrence (you usually only have one).

Usage::

    scripts/smoke_test.py [--release|--debug] [--filter SUBSTR] [--no-build]

Exits 0 when every test passes, 1 otherwise. Suitable for CI.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SMOKE_DIR = REPO_ROOT / "scripts" / "smoke"

# Exit codes documented in src/main.rs.
EX_COMPILE = 65
EX_RUNTIME = 70

# Match a directive anywhere in a line, so trailing inline comments work:
#     print 1 + 2;  // expect: 3
# The book's own test runner uses the same convention.
DIRECTIVE_RE = re.compile(
    r"//\s*(expect|expect_runtime_error|expect_compile_error):\s?(.*?)\s*$"
)


# ---- terminal colours ----------------------------------------------------


def _supports_colour() -> bool:
    return sys.stdout.isatty() and os.environ.get("NO_COLOR") is None


if _supports_colour():
    GREEN = "\033[32m"
    RED = "\033[31m"
    DIM = "\033[2m"
    RESET = "\033[0m"
else:
    GREEN = RED = DIM = RESET = ""


# ---- directive parsing ---------------------------------------------------


@dataclass
class Expectations:
    """Parsed directives from a single .lox source file."""

    stdout_lines: list[str] = field(default_factory=list)
    runtime_error: str | None = None
    compile_error: str | None = None

    @classmethod
    def parse(cls, path: Path) -> "Expectations":
        exp = cls()
        for raw in path.read_text(encoding="utf-8").splitlines():
            m = DIRECTIVE_RE.search(raw)
            if not m:
                continue
            kind, rest = m.group(1), m.group(2)
            if kind == "expect":
                exp.stdout_lines.append(rest)
            elif kind == "expect_runtime_error" and exp.runtime_error is None:
                exp.runtime_error = rest
            elif kind == "expect_compile_error" and exp.compile_error is None:
                exp.compile_error = rest
        return exp

    @property
    def expected_stdout(self) -> str:
        return "\n".join(self.stdout_lines)


# ---- test outcome --------------------------------------------------------


@dataclass
class Outcome:
    name: str
    passed: bool
    reason: str = ""
    expected_stdout: str = ""
    actual_stdout: str = ""


# ---- driver --------------------------------------------------------------


def build_binary(profile: str) -> Path:
    args = ["cargo", "build", "--quiet"]
    if profile == "release":
        args.append("--release")
    print(f"Building rlox ({profile})...")
    subprocess.run(args, cwd=REPO_ROOT, check=True)
    return REPO_ROOT / "target" / profile / "rlox"


def run_one(binary: Path, source: Path) -> Outcome:
    name = source.stem
    exp = Expectations.parse(source)

    proc = subprocess.run(
        [str(binary), str(source)],
        capture_output=True,
        text=True,
        check=False,
    )
    rc = proc.returncode
    actual_stdout = proc.stdout.rstrip("\n")
    actual_stderr = proc.stderr

    expected_stdout = exp.expected_stdout

    if exp.runtime_error is not None:
        if rc != EX_RUNTIME:
            return Outcome(
                name,
                False,
                f"exit code {rc} (expected {EX_RUNTIME} for runtime error)",
            )
        if exp.runtime_error not in actual_stderr:
            return Outcome(
                name,
                False,
                f"stderr missing expected runtime message: "
                f"{exp.runtime_error!r}\n  stderr was: {actual_stderr!r}",
            )
        return Outcome(name, True)

    if exp.compile_error is not None:
        if rc != EX_COMPILE:
            return Outcome(
                name,
                False,
                f"exit code {rc} (expected {EX_COMPILE} for compile error)",
            )
        if exp.compile_error not in actual_stderr:
            return Outcome(
                name,
                False,
                f"stderr missing expected compile message: "
                f"{exp.compile_error!r}\n  stderr was: {actual_stderr!r}",
            )
        return Outcome(name, True)

    # Happy-path: exit 0 and stdout matches.
    if rc != 0:
        return Outcome(
            name,
            False,
            f"exit code {rc} (expected 0); stderr: {actual_stderr!r}",
        )
    if actual_stdout != expected_stdout:
        return Outcome(
            name,
            False,
            "stdout differs",
            expected_stdout=expected_stdout,
            actual_stdout=actual_stdout,
        )
    return Outcome(name, True)


def report(outcome: Outcome) -> None:
    if outcome.passed:
        print(f"{GREEN}PASS{RESET} {outcome.name}")
        return
    print(f"{RED}FAIL{RESET} {outcome.name}")
    print(f"  {DIM}{outcome.reason}{RESET}")
    if outcome.expected_stdout != outcome.actual_stdout:
        print(f"  {DIM}--- expected ---{RESET}")
        for line in outcome.expected_stdout.splitlines() or [""]:
            print(f"    {line}")
        print(f"  {DIM}--- actual ---{RESET}")
        for line in outcome.actual_stdout.splitlines() or [""]:
            print(f"    {line}")


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    profile = ap.add_mutually_exclusive_group()
    profile.add_argument(
        "--release",
        dest="profile",
        action="store_const",
        const="release",
        help="build and test against the release artifact (default)",
    )
    profile.add_argument(
        "--debug",
        dest="profile",
        action="store_const",
        const="debug",
        help="build and test against the debug artifact",
    )
    ap.set_defaults(profile="release")
    ap.add_argument(
        "--no-build",
        action="store_true",
        help="skip the cargo build step (use existing artifact)",
    )
    ap.add_argument(
        "--filter",
        metavar="SUBSTR",
        default="",
        help="only run tests whose name contains this substring",
    )
    args = ap.parse_args()

    if args.no_build:
        binary = REPO_ROOT / "target" / args.profile / "rlox"
        if not binary.exists():
            print(
                f"--no-build given but {binary} doesn't exist", file=sys.stderr
            )
            return 1
    else:
        binary = build_binary(args.profile)

    files = sorted(SMOKE_DIR.glob("*.lox"))
    if args.filter:
        files = [f for f in files if args.filter in f.stem]
    if not files:
        print(f"No tests matched filter: {args.filter!r}", file=sys.stderr)
        return 1

    outcomes = [run_one(binary, f) for f in files]
    for o in outcomes:
        report(o)

    passed = sum(1 for o in outcomes if o.passed)
    failed = len(outcomes) - passed
    print()
    print(
        f"{GREEN}{passed} passed{RESET}, "
        f"{RED}{failed} failed{RESET} out of {len(outcomes)}"
    )
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
