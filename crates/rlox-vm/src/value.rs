//! Runtime values manipulated by the bytecode VM.
//!
//! Chapter 14 deals only with numeric literals; chapter 18 (*Types of
//! Values*) introduces the full Lox value taxonomy modulo objects:
//!
//! - [`Value::Nil`] — the singleton nil.
//! - [`Value::Bool`] — `true` or `false`.
//! - [`Value::Number`] — a 64-bit IEEE-754 float.
//!
//! Chapter 19 (*Strings*) will add `Obj(Handle<Obj>)` once the heap
//! arrives.
//!
//! Equality follows clox/jlox semantics:
//!
//! - `Nil == Nil` only.
//! - `Bool(a) == Bool(b)` iff `a == b`.
//! - `Number(a) == Number(b)` uses `f64`'s bit-level `PartialEq`, so
//!   `NaN != NaN`. The reference implementation agrees.
//! - Mixed-type comparisons are `false` rather than a runtime error;
//!   the runtime error path is reserved for ordering (`<`, `>`) and
//!   arithmetic, which require both operands to be numbers.

use std::fmt;

/// A Lox runtime value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// The singleton `nil`.
    Nil,
    /// A boolean.
    Bool(bool),
    /// A 64-bit IEEE-754 floating-point number — the only Lox numeric type.
    Number(f64),
}

impl Value {
    /// Format the value the way `OP_PRINT` will once it exists, and the
    /// way the chapter 14 disassembler shows constants. Matches jlox /
    /// clox output.
    #[must_use]
    pub fn print_repr(&self) -> String {
        format!("{self}")
    }

    /// Lox truthiness: `nil` and `false` are falsy; everything else
    /// (including `0`, the empty string, and any non-`nil` non-`false`
    /// object) is truthy. Matches jlox/clox.
    #[must_use]
    pub const fn is_truthy(self) -> bool {
        !matches!(self, Self::Nil | Self::Bool(false))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => f.write_str("nil"),
            Self::Bool(b) => write!(f, "{b}"),
            // Whole numbers render without decimal, fractions naturally,
            // matching clox's `printValue` and the upstream test corpus.
            Self::Number(n) => write!(f, "{n}"),
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Self::Bool(b)
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

    #[test]
    fn display_renders_nil_and_bools() {
        assert_eq!(Value::Nil.to_string(), "nil");
        assert_eq!(Value::Bool(true).to_string(), "true");
        assert_eq!(Value::Bool(false).to_string(), "false");
    }

    #[test]
    fn is_truthy_matches_lox_rules() {
        // Only nil and false are falsy.
        assert!(!Value::Nil.is_truthy());
        assert!(!Value::Bool(false).is_truthy());
        assert!(Value::Bool(true).is_truthy());
        assert!(Value::Number(0.0).is_truthy()); // 0 is truthy in Lox
        assert!(Value::Number(-1.0).is_truthy());
    }
}
