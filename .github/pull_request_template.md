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

<!-- Reference the milestone phase if applicable, e.g. "Phase 2 - scanner.rs". -->

## TDD checklist

- [ ] Failing tests committed first (Red)
- [ ] Implementation makes the tests pass (Green)
- [ ] Refactor pass complete (Refactor)
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test --all-targets`
- [ ] Coverage not regressed

## Notes

<!-- Anything reviewers should know: trade-offs, follow-ups, references. -->
