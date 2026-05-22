<!--
Title format: <type>(<optional scope>): <subject>
Examples:
  feat(scanner): support multi-line strings
  fix(parser): synchronize after missing semicolon
  test(interpreter): cover string concatenation type errors
-->

## Summary

<!-- One or two sentences describing what this PR does and why. -->

## Phase / scope

<!-- Reference the milestone phase/chapter if applicable, or describe the logical scope. -->

## TDD checklist

- [ ] Failing tests committed first (Red)
- [ ] Implementation makes the tests pass (Green)
- [ ] Refactor pass complete (Refactor)
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [ ] `cargo test --workspace --all-targets --locked`
- [ ] `cargo test -p rlox --no-default-features --features vm`
- [ ] `cargo clippy -p rlox --no-default-features --features vm --all-targets -- -D warnings`
- [ ] Coverage not regressed

## Notes

<!-- Anything reviewers should know: trade-offs, follow-ups, references. -->
