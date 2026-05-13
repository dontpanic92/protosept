use crate::bytecode::Module;

use super::Context;
use super::ForeignCarrierMethod;
use super::data::{Data, StackFrame};

impl Context {
    pub fn load_module(&mut self, mut module: Module) {
        // Push the main module first to ensure it's at index 0
        self.build_vtable(&module);
        let module_idx_for_foreign = self.modules.len();
        self.discover_foreign_carriers(module_idx_for_foreign, &module);

        // Extract imported modules and init address before pushing the main module
        let imported_modules = module.imported_modules.clone();
        let init_address = module.module_init_address;
        module
            .prepare_execution()
            .expect("module bytecode should decode before execution");
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
    fn load_module_internal(&mut self, mut module: Module) {
        self.build_vtable(&module);
        let module_idx_for_foreign = self.modules.len();
        self.discover_foreign_carriers(module_idx_for_foreign, &module);

        // Extract imported modules and init address before pushing this module
        let imported_modules = module.imported_modules.clone();
        let init_address = module.module_init_address;
        module
            .prepare_execution()
            .expect("module bytecode should decode before execution");
        // Allocate module-level variable storage
        let var_count = module.module_var_count as usize;
        let module_idx = self.modules.len();
        self.modules.push(module);
        self.module_vars.push(vec![Data::Int(0); var_count]);

        // Register imported modules of this module
        for (module_path, imported) in imported_modules {
            if !self.imported_modules.contains_key(&module_path) {
                let idx = self.modules.len();
                self.imported_modules.insert(module_path.clone(), idx);
                self.load_module_internal(*imported);
            }
        }

        // Run module-level init code if present
        if let Some(addr) = init_address {
            self.run_module_init(module_idx, addr as usize);
        }
    }

    /// Walk a freshly-built module's symbols looking for `HostMethod`
    /// children of synthetic `__ForeignCarrier_*` structs. For each
    /// distinct `type_tag` discovered, record the carrier's `TypeId` so
    /// `Context::push_foreign` can stamp it onto new foreign boxes.
    /// Also registers the proto's declared finalizer, if any, and stashes
    /// the proto's UUID in `foreign_uuids` keyed by tag.
    fn discover_foreign_carriers(&mut self, module_idx: usize, module: &Module) {
        use crate::semantic::{SymbolKind, TypeDefinition};
        for sym in &module.symbols {
            if let SymbolKind::Type(carrier_type_id) = sym.kind {
                let Some(TypeDefinition::Struct(struct_def)) =
                    module.types.get(carrier_type_id as usize)
                else {
                    continue;
                };
                let Some(&proto_id) = struct_def.conforming_to.first() else {
                    continue;
                };
                let Some(TypeDefinition::Proto(proto)) = module.types.get(proto_id as usize) else {
                    continue;
                };
                let Some(type_tag) = &proto.foreign_type_tag else {
                    continue;
                };

                let tag = type_tag.to_string();
                let entry = self.foreign_types.entry(tag.clone()).or_insert_with(|| {
                    super::ForeignTypeReg {
                        finalizer: None,
                        host_registered: false,
                        carrier_type_ids: Vec::new(),
                    }
                });
                let pair = (module_idx, carrier_type_id);
                if !entry.carrier_type_ids.contains(&pair) {
                    entry.carrier_type_ids.push(pair);
                }
                if !entry.host_registered && entry.finalizer.is_none() {
                    entry.finalizer = proto.foreign_finalizer.as_ref().map(ToString::to_string);
                }
                if let Some(uuid_str) = &proto.foreign_uuid {
                    self.foreign_uuids
                        .entry(tag.clone())
                        .or_insert_with(|| uuid_str.to_string());
                }

                // The HostMethod children are unique to carrier structs.
                let mut method_rows: Vec<(u32, u32)> = Vec::new(); // (method_hash, method_symbol_id)
                for (child_name, &child_id) in sym.children.iter() {
                    if let Some(child) = module.symbols.get(child_id as usize)
                        && let SymbolKind::HostMethod { type_tag, .. } = &child.kind
                    {
                        debug_assert_eq!(type_tag.as_str(), tag.as_str());
                        method_rows.push((Self::hash_method_name(child_name.as_str()), child_id));
                    }
                }

                // Maintain the per-type-tag method index and emit cross-module
                // vtable entries so that a foreign value stamped with another
                // module's carrier_type_id still dispatches correctly when its
                // method is called from this module (and vice versa).
                {
                    let new_rows: Vec<ForeignCarrierMethod> = method_rows
                        .iter()
                        .map(|&(method_hash, method_symbol_id)| ForeignCarrierMethod {
                            _module_idx: module_idx,
                            carrier_type_id,
                            proto_type_id: proto_id,
                            method_hash,
                            method_symbol_id,
                        })
                        .collect();

                    let prior_rows: Vec<ForeignCarrierMethod> = self
                        .foreign_carrier_methods
                        .get(&tag)
                        .cloned()
                        .unwrap_or_default();

                    // Cross-emit vtable entries between every prior row and
                    // every new row sharing the same method_hash.
                    for new_row in &new_rows {
                        for prior in &prior_rows {
                            if prior.method_hash != new_row.method_hash {
                                continue;
                            }
                            // foreign value stamped with the prior carrier
                            // dispatches in the new module's context using
                            // the new module's symbol
                            self.vtable.insert(
                                (
                                    prior.carrier_type_id,
                                    new_row.proto_type_id,
                                    new_row.method_hash,
                                ),
                                new_row.method_symbol_id,
                            );
                            // and the reverse: foreign value stamped with
                            // the new carrier dispatches in the prior
                            // module's context using the prior module's
                            // symbol
                            self.vtable.insert(
                                (
                                    new_row.carrier_type_id,
                                    prior.proto_type_id,
                                    prior.method_hash,
                                ),
                                prior.method_symbol_id,
                            );
                        }
                    }

                    let bucket = self.foreign_carrier_methods.entry(tag).or_default();
                    bucket.extend(new_rows);
                }
            }
        }
    }

    /// Run module-level initialization code (initializes module-level bindings).
    fn run_module_init(&mut self, module_idx: usize, init_address: usize) {
        let mut init_frame = StackFrame::new();
        init_frame.pc = self
            .modules
            .get(module_idx)
            .and_then(|module| module.bytecode_address_to_instruction_index(init_address as u32))
            .expect("module init address should point to a decoded instruction");
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
        crate::bytecode::hash_method_name(name)
    }
}
