//! Safe handle-based heap for VM-managed objects.
//!
//! clox uses an intrusive linked list of `Obj*` nodes plus manual
//! ownership tracking; chapter 26 adds a mark-sweep GC by walking the
//! same list and `free`ing white nodes. This crate keeps the same
//! reachability model while staying within the workspace-wide
//! `unsafe_code = "forbid"` lint by storing every object in heap slots
//! and handing out opaque [`Handle`] values instead of raw pointers.
//!
//! - **Allocation** stores the object in either a reclaimed slot or a
//!   fresh slot and returns its handle.
//! - **Lookup** is `O(1)` via `Heap::get(handle)`.
//! - **Equality** can dereference through the heap and compare contents
//!   ([`values_equal`](crate::value::values_equal)). This keeps string
//!   equality correct without requiring the custom interning table from
//!   clox chapter 20.
//! - **Garbage collection** is precise mark-sweep: roots are marked,
//!   object references are traced through a gray stack, and unmarked
//!   slots are swept into a free list. [`Handle`] includes a generation
//!   tag so using a stale handle after collection panics instead of
//!   aliasing a later object that reused the same slot.
//!
//! Today the heap stores strings plus callable objects:
//! [`Obj::Function`] for compiled function prototypes, [`Obj::Closure`]
//! for runtime function values, [`Obj::Upvalue`] for captured locals,
//! [`Obj::Native`] for built-ins such as `clock`, [`Obj::Class`] for
//! user-defined classes, [`Obj::Instance`] for field-bearing class
//! instances, and [`Obj::BoundMethod`] for methods captured from a
//! receiver. Inheritance arrives in a later chapter.

use std::collections::HashMap;
use std::fmt;

use crate::chunk::Chunk;
use crate::value::Value;

const INITIAL_GC_THRESHOLD: usize = 1024;
const GC_HEAP_GROW_FACTOR: usize = 2;

/// Opaque handle into a [`Heap`].
///
/// The internal slot index and generation are intentionally not part of
/// the public API; callers should treat handles as copyable capabilities
/// that remain meaningful only while the referenced object is live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle {
    index: u32,
    generation: u32,
}

impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The disassembler / debug trace emit `<obj#N>` for an
        // un-dereferenced handle, matching the visual style of clox's
        // `printObject` shorthand for unknown types.
        write!(f, "obj#{}", self.index)
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

/// A method closure paired with the receiver it was accessed from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundMethod {
    /// Receiver bound to the method's implicit `this`.
    pub receiver: Value,
    /// Closure to call when the bound method is invoked.
    pub method: Handle,
}

impl BoundMethod {
    /// Create a method value bound to `receiver`.
    #[must_use]
    pub const fn new(receiver: Value, method: Handle) -> Self {
        Self { receiver, method }
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

/// A user-defined Lox class. Classes are callable factories for
/// instances and own a method table populated by `OP_METHOD`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    /// Name used for display and diagnostics.
    pub name: String,
    /// Methods keyed by method name. Values are handles to closures.
    pub methods: HashMap<String, Handle>,
}

impl Class {
    /// Create a class named `name`.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            methods: HashMap::new(),
        }
    }
}

/// A runtime instance of a Lox [`Class`].
#[derive(Debug, Clone)]
pub struct Instance {
    /// Class this instance was created from.
    pub class: Handle,
    /// Dynamically added fields, keyed by property name.
    pub fields: HashMap<String, Value>,
}

impl Instance {
    /// Create an empty instance of `class`.
    #[must_use]
    pub fn new(class: Handle) -> Self {
        Self {
            class,
            fields: HashMap::new(),
        }
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
    /// A receiver bound to a method closure.
    BoundMethod(BoundMethod),
    /// A captured local variable.
    Upvalue(Upvalue),
    /// A native Rust function exposed to Lox.
    Native(NativeFunction),
    /// A user-defined class object.
    Class(Class),
    /// A runtime instance with dynamically assigned fields.
    Instance(Instance),
}

/// Roots that seed one garbage-collection run.
///
/// VM roots are usually stack/global values plus handles stored in call
/// frames and open-upvalue lists. `chunks` covers compiled code that is
/// borrowed by the VM, such as the top-level script chunk, whose
/// constants may contain object handles.
#[derive(Debug, Default, Clone, Copy)]
pub struct GcRoots<'a> {
    /// Direct value roots.
    pub values: &'a [Value],
    /// Direct object-handle roots.
    pub handles: &'a [Handle],
    /// Compiled chunks whose constants should be considered roots.
    pub chunks: &'a [&'a Chunk],
}

impl<'a> GcRoots<'a> {
    /// Empty root set.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Root only the supplied values.
    #[must_use]
    pub const fn values(values: &'a [Value]) -> Self {
        Self {
            values,
            handles: &[],
            chunks: &[],
        }
    }
}

/// Object-count statistics for one collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcStats {
    /// Live object count before marking and sweeping.
    pub before: usize,
    /// Live object count after sweeping.
    pub after: usize,
    /// Number of objects reclaimed.
    pub collected: usize,
    /// Object-count threshold for the next automatic VM collection.
    pub next_gc: usize,
}

#[derive(Debug)]
struct HeapSlot {
    obj: Option<Obj>,
    is_marked: bool,
    generation: u32,
}

impl HeapSlot {
    fn occupied(obj: Obj) -> Self {
        Self {
            obj: Some(obj),
            is_marked: false,
            generation: 0,
        }
    }
}

/// VM heap. Owns every object referenced by a [`Value::Obj`]
/// elsewhere in the runtime.
///
/// [`Value::Obj`]: crate::value::Value::Obj
#[derive(Debug)]
pub struct Heap {
    slots: Vec<HeapSlot>,
    free: Vec<u32>,
    gray_stack: Vec<Handle>,
    live: usize,
    next_gc: usize,
}

impl Default for Heap {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            gray_stack: Vec::new(),
            live: 0,
            next_gc: INITIAL_GC_THRESHOLD,
        }
    }
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

    /// Allocate an [`Obj::BoundMethod`] and return its handle.
    pub fn alloc_bound_method(&mut self, bound_method: BoundMethod) -> Handle {
        self.alloc(Obj::BoundMethod(bound_method))
    }

    /// Allocate an [`Obj::Upvalue`] and return its handle.
    pub fn alloc_upvalue(&mut self, upvalue: Upvalue) -> Handle {
        self.alloc(Obj::Upvalue(upvalue))
    }

    /// Allocate an [`Obj::Native`] and return its handle.
    pub fn alloc_native(&mut self, native: NativeFunction) -> Handle {
        self.alloc(Obj::Native(native))
    }

    /// Allocate an [`Obj::Class`] and return its handle.
    pub fn alloc_class(&mut self, class: Class) -> Handle {
        self.alloc(Obj::Class(class))
    }

    /// Allocate an [`Obj::Instance`] and return its handle.
    pub fn alloc_instance(&mut self, instance: Instance) -> Handle {
        self.alloc(Obj::Instance(instance))
    }

    fn alloc(&mut self, obj: Obj) -> Handle {
        self.live += 1;

        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            debug_assert!(slot.obj.is_none());
            slot.obj = Some(obj);
            slot.is_marked = false;
            return Handle {
                index,
                generation: slot.generation,
            };
        }

        let index =
            u32::try_from(self.slots.len()).expect("heap exceeded u32 capacity (4 billion slots)");
        self.slots.push(HeapSlot::occupied(obj));
        Handle {
            index,
            generation: 0,
        }
    }

    /// Borrow the object referenced by `h`. Panics if `h` is stale
    /// because the object was swept, or invalid because it came from a
    /// different heap.
    #[must_use]
    pub fn get(&self, h: Handle) -> &Obj {
        self.slots[self.valid_slot_index(h)]
            .obj
            .as_ref()
            .unwrap_or_else(|| panic!("stale heap handle {h:?}"))
    }

    /// Mutably borrow the object referenced by `h`.
    #[must_use]
    pub fn get_mut(&mut self, h: Handle) -> &mut Obj {
        let index = self.valid_slot_index(h);
        self.slots[index]
            .obj
            .as_mut()
            .unwrap_or_else(|| panic!("stale heap handle {h:?}"))
    }

    /// Convenience accessor for string objects. Returns `None` if the
    /// handle points at a non-string object.
    #[must_use]
    pub fn as_str(&self, h: Handle) -> Option<&str> {
        match self.get(h) {
            Obj::Str(s) => Some(s),
            Obj::Function(_)
            | Obj::Closure(_)
            | Obj::BoundMethod(_)
            | Obj::Upvalue(_)
            | Obj::Native(_)
            | Obj::Class(_)
            | Obj::Instance(_) => None,
        }
    }

    /// Convenience accessor for function objects.
    #[must_use]
    pub fn as_function(&self, h: Handle) -> Option<&Function> {
        match self.get(h) {
            Obj::Function(function) => Some(function),
            Obj::Str(_)
            | Obj::Closure(_)
            | Obj::BoundMethod(_)
            | Obj::Upvalue(_)
            | Obj::Native(_)
            | Obj::Class(_)
            | Obj::Instance(_) => None,
        }
    }

    /// Convenience accessor for closure objects.
    #[must_use]
    pub fn as_closure(&self, h: Handle) -> Option<&Closure> {
        match self.get(h) {
            Obj::Closure(closure) => Some(closure),
            Obj::Str(_)
            | Obj::Function(_)
            | Obj::BoundMethod(_)
            | Obj::Upvalue(_)
            | Obj::Native(_)
            | Obj::Class(_)
            | Obj::Instance(_) => None,
        }
    }

    /// Mutable convenience accessor for closure objects.
    #[must_use]
    pub fn as_closure_mut(&mut self, h: Handle) -> Option<&mut Closure> {
        match self.get_mut(h) {
            Obj::Closure(closure) => Some(closure),
            Obj::Str(_)
            | Obj::Function(_)
            | Obj::BoundMethod(_)
            | Obj::Upvalue(_)
            | Obj::Native(_)
            | Obj::Class(_)
            | Obj::Instance(_) => None,
        }
    }

    /// Convenience accessor for bound method objects.
    #[must_use]
    pub fn as_bound_method(&self, h: Handle) -> Option<BoundMethod> {
        match self.get(h) {
            Obj::BoundMethod(bound_method) => Some(*bound_method),
            Obj::Str(_)
            | Obj::Function(_)
            | Obj::Closure(_)
            | Obj::Upvalue(_)
            | Obj::Native(_)
            | Obj::Class(_)
            | Obj::Instance(_) => None,
        }
    }

    /// Convenience accessor for upvalue objects.
    #[must_use]
    pub fn as_upvalue(&self, h: Handle) -> Option<Upvalue> {
        match self.get(h) {
            Obj::Upvalue(upvalue) => Some(*upvalue),
            Obj::Str(_)
            | Obj::Function(_)
            | Obj::Closure(_)
            | Obj::BoundMethod(_)
            | Obj::Native(_)
            | Obj::Class(_)
            | Obj::Instance(_) => None,
        }
    }

    /// Mutable convenience accessor for upvalue objects.
    #[must_use]
    pub fn as_upvalue_mut(&mut self, h: Handle) -> Option<&mut Upvalue> {
        match self.get_mut(h) {
            Obj::Upvalue(upvalue) => Some(upvalue),
            Obj::Str(_)
            | Obj::Function(_)
            | Obj::Closure(_)
            | Obj::BoundMethod(_)
            | Obj::Native(_)
            | Obj::Class(_)
            | Obj::Instance(_) => None,
        }
    }

    /// Convenience accessor for native function objects.
    #[must_use]
    pub fn as_native(&self, h: Handle) -> Option<NativeFunction> {
        match self.get(h) {
            Obj::Native(native) => Some(*native),
            Obj::Str(_)
            | Obj::Function(_)
            | Obj::Closure(_)
            | Obj::BoundMethod(_)
            | Obj::Upvalue(_)
            | Obj::Class(_)
            | Obj::Instance(_) => None,
        }
    }

    /// Convenience accessor for class objects.
    #[must_use]
    pub fn as_class(&self, h: Handle) -> Option<&Class> {
        match self.get(h) {
            Obj::Class(class) => Some(class),
            Obj::Str(_)
            | Obj::Function(_)
            | Obj::Closure(_)
            | Obj::BoundMethod(_)
            | Obj::Upvalue(_)
            | Obj::Native(_)
            | Obj::Instance(_) => None,
        }
    }

    /// Mutable convenience accessor for class objects.
    #[must_use]
    pub fn as_class_mut(&mut self, h: Handle) -> Option<&mut Class> {
        match self.get_mut(h) {
            Obj::Class(class) => Some(class),
            Obj::Str(_)
            | Obj::Function(_)
            | Obj::Closure(_)
            | Obj::BoundMethod(_)
            | Obj::Upvalue(_)
            | Obj::Native(_)
            | Obj::Instance(_) => None,
        }
    }

    /// Convenience accessor for instance objects.
    #[must_use]
    pub fn as_instance(&self, h: Handle) -> Option<&Instance> {
        match self.get(h) {
            Obj::Instance(instance) => Some(instance),
            Obj::Str(_)
            | Obj::Function(_)
            | Obj::Closure(_)
            | Obj::BoundMethod(_)
            | Obj::Upvalue(_)
            | Obj::Native(_)
            | Obj::Class(_) => None,
        }
    }

    /// Mutable convenience accessor for instance objects.
    #[must_use]
    pub fn as_instance_mut(&mut self, h: Handle) -> Option<&mut Instance> {
        match self.get_mut(h) {
            Obj::Instance(instance) => Some(instance),
            Obj::Str(_)
            | Obj::Function(_)
            | Obj::Closure(_)
            | Obj::BoundMethod(_)
            | Obj::Upvalue(_)
            | Obj::Native(_)
            | Obj::Class(_) => None,
        }
    }

    /// Number of live objects currently allocated.
    #[must_use]
    pub fn len(&self) -> usize {
        self.live
    }

    /// `true` if no objects are allocated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// `true` if the VM should run a collection at its next safe point.
    #[must_use]
    pub fn needs_collection(&self) -> bool {
        self.live > self.next_gc
    }

    /// Force the next VM allocation safe point to collect.
    ///
    /// This is primarily a diagnostic hook for stress-testing root
    /// handling without exposing the collector's threshold as public
    /// tuning API.
    #[cfg(test)]
    pub(crate) fn force_next_collection(&mut self) {
        self.next_gc = 0;
    }

    /// Run a precise mark-sweep collection from `roots`.
    #[must_use]
    pub fn collect_garbage(&mut self, roots: GcRoots<'_>) -> GcStats {
        let before = self.live;
        self.gray_stack.clear();

        for value in roots.values {
            self.mark_value(*value);
        }
        for handle in roots.handles {
            self.mark_handle(*handle);
        }
        for chunk in roots.chunks {
            self.mark_chunk(chunk);
        }

        self.trace_references();
        self.sweep();
        self.next_gc = self
            .live
            .saturating_mul(GC_HEAP_GROW_FACTOR)
            .max(INITIAL_GC_THRESHOLD);

        GcStats {
            before,
            after: self.live,
            collected: before - self.live,
            next_gc: self.next_gc,
        }
    }

    fn mark_chunk(&mut self, chunk: &Chunk) {
        for value in &chunk.constants {
            self.mark_value(*value);
        }
    }

    fn mark_value(&mut self, value: Value) {
        if let Value::Obj(handle) = value {
            self.mark_handle(handle);
        }
    }

    fn mark_handle(&mut self, handle: Handle) {
        let index = self.valid_slot_index(handle);
        let slot = &mut self.slots[index];
        assert!(slot.obj.is_some(), "stale heap handle {handle:?}");
        if slot.is_marked {
            return;
        }

        slot.is_marked = true;
        self.gray_stack.push(handle);
    }

    fn trace_references(&mut self) {
        while let Some(handle) = self.gray_stack.pop() {
            self.blacken_object(handle);
        }
    }

    fn blacken_object(&mut self, handle: Handle) {
        let (values, handles) = match self.get(handle) {
            Obj::Str(_) | Obj::Native(_) => (Vec::new(), Vec::new()),
            Obj::Function(function) => (function.chunk.constants.clone(), Vec::new()),
            Obj::Closure(closure) => {
                let mut handles = Vec::with_capacity(1 + closure.upvalues.len());
                handles.push(closure.function);
                handles.extend(closure.upvalues.iter().copied());
                (Vec::new(), handles)
            }
            Obj::BoundMethod(bound_method) => {
                (vec![bound_method.receiver], vec![bound_method.method])
            }
            Obj::Upvalue(upvalue) => match upvalue.location {
                UpvalueLocation::Open(_) => (Vec::new(), Vec::new()),
                UpvalueLocation::Closed(value) => (vec![value], Vec::new()),
            },
            Obj::Class(class) => (Vec::new(), class.methods.values().copied().collect()),
            Obj::Instance(instance) => (
                instance.fields.values().copied().collect(),
                vec![instance.class],
            ),
        };

        for value in values {
            self.mark_value(value);
        }
        for handle in handles {
            self.mark_handle(handle);
        }
    }

    fn sweep(&mut self) {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.obj.is_none() {
                continue;
            }

            if slot.is_marked {
                slot.is_marked = false;
                continue;
            }

            slot.obj = None;
            slot.generation = slot
                .generation
                .checked_add(1)
                .expect("heap handle generation overflowed");
            self.live -= 1;
            self.free
                .push(u32::try_from(index).expect("heap slot index exceeded u32 capacity"));
        }
    }

    fn valid_slot_index(&self, handle: Handle) -> usize {
        let slot = self
            .slots
            .get(handle.index as usize)
            .unwrap_or_else(|| panic!("invalid heap handle {handle:?}"));
        assert_eq!(
            slot.generation, handle.generation,
            "stale heap handle {handle:?}"
        );
        handle.index as usize
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
        let class = h.alloc_class(Class::new("Bagel"));
        let instance = h.alloc_instance(Instance::new(class));
        let bound_method = h.alloc_bound_method(BoundMethod::new(Value::Obj(instance), closure));
        let native = h.alloc_native(NativeFunction {
            name: "clock",
            arity: 0,
            function: native,
        });

        assert_eq!(h.as_function(function).unwrap().display_name(), "f");
        assert_eq!(h.as_closure(closure).unwrap().function, function);
        assert_eq!(h.as_bound_method(bound_method).unwrap().method, closure);
        assert_eq!(
            h.as_bound_method(bound_method).unwrap().receiver,
            Value::Obj(instance)
        );
        assert_eq!(
            h.as_upvalue(upvalue).unwrap().location,
            UpvalueLocation::Open(3)
        );
        assert_eq!(h.as_class(class).unwrap().name, "Bagel");
        assert_eq!(h.as_instance(instance).unwrap().class, class);
        assert_eq!(h.as_native(native).unwrap().name, "clock");
        assert_eq!(h.as_str(function), None);
        assert!(h.as_function(native).is_none());
        assert!(h.as_bound_method(instance).is_none());
        assert!(h.as_class(instance).is_none());
    }

    #[test]
    fn collect_garbage_sweeps_unreachable_objects() {
        let mut h = Heap::new();
        let live = h.alloc_string("live");
        let dead = h.alloc_string("dead");

        let stats = h.collect_garbage(GcRoots::values(&[Value::Obj(live)]));

        assert_eq!(
            stats,
            GcStats {
                before: 2,
                after: 1,
                collected: 1,
                next_gc: INITIAL_GC_THRESHOLD,
            }
        );
        assert_eq!(h.as_str(live), Some("live"));
        assert_eq!(h.len(), 1);

        let reused = h.alloc_string("reused");
        assert_ne!(dead, reused);
        assert_eq!(reused.to_string(), dead.to_string());
        assert_eq!(h.as_str(reused), Some("reused"));
    }

    #[test]
    fn collect_garbage_traces_closure_function_and_closed_upvalue() {
        let mut h = Heap::new();
        let captured = h.alloc_string("captured");
        let upvalue = h.alloc_upvalue(Upvalue {
            location: UpvalueLocation::Closed(Value::Obj(captured)),
        });

        let constant = h.alloc_string("constant");
        let mut chunk = Chunk::new();
        chunk.add_constant(Value::Obj(constant));
        let function =
            h.alloc_function(Function::with_upvalues(Some("f".to_string()), 0, 1, chunk));
        let closure = h.alloc_closure(Closure::new(function, vec![upvalue]));
        h.alloc_string("dead");

        let stats = h.collect_garbage(GcRoots {
            values: &[],
            handles: &[closure],
            chunks: &[],
        });

        assert_eq!(stats.before, 6);
        assert_eq!(stats.after, 5);
        assert_eq!(stats.collected, 1);
        assert_eq!(h.as_str(captured), Some("captured"));
        assert_eq!(h.as_str(constant), Some("constant"));
        assert!(h.as_closure(closure).is_some());
        assert!(h.as_function(function).is_some());
        assert!(h.as_upvalue(upvalue).is_some());
    }

    #[test]
    fn collect_garbage_marks_chunk_constants() {
        let mut h = Heap::new();
        let live = h.alloc_string("literal");
        let mut chunk = Chunk::new();
        chunk.add_constant(Value::Obj(live));
        h.alloc_string("dead");

        let stats = h.collect_garbage(GcRoots {
            values: &[],
            handles: &[],
            chunks: &[&chunk],
        });

        assert_eq!(stats.before, 2);
        assert_eq!(stats.after, 1);
        assert_eq!(h.as_str(live), Some("literal"));
    }

    #[test]
    fn collect_garbage_traces_instance_class_and_fields() {
        let mut h = Heap::new();
        let class = h.alloc_class(Class::new("Box"));
        let value = h.alloc_string("field");
        let mut instance = Instance::new(class);
        instance
            .fields
            .insert("payload".to_string(), Value::Obj(value));
        let instance = h.alloc_instance(instance);
        h.alloc_string("dead");

        let stats = h.collect_garbage(GcRoots {
            values: &[Value::Obj(instance)],
            handles: &[],
            chunks: &[],
        });

        assert_eq!(stats.before, 4);
        assert_eq!(stats.after, 3);
        assert_eq!(h.as_class(class).unwrap().name, "Box");
        assert_eq!(h.as_str(value), Some("field"));
        assert!(h.as_instance(instance).is_some());
    }

    #[test]
    fn collect_garbage_traces_class_methods_and_bound_methods() {
        let mut h = Heap::new();
        let function = h.alloc_function(Function::with_upvalues(
            Some("method".to_string()),
            0,
            0,
            Chunk::new(),
        ));
        let closure = h.alloc_closure(Closure::new(function, Vec::new()));
        let mut class = Class::new("Box");
        class.methods.insert("method".to_string(), closure);
        let class = h.alloc_class(class);
        let instance = h.alloc_instance(Instance::new(class));
        let bound_method = h.alloc_bound_method(BoundMethod::new(Value::Obj(instance), closure));
        h.alloc_string("dead");

        let stats = h.collect_garbage(GcRoots {
            values: &[],
            handles: &[class, bound_method],
            chunks: &[],
        });

        assert_eq!(stats.before, 6);
        assert_eq!(stats.after, 5);
        assert_eq!(stats.collected, 1);
        assert!(h.as_function(function).is_some());
        assert!(h.as_closure(closure).is_some());
        assert_eq!(h.as_class(class).unwrap().methods["method"], closure);
        assert!(h.as_instance(instance).is_some());
        assert!(h.as_bound_method(bound_method).is_some());
    }

    #[test]
    #[should_panic(expected = "stale heap handle")]
    fn stale_handle_panics_after_collection() {
        let mut h = Heap::new();
        let dead = h.alloc_string("dead");
        let _ = h.collect_garbage(GcRoots::empty());
        let _ = h.get(dead);
    }
}
