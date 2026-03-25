use std::collections::HashMap;

use crate::bytecode::Module;

#[macro_use]
mod data;
mod execution;
mod gc;
mod modules;

pub use data::*;

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
}
