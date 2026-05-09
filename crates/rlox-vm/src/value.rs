//! Runtime values manipulated by the bytecode VM.
//!
//! Chapter 14 deals only with numeric literals, so [`Value`] starts as a
//! tagged enum with a single `Number(f64)` arm. Chapter 18 (Types of
//! Values) will add `Nil` / `Bool`, and chapter 19 (Strings) will add
//! `Obj(Handle<Obj>)` once the heap arrives. Keeping the enum here from
//! day one means downstream code (the disassembler, the constant pool)
//! never has to be retrofitted from a bare `f64`.

use std::fmt;

/// A Lox runtime value.
///
/// Equality on numbers uses `f64`'s bit-for-bit `PartialEq` (which is
/// what jlox / clox effectively do via `==`), so `NaN != NaN`. That's a
/// quirk of the reference implementation and the upstream test suite
/// agrees, so we mirror it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// A 64-bit IEEE-754 floating-point number — the only Lox numeric type.
    Number(f64),
}

impl Value {
    /// Format the value the way `OP_PRINT` will once it exists, and the
    /// way the chapter 14 disassembler shows constants.
    ///
    /// Matches jlox / clox output: whole numbers render as `123`,
    /// fractions render naturally without trailing zeros.
    #[must_use]
    pub fn print_repr(&self) -> String {
        format!("{self}")
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Chapter 14 / 18 disassembler examples (e.g. `1.2`, `42`)
            // use Rust's default `{}` for f64, which already strips
            // trailing zeros for whole numbers and avoids scientific
            // notation for typical Lox programs.
            Self::Number(n) => write!(f, "{n}"),
        }
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Self::Number(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_renders_whole_numbers_without_decimal() {
        assert_eq!(format!("{}", Value::Number(42.0)), "42");
    }

    #[test]
    fn display_renders_fractions_naturally() {
        assert_eq!(format!("{}", Value::Number(1.2)), "1.2");
    }

    #[test]
    fn from_f64_constructs_number() {
        let v: Value = 1.5_f64.into();
        assert_eq!(v, Value::Number(1.5));
    }
}
