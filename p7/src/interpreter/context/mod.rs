use std::collections::HashMap;

use crate::bytecode::Module;
use crate::errors::RuntimeError;

#[macro_use]
mod data;
mod execution;
mod gc;
mod modules;

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
    /// Concrete `TypeId` of the synthetic `__ForeignCarrier_<F>` struct in
    /// each loaded module. Populated lazily by [`Context::add_module`] as
    /// new modules are loaded. Multiple modules may share the same tag.
    pub carrier_type_ids: Vec<(usize, u32)>, // (module_idx, type_id)
}

pub struct Context {
    pub stack: Vec<StackFrame>,
    modules: Vec<Module>,
    pub heap: Vec<Struct>,
    pub box_heap: Vec<Data>,
    // GC state
    allocation_count: usize,
    gc_threshold: usize,
    // Vtable for dynamic dispatch: (concrete_type_id, proto_id, method_name_hash) -> symbol_id
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
            box_heap: Vec::new(),
            allocation_count: 0,
            gc_threshold: 100, // Run GC after every 100 allocations
            vtable: HashMap::new(),
            host_functions: HashMap::new(),
            imported_modules: HashMap::new(),
            script_dir: None,
            stop_depth: None,
            module_vars: Vec::new(),
            foreign_types: HashMap::new(),
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
        let entry = self.foreign_types
            .entry(type_tag.to_string())
            .or_insert_with(|| ForeignTypeReg {
                finalizer: None,
                carrier_type_ids: Vec::new(),
            });
        entry.finalizer = finalizer.map(str::to_string);
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
        let module_idx = self.stack_frame()?.module_idx;
        let carrier_type_id = self
            .foreign_types
            .get(type_tag)
            .and_then(|reg| {
                reg.carrier_type_ids
                    .iter()
                    .find_map(|(m, t)| if *m == module_idx { Some(*t) } else { None })
                    .or_else(|| reg.carrier_type_ids.first().map(|(_, t)| *t))
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
        let box_idx = self.box_heap.len() as u32;
        self.box_heap.push(payload);
        self.allocation_count += 1;

        self.stack_frame_mut()?
            .stack
            .push(Data::ProtoBoxRef {
                box_idx,
                concrete_type_id: carrier_type_id,
            });
        Ok(())
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
                self.stack_frame_mut()?
                    .stack
                    .push(Data::Some(Box::new(v)));
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

        let (box_idx, _ctid) = match v {
            Data::ProtoBoxRef { box_idx, concrete_type_id } => (box_idx, concrete_type_id),
            Data::ProtoRefRef { ref_idx, concrete_type_id } => (ref_idx, concrete_type_id),
            Data::BoxRef(idx) => (idx, 0),
            other => {
                return Err(RuntimeError::Other(format!(
                    "pop_foreign: expected a foreign-bearing reference, got {:?}",
                    other
                )));
            }
        };

        let payload = self
            .box_heap
            .get(box_idx as usize)
            .ok_or_else(|| RuntimeError::Other("pop_foreign: invalid box index".to_string()))?;

        match payload {
            Data::Foreign { type_tag, handle, .. } => {
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
