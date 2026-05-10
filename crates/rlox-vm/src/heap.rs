//! Safe handle-based heap for VM-managed objects.
//!
//! clox uses an intrusive linked list of `Obj*` nodes plus manual
//! ownership tracking; chapter 26 adds a mark-sweep GC by walking the
//! same list and `free`ing white nodes. We keep the design GC-ready
//! while staying within the workspace-wide `unsafe_code = "forbid"`
//! lint by storing every object in a `Vec<Obj>` and handing out opaque
//! [`Handle`] indices instead of raw pointers.
//!
//! - **Allocation** appends to the vector and returns the new index.
//! - **Lookup** is `O(1)` via `Heap::get(handle)`.
//! - **Equality** can dereference through the heap and compare contents
//!   ([`values_equal`](crate::value::values_equal)). Chapter 21 will
//!   intern strings and let the equality check fall back to pointer
//!   identity.
//! - **Garbage collection** in chapter 26 will replace the bare `Vec`
//!   with a free-list-friendly storage and possibly grow [`Handle`]
//!   with a generation tag. Today the heap only grows.
//!
//! Today the only object kind is [`Obj::Str`]. Chapter 24 (functions),
//! chapter 25 (closures), chapter 27 (classes), and chapter 28
//! (instances) will each contribute new variants here.

use std::fmt;

/// Opaque handle into a [`Heap`].
///
/// The internal index is intentionally not part of the public API:
/// chapter 26 will likely tag it with a generation counter for safer
/// debugging, and we don't want callers to start doing arithmetic on
/// raw indices in the meantime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle(u32);

impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The disassembler / debug trace emit `<obj#N>` for an
        // un-dereferenced handle, matching the visual style of clox's
        // `printObject` shorthand for unknown types.
        write!(f, "obj#{}", self.0)
    }
}

/// A heap-allocated VM object. Chapter 19 only knows about strings;
/// later chapters will grow this enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Obj {
    /// A Lox string. Backed by an owned `String`; chapter 21 will add
    /// interning so equal strings share a single allocation.
    Str(String),
}

/// VM heap. Owns every object referenced by a [`Value::Obj`]
/// elsewhere in the runtime.
///
/// [`Value::Obj`]: crate::value::Value::Obj
#[derive(Debug, Default)]
pub struct Heap {
    objs: Vec<Obj>,
}

impl Heap {
    /// Create an empty heap. Cheap; no syscalls.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate an [`Obj::Str`] containing `s` and return its handle.
    /// Strings are stored verbatim — interning waits for chapter 21.
    pub fn alloc_string(&mut self, s: impl Into<String>) -> Handle {
        let idx =
            u32::try_from(self.objs.len()).expect("heap exceeded u32 capacity (4 billion objects)");
        self.objs.push(Obj::Str(s.into()));
        Handle(idx)
    }

    /// Borrow the object referenced by `h`. Panics on a stale handle —
    /// today that can only happen through programmer error since
    /// nothing deallocates yet.
    #[must_use]
    pub fn get(&self, h: Handle) -> &Obj {
        &self.objs[h.0 as usize]
    }

    /// Convenience accessor for the (currently only) string variant.
    /// Returns `None` if the handle points at a non-string object;
    /// today there are no other kinds, so this is `Some` in practice
    /// but the helper future-proofs callers.
    #[must_use]
    pub fn as_str(&self, h: Handle) -> Option<&str> {
        match self.get(h) {
            Obj::Str(s) => Some(s),
        }
    }

    /// Number of objects currently allocated. Useful for tests; this
    /// will become a less interesting number once chapter 26 adds GC.
    #[must_use]
    pub fn len(&self) -> usize {
        self.objs.len()
    }

    /// `true` if no objects are allocated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_string_returns_distinct_handles_per_call() {
        let mut h = Heap::new();
        let a = h.alloc_string("foo");
        let b = h.alloc_string("foo");
        // Chapter 19: no interning yet, so equal strings get distinct
        // handles. Chapter 21 will collapse these to one.
        assert_ne!(a, b);
        assert_eq!(h.as_str(a), Some("foo"));
        assert_eq!(h.as_str(b), Some("foo"));
    }

    #[test]
    fn handle_display_uses_obj_prefix() {
        let mut h = Heap::new();
        let s = h.alloc_string("x");
        assert_eq!(s.to_string(), "obj#0");
    }

    #[test]
    fn empty_and_len_track_allocations() {
        let mut h = Heap::new();
        assert!(h.is_empty());
        h.alloc_string("a");
        h.alloc_string("b");
        assert_eq!(h.len(), 2);
        assert!(!h.is_empty());
    }
}
