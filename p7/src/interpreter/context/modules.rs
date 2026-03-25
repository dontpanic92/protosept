use crate::bytecode::Module;
use crate::errors::RuntimeError;

use super::Context;
use super::data::{ContextResult, Data, StackFrame};

impl Context {
    pub fn load_module(&mut self, module: Module) {
        // Push the main module first to ensure it's at index 0
        self.build_vtable(&module);

        // Extract imported modules and init address before pushing the main module
        let imported_modules = module.imported_modules.clone();
        let init_address = module.module_init_address;
        // Allocate module-level variable storage
        let var_count = module.module_var_count as usize;
        let module_idx = self.modules.len();
        self.modules.push(module);
        self.module_vars.push(vec![Data::Int(0); var_count]);

        // Now register and load all imported modules
        for (module_path, imported) in imported_modules {
            let imported_module_idx = self.modules.len();
            self.imported_modules
                .insert(module_path.clone(), imported_module_idx);
            self.load_module_internal(*imported);
        }

        // Run module-level init code if present
        if let Some(addr) = init_address {
            self.run_module_init(module_idx, addr as usize);
        }
    }

    /// Helper to load a module and recursively load its dependencies.
    /// Registers each module in imported_modules if not already present.
    fn load_module_internal(&mut self, module: Module) {
        self.build_vtable(&module);

        // Extract imported modules and init address before pushing this module
        let imported_modules = module.imported_modules.clone();
        let init_address = module.module_init_address;
        // Allocate module-level variable storage
        let var_count = module.module_var_count as usize;
        let module_idx = self.modules.len();
        self.modules.push(module);
        self.module_vars.push(vec![Data::Int(0); var_count]);

        // Register imported modules of this module
        for (module_path, imported) in imported_modules {
            if !self.imported_modules.contains_key(&module_path) {
                let idx = self.modules.len();
                self.imported_modules
                    .insert(module_path.clone(), idx);
                self.load_module_internal(*imported);
            }
        }

        // Run module-level init code if present
        if let Some(addr) = init_address {
            self.run_module_init(module_idx, addr as usize);
        }
    }

    /// Run module-level initialization code (initializes module-level bindings).
    fn run_module_init(&mut self, module_idx: usize, init_address: usize) {
        let mut init_frame = StackFrame::new();
        init_frame.pc = init_address;
        init_frame.module_idx = module_idx;
        self.stack.push(init_frame);
        // Run the init code; it ends with a Ret instruction
        let _ = self.run_interpreter_loop();
        // Pop the init frame
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    /// Build vtable for dynamic dispatch by mapping (concrete_type_id, proto_id, method_name) -> symbol_id
    fn build_vtable(&mut self, module: &Module) {
        use crate::semantic::TypeDefinition;

        // Iterate through all structs
        for (type_id, udt) in module.types.iter().enumerate() {
            if let TypeDefinition::Struct(struct_def) = udt {
                let struct_type_id = type_id as u32;

                // For each protocol this struct conforms to
                for &proto_id in &struct_def.conforming_to {
                    // Get the proto definition
                    if let Some(TypeDefinition::Proto(proto)) = module.types.get(proto_id as usize)
                    {
                        // For each method in the proto
                        for (method_name, _, _) in &proto.methods {
                            // Find the struct's symbol and look for this method
                            if let Some(struct_symbol) = module.symbols.iter()
                                    .find(|s| matches!(&s.kind, crate::semantic::SymbolKind::Type(id) if *id == struct_type_id))
                                {
                                // Look for the method in the struct's children
                                if let Some(&method_symbol_id) = struct_symbol.children.get(method_name) {
                                    // Hash the method name for fast lookup
                                    let method_hash = Self::hash_method_name(method_name);

                                    // Store in vtable: (struct_type_id, proto_id, method_hash) -> method_symbol_id
                                    self.vtable.insert(
                                        (struct_type_id, proto_id, method_hash),
                                        method_symbol_id
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Simple hash function for method names
    pub(super) fn hash_method_name(name: &str) -> u32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        hasher.finish() as u32
    }

    /// Get method name from hash for error messages (reverse lookup)
    pub(super) fn get_method_name_from_hash(&self, proto_id: u32, method_hash: u32) -> ContextResult<String> {
        use crate::semantic::TypeDefinition;

        if let Some(TypeDefinition::Proto(proto)) = self.modules[0].types.get(proto_id as usize) {
            for (method_name, _, _) in &proto.methods {
                if Self::hash_method_name(method_name) == method_hash {
                    return Ok(method_name.to_string());
                }
            }
        }
        Err(RuntimeError::Other(format!(
            "Method with hash {} not found in proto {}",
            method_hash, proto_id
        )))
    }
}
