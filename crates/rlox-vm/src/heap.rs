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
//!   ([`values_equal`](crate::value::values_equal)). This keeps string
//!   equality correct without requiring the custom interning table from
//!   clox chapter 20.
//! - **Garbage collection** in chapter 26 will replace the bare `Vec`
//!   with a free-list-friendly storage and possibly grow [`Handle`]
//!   with a generation tag. Today the heap only grows.
//!
//! Today the heap stores strings plus callable objects:
//! [`Obj::Function`] for compiled function prototypes, [`Obj::Closure`]
//! for runtime function values, [`Obj::Upvalue`] for captured locals,
//! and [`Obj::Native`] for built-ins such as `clock`. Chapter 27
//! (classes) and chapter 28 (instances) will contribute new variants
//! here.

use std::fmt;

use crate::chunk::Chunk;
use crate::value::Value;

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

/// A user-defined Lox function. Functions are first-class values and
/// carry their own bytecode chunk plus fixed arity.
#[derive(Debug, Clone)]
pub struct Function {
    /// `None` is reserved for the implicit top-level script function in
    /// the book. This Rust VM keeps top-level code as a bare [`Chunk`],
    /// but the display path still handles the script case for traces.
    pub name: Option<String>,
    /// Number of parameters expected by this function.
    pub arity: u8,
    /// Number of upvalues captured by closures wrapping this function.
    pub upvalue_count: u8,
    /// Bytecode body for the function.
    pub chunk: Chunk,
}

impl Function {
    /// Create a named or script function.
    #[must_use]
    pub const fn new(name: Option<String>, arity: u8, chunk: Chunk) -> Self {
        Self {
            name,
            arity,
            upvalue_count: 0,
            chunk,
        }
    }

    /// Create a function with known upvalue metadata.
    #[must_use]
    pub const fn with_upvalues(
        name: Option<String>,
        arity: u8,
        upvalue_count: u8,
        chunk: Chunk,
    ) -> Self {
        Self {
            name,
            arity,
            upvalue_count,
            chunk,
        }
    }

    /// Human-readable name used in value display and diagnostics.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("<script>")
    }
}

/// A runtime closure around a compiled [`Function`].
#[derive(Debug, Clone)]
pub struct Closure {
    /// Handle to the compiled function prototype.
    pub function: Handle,
    /// Handles to captured upvalue objects, in compiler-assigned order.
    pub upvalues: Vec<Handle>,
}

impl Closure {
    /// Create a closure for `function`.
    #[must_use]
    pub const fn new(function: Handle, upvalues: Vec<Handle>) -> Self {
        Self { function, upvalues }
    }
}

/// Storage location for a captured variable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UpvalueLocation {
    /// The variable still lives in the VM stack at this absolute slot.
    Open(usize),
    /// The declaring stack frame has returned; the variable now lives
    /// in the upvalue object itself.
    Closed(Value),
}

/// A captured variable shared by one or more closures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Upvalue {
    /// Current storage location for the variable.
    pub location: UpvalueLocation,
}

impl Upvalue {
    /// Create an open upvalue pointing at `slot`.
    #[must_use]
    pub const fn open(slot: usize) -> Self {
        Self {
            location: UpvalueLocation::Open(slot),
        }
    }
}

/// A built-in callable implemented by Rust code.
#[derive(Clone, Copy)]
pub struct NativeFunction {
    /// Name exposed to Lox source.
    pub name: &'static str,
    /// Number of arguments expected by the native.
    pub arity: u8,
    /// Implementation invoked after arity has been checked.
    pub function: fn(&[Value]) -> Value,
}

impl fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeFunction")
            .field("name", &self.name)
            .field("arity", &self.arity)
            .finish_non_exhaustive()
    }
}

/// A heap-allocated VM object.
#[derive(Debug, Clone)]
pub enum Obj {
    /// A Lox string backed by an owned `String`.
    Str(String),
    /// A compiled user-defined Lox function prototype.
    Function(Function),
    /// A runtime callable closure.
    Closure(Closure),
    /// A captured local variable.
    Upvalue(Upvalue),
    /// A native Rust function exposed to Lox.
    Native(NativeFunction),
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
    /// Strings are stored verbatim; equal contents may have distinct
    /// handles.
    pub fn alloc_string(&mut self, s: impl Into<String>) -> Handle {
        self.alloc(Obj::Str(s.into()))
    }

    /// Allocate an [`Obj::Function`] and return its handle.
    pub fn alloc_function(&mut self, function: Function) -> Handle {
        self.alloc(Obj::Function(function))
    }

    /// Allocate an [`Obj::Closure`] and return its handle.
    pub fn alloc_closure(&mut self, closure: Closure) -> Handle {
        self.alloc(Obj::Closure(closure))
    }

    /// Allocate an [`Obj::Upvalue`] and return its handle.
    pub fn alloc_upvalue(&mut self, upvalue: Upvalue) -> Handle {
        self.alloc(Obj::Upvalue(upvalue))
    }

    /// Allocate an [`Obj::Native`] and return its handle.
    pub fn alloc_native(&mut self, native: NativeFunction) -> Handle {
        self.alloc(Obj::Native(native))
    }

    fn alloc(&mut self, obj: Obj) -> Handle {
        let idx =
            u32::try_from(self.objs.len()).expect("heap exceeded u32 capacity (4 billion objects)");
        self.objs.push(obj);
        Handle(idx)
    }

    /// Borrow the object referenced by `h`. Panics on a stale handle —
    /// today that can only happen through programmer error since
    /// nothing deallocates yet.
    #[must_use]
    pub fn get(&self, h: Handle) -> &Obj {
        &self.objs[h.0 as usize]
    }

    /// Mutably borrow the object referenced by `h`.
    #[must_use]
    pub fn get_mut(&mut self, h: Handle) -> &mut Obj {
        &mut self.objs[h.0 as usize]
    }

    /// Convenience accessor for string objects. Returns `None` if the
    /// handle points at a non-string object.
    #[must_use]
    pub fn as_str(&self, h: Handle) -> Option<&str> {
        match self.get(h) {
            Obj::Str(s) => Some(s),
            Obj::Function(_) | Obj::Closure(_) | Obj::Upvalue(_) | Obj::Native(_) => None,
        }
    }

    /// Convenience accessor for function objects.
    #[must_use]
    pub fn as_function(&self, h: Handle) -> Option<&Function> {
        match self.get(h) {
            Obj::Function(function) => Some(function),
            Obj::Str(_) | Obj::Closure(_) | Obj::Upvalue(_) | Obj::Native(_) => None,
        }
    }

    /// Convenience accessor for closure objects.
    #[must_use]
    pub fn as_closure(&self, h: Handle) -> Option<&Closure> {
        match self.get(h) {
            Obj::Closure(closure) => Some(closure),
            Obj::Str(_) | Obj::Function(_) | Obj::Upvalue(_) | Obj::Native(_) => None,
        }
    }

    /// Mutable convenience accessor for closure objects.
    #[must_use]
    pub fn as_closure_mut(&mut self, h: Handle) -> Option<&mut Closure> {
        match self.get_mut(h) {
            Obj::Closure(closure) => Some(closure),
            Obj::Str(_) | Obj::Function(_) | Obj::Upvalue(_) | Obj::Native(_) => None,
        }
    }

    /// Convenience accessor for upvalue objects.
    #[must_use]
    pub fn as_upvalue(&self, h: Handle) -> Option<Upvalue> {
        match self.get(h) {
            Obj::Upvalue(upvalue) => Some(*upvalue),
            Obj::Str(_) | Obj::Function(_) | Obj::Closure(_) | Obj::Native(_) => None,
        }
    }

    /// Mutable convenience accessor for upvalue objects.
    #[must_use]
    pub fn as_upvalue_mut(&mut self, h: Handle) -> Option<&mut Upvalue> {
        match self.get_mut(h) {
            Obj::Upvalue(upvalue) => Some(upvalue),
            Obj::Str(_) | Obj::Function(_) | Obj::Closure(_) | Obj::Native(_) => None,
        }
    }

    /// Convenience accessor for native function objects.
    #[must_use]
    pub fn as_native(&self, h: Handle) -> Option<NativeFunction> {
        match self.get(h) {
            Obj::Native(native) => Some(*native),
            Obj::Str(_) | Obj::Function(_) | Obj::Closure(_) | Obj::Upvalue(_) => None,
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
        // No interning yet, so equal strings get distinct handles.
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

    #[test]
    fn alloc_callables_and_upvalues_store_metadata() {
        fn native(_args: &[Value]) -> Value {
            Value::Nil
        }

        let mut h = Heap::new();
        let function = h.alloc_function(Function::new(Some("f".to_string()), 2, Chunk::new()));
        let upvalue = h.alloc_upvalue(Upvalue::open(3));
        let closure = h.alloc_closure(Closure::new(function, vec![upvalue]));
        let native = h.alloc_native(NativeFunction {
            name: "clock",
            arity: 0,
            function: native,
        });

        assert_eq!(h.as_function(function).unwrap().display_name(), "f");
        assert_eq!(h.as_closure(closure).unwrap().function, function);
        assert_eq!(
            h.as_upvalue(upvalue).unwrap().location,
            UpvalueLocation::Open(3)
        );
        assert_eq!(h.as_native(native).unwrap().name, "clock");
        assert_eq!(h.as_str(function), None);
        assert!(h.as_function(native).is_none());
    }
}
