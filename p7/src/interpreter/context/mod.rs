use std::collections::HashMap;

use crate::bytecode::Module;
use crate::errors::RuntimeError;

#[macro_use]
mod data;
mod box_heap;
mod execution;
pub use execution::encode_return_ty;
mod gc;
mod modules;

pub use box_heap::BoxHeap;
pub use data::*;

/// Registration metadata for an `@foreign` proto. Created via
/// [`Context::register_foreign_type`] before any module that uses the proto
/// is loaded. The dispatcher itself is registered as an ordinary host
/// function with `register_host_function`; this struct only tracks the
/// per-type-tag finalizer and the carrier `type_id`.
#[derive(Debug, Clone)]
pub(super) struct ForeignTypeReg {
    /// Optional finalizer host fn name. Called once with the handle when an
    /// owned `box<F>` is collected. `None` means no finalizer (e.g. for
    /// resources whose Rust-side owner outlives the script).
    pub finalizer: Option<String>,
    /// True when the host explicitly called `register_foreign_type`.
    /// Declaration discovery fills unset metadata but must not overwrite
    /// host-provided finalizer choices, including an explicit `None`.
    pub host_registered: bool,
    /// Concrete `TypeId` of the synthetic `__ForeignCarrier_<F>` struct in
    /// each loaded module. Populated lazily by [`Context::add_module`] as
    /// new modules are loaded. Multiple modules may share the same tag.
    pub carrier_type_ids: Vec<(usize, u32)>, // (module_idx, type_id)
}

pub struct Context {
    pub stack: Vec<StackFrame>,
    modules: Vec<Module>,
    pub heap: Vec<Struct>,
    pub box_heap: BoxHeap,
    // GC state
    allocation_count: usize,
    gc_threshold: usize,
    // Vtable for dynamic dispatch: (origin_module_idx, concrete_type_id, method_name_hash) -> symbol_id.
    // `origin_module_idx` is the module that defined `concrete_type_id`; both numeric ids are
    // meaningful only within that module, so the key must include the module to avoid collisions
    // across sibling modules.
    vtable: HashMap<(u32, u32, u32), u32>,
    // Host function registry: function_name -> host function
    host_functions: HashMap<String, HostFunction>,
    // Imported modules registry: module_path -> module_index in modules Vec
    imported_modules: HashMap<String, usize>,
    // Optional containing directory of the entry script (filesystem-only)
    script_dir: Option<String>,
    // When set, the interpreter loop stops once stack depth drops to this level.
    // Used by call_closure to run a single closure invocation from a host function.
    stop_depth: Option<usize>,
    // Module-level variables (thread-local): indexed by [module_idx][var_id]
    module_vars: Vec<Vec<Data>>,
    // Registry of @foreign protos by type_tag, populated via
    // `register_foreign_type` and consulted by `push_foreign` / GC.
    pub(super) foreign_types: HashMap<String, ForeignTypeReg>,
    // type_tag -> UUID string. Populated alongside foreign_types when a
    // module is loaded; surfaced via [`Context::foreign_uuid`] so the
    // host dispatcher can `query_interface` by UUID for a given foreign
    // proto without re-walking the module.
    pub(super) foreign_uuids: HashMap<String, String>,
    // Per type_tag bookkeeping of every carrier+method symbol discovered in
    // every loaded module. Used to populate cross-module vtable entries when
    // a foreign value stamped by one module flows to another. See
    // `discover_foreign_carriers` for the maintenance logic.
    pub(super) foreign_carrier_methods: HashMap<String, Vec<ForeignCarrierMethod>>,
    // Host-owned values that must survive GC compaction. These are used by
    // embedding runtimes that keep script state between calls.
    external_roots: Vec<Option<Data>>,
    // Lazy cache resolving an (importing_module_idx, local_type_id) pair to the
    // (defining_module_idx, defining_local_type_id) where the type's vtable
    // actually lives. Populated on demand by `resolve_concrete_origin` when a
    // BoxToProto/RefToProto/SAM site boxes a type imported from another module.
    // See `gaps.md` #1: without this, proto boxes stamp the importer's module
    // and local type id, which has no vtable entry and dispatches wrong.
    imported_type_origin: HashMap<(u32, u32), (u32, u32)>,
}

/// One row in `Context::foreign_carrier_methods`: a single proto-method
/// HostMethod symbol in a specific loaded module, along with the carrier and
/// proto type ids needed to key the vtable.
// Retained as diagnostic bookkeeping for foreign carrier/proto dispatch; the
// fields are populated but not yet read back, so silence dead-code here.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) struct ForeignCarrierMethod {
    pub _module_idx: usize,
    pub carrier_type_id: u32,
    pub proto_type_id: u32,
    pub method_hash: u32,
    pub method_symbol_id: u32,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    pub fn new() -> Self {
        let mut ctx = Self {
            stack: vec![StackFrame::new()],
            modules: Vec::new(),
            heap: Vec::new(),
            box_heap: BoxHeap::new(),
            allocation_count: 0,
            gc_threshold: 100, // Run GC after every 100 allocations
            vtable: HashMap::new(),
            host_functions: HashMap::new(),
            imported_modules: HashMap::new(),
            script_dir: None,
            stop_depth: None,
            module_vars: Vec::new(),
            foreign_types: HashMap::new(),
            foreign_uuids: HashMap::new(),
            foreign_carrier_methods: HashMap::new(),
            external_roots: Vec::new(),
            imported_type_origin: HashMap::new(),
        };

        // Register builtin host functions
        ctx.register_builtin_host_functions();
        ctx
    }

    /// Set the containing directory of the entry script (filesystem-only).
    /// When set, `__script_dir__` evaluates to `Some(dir)` at runtime.
    pub fn set_script_dir(&mut self, dir: Option<String>) {
        self.script_dir = dir;
    }

    /// Get the script directory, if set.
    pub fn script_dir(&self) -> Option<&str> {
        self.script_dir.as_deref()
    }

    pub fn add_external_root(&mut self, data: Data) -> usize {
        if let Some((idx, slot)) = self
            .external_roots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
        {
            *slot = Some(data);
            idx
        } else {
            self.external_roots.push(Some(data));
            self.external_roots.len() - 1
        }
    }

    pub fn set_external_root(&mut self, idx: usize, data: Data) {
        if idx >= self.external_roots.len() {
            self.external_roots.resize_with(idx + 1, || None);
        }
        self.external_roots[idx] = Some(data);
    }

    pub fn external_root(&self, idx: usize) -> Option<Data> {
        self.external_roots.get(idx).and_then(Clone::clone)
    }

    pub fn remove_external_root(&mut self, idx: usize) {
        if let Some(slot) = self.external_roots.get_mut(idx) {
            *slot = None;
        }
    }

    /// Register all builtin host functions
    fn register_builtin_host_functions(&mut self) {
        super::builtin::register_builtin_functions(self);
        super::std_impl::register_std_functions(self);
    }

    /// Register a custom host function
    pub fn register_host_function(&mut self, name: String, func: HostFunction) {
        self.host_functions.insert(name, func);
    }

    /// Register an `@foreign` proto's runtime metadata.
    ///
    /// `type_tag` must match the `@foreign(type_tag="...")` value declared
    /// on the proto. `finalizer`, when provided, is the name of an already-
    /// registered host function (via [`Context::register_host_function`])
    /// that will be invoked once with the handle as a `Data::Int` argument
    /// when an owned `box<F>` is collected.
    ///
    /// The dispatcher itself is just an ordinary host function — register
    /// it via [`Context::register_host_function`] under the name declared
    /// in the proto's `@foreign(dispatcher="...")` clause.
    ///
    /// Calling this method twice for the same `type_tag` overwrites any
    /// previously-registered finalizer but preserves the discovered carrier
    /// type ids.
    pub fn register_foreign_type(&mut self, type_tag: &str, finalizer: Option<&str>) {
        let entry = self
            .foreign_types
            .entry(type_tag.to_string())
            .or_insert_with(|| ForeignTypeReg {
                finalizer: None,
                host_registered: false,
                carrier_type_ids: Vec::new(),
            });
        entry.finalizer = finalizer.map(str::to_string);
        entry.host_registered = true;
    }

    /// Look up the COM-style UUID associated with a `@foreign(uuid="...")`
    /// proto, keyed by its `type_tag`. Returns the UUID string verbatim
    /// (lowercase, hyphenated, 36 chars). The host dispatcher uses this
    /// to `query_interface` from the type-erased ComObjectTable handle to
    /// the right interface pointer.
    pub fn foreign_uuid(&self, type_tag: &str) -> Option<&str> {
        self.foreign_uuids.get(type_tag).map(String::as_str)
    }

    /// Returns the `type_tag` of the first `@foreign` proto that the
    /// struct `(module_idx, type_id)` lists in its conformance bracket
    /// (i.e. `struct[F] X(...)` with `F` carrying `@foreign(type_tag=...)`).
    /// Used by the crosscom dispatcher to wrap script-impl-of-foreign-proto
    /// values into Rust-side CCWs when they cross the C-ABI boundary as
    /// `box<F>` arguments to host methods.
    ///
    /// Returns `None` if `(module_idx, type_id)` is out of range, names a
    /// non-struct type, or names a struct that conforms to no foreign
    /// protos.
    pub fn struct_first_foreign_proto_tag(&self, module_idx: usize, type_id: u32) -> Option<&str> {
        use crate::semantic::TypeDefinition;
        let module = self.modules.get(module_idx)?;
        let TypeDefinition::Struct(s) = module.types.get(type_id as usize)? else {
            return None;
        };
        for &proto_id in &s.conforming_to {
            if let Some(TypeDefinition::Proto(p)) = module.types.get(proto_id as usize)
                && let Some(tag) = &p.foreign_type_tag
            {
                return Some(tag.as_str());
            }
        }
        None
    }

    /// All foreign-tagged proto type_tags that the struct
    /// `(module_idx, type_id)` conforms to, in declaration order.
    /// Used by the crosscom dispatcher to pick the most-derived
    /// interface UUID when reverse-wrapping a script-side
    /// `box<F>` that the script struct may also conform to
    /// derived interfaces of.
    pub fn struct_foreign_proto_tags(&self, module_idx: usize, type_id: u32) -> Vec<&str> {
        use crate::semantic::TypeDefinition;
        let mut out = Vec::new();
        let Some(module) = self.modules.get(module_idx) else {
            return out;
        };
        let Some(TypeDefinition::Struct(s)) = module.types.get(type_id as usize) else {
            return out;
        };
        for &proto_id in &s.conforming_to {
            if let Some(TypeDefinition::Proto(p)) = module.types.get(proto_id as usize)
                && let Some(tag) = &p.foreign_type_tag
            {
                out.push(tag.as_str());
            }
        }
        out
    }

    /// Push an owning foreign value onto the current stack frame.
    ///
    /// The runtime allocates a box on `box_heap` containing
    /// `Data::Foreign { type_tag, handle, owned: true }` and stamps it with
    /// the carrier struct's concrete type id so that `CallProtoMethod` can
    /// dispatch to the proto's host methods. Returns an error if the
    /// `type_tag` has no registered foreign type or no module has provided
    /// a carrier for it yet.
    ///
    /// Use [`Context::push_foreign_ref`] for borrowed (`ref<F>`) values
    /// where the host retains ownership and no finalizer should fire.
    pub fn push_foreign(&mut self, type_tag: &str, handle: i64) -> ContextResult<()> {
        self.push_foreign_inner(type_tag, handle, true)
    }

    /// Push a borrowed foreign value (`ref<F>` semantics). The finalizer
    /// will not fire for this value when it is collected.
    pub fn push_foreign_ref(&mut self, type_tag: &str, handle: i64) -> ContextResult<()> {
        self.push_foreign_inner(type_tag, handle, false)
    }

    fn push_foreign_inner(
        &mut self,
        type_tag: &str,
        handle: i64,
        owned: bool,
    ) -> ContextResult<()> {
        let data = self.alloc_foreign_inner(type_tag, handle, owned)?;
        self.stack_frame_mut()?.stack.push(data);
        Ok(())
    }

    /// Construct a `Data::ProtoBoxRef` wrapping a foreign value without
    /// pushing it onto the active stack frame. Used by hosts that need
    /// to materialise a `box<F>` value *before* calling
    /// [`Context::push_function`] or [`Context::push_proto_method`] —
    /// e.g. to pass `box<IUiHost>` as the first argument to a script's
    /// `render(self, ui: box<IUiHost>, dt: float)`.
    ///
    /// When invoked outside any active frame, the carrier type id is
    /// resolved from the first module that registered a carrier for
    /// `type_tag` (the entry-point module is the typical site, since
    /// `@foreign` protos are declared once per module).
    pub fn alloc_foreign(&mut self, type_tag: &str, handle: i64) -> ContextResult<Data> {
        self.alloc_foreign_inner(type_tag, handle, true)
    }

    /// Like [`Context::alloc_foreign`], but marks the foreign payload
    /// as borrowed (`owned: false`), suppressing the carrier's
    /// finalizer when the box is reclaimed by GC.
    pub fn alloc_foreign_ref(&mut self, type_tag: &str, handle: i64) -> ContextResult<Data> {
        self.alloc_foreign_inner(type_tag, handle, false)
    }

    fn alloc_foreign_inner(
        &mut self,
        type_tag: &str,
        handle: i64,
        owned: bool,
    ) -> ContextResult<Data> {
        // When there is no active frame (e.g. host is preparing args
        // before push_function), fall back to the first registered
        // carrier. With an active frame, prefer the carrier that lives
        // in the calling frame's module so that dispatch keys back to
        // it.
        let module_idx = self.stack.last().map(|f| f.module_idx);
        let (carrier_module_idx, carrier_type_id) = self
            .foreign_types
            .get(type_tag)
            .and_then(|reg| {
                if let Some(midx) = module_idx {
                    reg.carrier_type_ids
                        .iter()
                        .find(|(m, _)| *m == midx)
                        .copied()
                        .or_else(|| reg.carrier_type_ids.first().copied())
                } else {
                    reg.carrier_type_ids.first().copied()
                }
            })
            .ok_or_else(|| {
                RuntimeError::Other(format!(
                    "No @foreign carrier registered for type_tag '{}'. \
                     Did you call Context::register_foreign_type and load \
                     the module declaring the proto?",
                    type_tag
                ))
            })?;

        let payload = Data::Foreign {
            type_tag: type_tag.to_string(),
            handle,
            owned,
        };
        let (box_idx, generation) = self.box_heap.alloc(payload);
        self.allocation_count += 1;

        Ok(Data::ProtoBoxRef {
            box_idx,
            generation,
            concrete_type_id: carrier_type_id,
            origin_module_idx: carrier_module_idx as u32,
        })
    }

    /// Push `Some(box<F>)` or `Null` based on an `Option<i64>` handle, for
    /// dispatcher implementations returning `?box<F>`.
    pub fn push_foreign_optional(
        &mut self,
        type_tag: &str,
        handle: Option<i64>,
    ) -> ContextResult<()> {
        match handle {
            Some(h) => {
                self.push_foreign(type_tag, h)?;
                let v = self
                    .stack_frame_mut()?
                    .stack
                    .pop()
                    .ok_or(RuntimeError::StackUnderflow)?;
                self.stack_frame_mut()?.stack.push(Data::some(v));
                Ok(())
            }
            None => {
                self.stack_frame_mut()?.stack.push(Data::Null);
                Ok(())
            }
        }
    }

    /// Pop a foreign value from the stack and return its handle.
    ///
    /// Verifies that the popped value is a foreign cell whose `type_tag`
    /// matches `expected_tag`. Designed for dispatcher implementations
    /// that need to recover the underlying host object after popping any
    /// preceding method arguments.
    pub fn pop_foreign(&mut self, expected_tag: &str) -> ContextResult<i64> {
        let v = self
            .stack_frame_mut()?
            .stack
            .pop()
            .ok_or(RuntimeError::StackUnderflow)?;

        // The receiver is a box-bearing reference; either a ProtoBoxRef
        // (the foreign carrier wrapping the host handle), a ProtoRefRef
        // (a ref-typed view onto the same), or a raw BoxRef (untyped
        // box). All three carry a stable `(idx, generation)` pair we use to
        // dereference into the box heap below.
        let (box_idx, generation, _ctid) = match v {
            Data::ProtoBoxRef {
                box_idx,
                generation,
                concrete_type_id,
                ..
            } => (box_idx, generation, concrete_type_id),
            Data::ProtoRefRef {
                ref_idx,
                generation,
                concrete_type_id,
                ..
            } => (ref_idx, generation, concrete_type_id),
            Data::BoxRef { idx, generation } => (idx, generation, 0),
            other => {
                return Err(RuntimeError::Other(format!(
                    "pop_foreign: expected a foreign-bearing reference, got {:?}",
                    other
                )));
            }
        };

        let payload = self.box_heap.get(box_idx, generation)?;

        match payload {
            Data::Foreign {
                type_tag, handle, ..
            } => {
                if type_tag != expected_tag {
                    return Err(RuntimeError::Other(format!(
                        "pop_foreign: expected type_tag '{}', got '{}'",
                        expected_tag, type_tag
                    )));
                }
                Ok(*handle)
            }
            other => Err(RuntimeError::Other(format!(
                "pop_foreign: box did not contain a Foreign value, got {:?}",
                other
            ))),
        }
    }
}
