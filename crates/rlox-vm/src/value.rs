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
//! - `Obj(a) == Obj(b)` requires dereferencing through the heap and is
//!   exposed by [`values_equal`] rather than [`PartialEq`] because it
//!   needs the heap as context. Chapter 21 will intern strings and
//!   collapse this back to pointer (handle) equality.
//! - Mixed-type comparisons are `false` rather than a runtime error;
//!   the runtime error path is reserved for ordering (`<`, `>`) and
//!   arithmetic, which require both operands to be numbers.

use std::fmt;

use crate::heap::{Handle, Heap, Obj};

/// A Lox runtime value.
///
/// Inline variants (`Nil`, `Bool`, `Number`) are `Copy` and small;
/// `Obj` carries a [`Handle`] into the VM's [`Heap`] so the value
/// itself stays a fixed-size enum.
#[derive(Debug, Clone, Copy)]
pub enum Value {
    /// The singleton `nil`.
    Nil,
    /// A boolean.
    Bool(bool),
    /// A 64-bit IEEE-754 floating-point number — the only Lox numeric type.
    Number(f64),
    /// A heap-allocated object (chapter 19: only strings; later chapters
    /// add functions, closures, classes, and instances).
    Obj(Handle),
}

/// Custom [`PartialEq`] that mirrors clox's `==`/`!=` semantics for
/// the inline variants. `Obj == Obj` returns `true` only when the two
/// handles are byte-identical; this is correct for chapter 21
/// onwards once strings are interned, but in chapter 19 callers that
/// want content equality (e.g. the `OP_EQUAL` interpreter arm) must
/// use [`values_equal`] instead.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Nil, Self::Nil) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::Obj(a), Self::Obj(b)) => a == b,
            _ => false,
        }
    }
}

/// Polymorphic equality that dereferences object handles through
/// `heap` so two distinct allocations of `"abc"` compare equal.
///
/// Chapter 21 (string interning) makes pointer equality correct on
/// its own, at which point this helper can shrink to `a == b`.
#[must_use]
pub fn values_equal(a: Value, b: Value, heap: &Heap) -> bool {
    match (a, b) {
        (Value::Obj(x), Value::Obj(y)) => {
            // Identical handles are trivially equal; otherwise walk
            // through the heap and compare contents per variant.
            if x == y {
                return true;
            }
            match (heap.get(x), heap.get(y)) {
                (Obj::Str(a), Obj::Str(b)) => a == b,
            }
        }
        // Inline variants delegate to the derived comparator above.
        _ => a == b,
    }
}

impl Value {
    /// Lox truthiness: `nil` and `false` are falsy; everything else
    /// (including `0`, the empty string, and any non-`nil` non-`false`
    /// object) is truthy. Matches jlox/clox.
    #[must_use]
    pub const fn is_truthy(self) -> bool {
        !matches!(self, Self::Nil | Self::Bool(false))
    }

    /// Wrap the value in a heap-aware [`Display`] adapter. Use this
    /// any time a value containing an [`Obj`] handle needs to be
    /// rendered; the bare `Display` impl can only emit a placeholder
    /// for objects since it doesn't see the heap.
    #[must_use]
    pub const fn display(self, heap: &Heap) -> ValueDisplay<'_> {
        ValueDisplay { value: self, heap }
    }
}

/// Heap-aware [`Display`] adapter. Strings render verbatim (matching
/// clox's `printValue`/`printObject` for `OBJ_STRING`), other future
/// object kinds will pick their own rendering.
pub struct ValueDisplay<'a> {
    value: Value,
    heap: &'a Heap,
}

impl fmt::Display for ValueDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            Value::Obj(h) => match self.heap.get(h) {
                Obj::Str(s) => f.write_str(s),
            },
            // Inline variants delegate to the bare Display impl below.
            other => write!(f, "{other}"),
        }
    }
}

/// Bare [`Display`] for [`Value`]. For inline variants this matches
/// clox's `printValue`; for [`Value::Obj`] it falls back to a stable
/// `obj#N` placeholder since the heap isn't in scope. Use
/// [`Value::display`] whenever the actual contents matter.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => f.write_str("nil"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Number(n) => write!(f, "{n}"),
            Self::Obj(h) => write!(f, "<{h}>"),
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

    #[test]
    fn obj_strings_are_truthy() {
        let mut heap = Heap::new();
        let h = heap.alloc_string("");
        assert!(Value::Obj(h).is_truthy()); // empty string is truthy
    }

    #[test]
    fn display_with_heap_renders_strings() {
        let mut heap = Heap::new();
        let h = heap.alloc_string("hello");
        assert_eq!(Value::Obj(h).display(&heap).to_string(), "hello");
    }

    #[test]
    fn values_equal_compares_string_contents() {
        let mut heap = Heap::new();
        let a = heap.alloc_string("abc");
        let b = heap.alloc_string("abc");
        let c = heap.alloc_string("xyz");
        // Distinct allocations of the same content are equal in
        // chapter 19 (interning lands in chapter 21).
        assert!(values_equal(Value::Obj(a), Value::Obj(b), &heap));
        assert!(!values_equal(Value::Obj(a), Value::Obj(c), &heap));
        // Mixed types remain unequal without raising.
        assert!(!values_equal(Value::Obj(a), Value::Number(1.0), &heap));
    }
}
