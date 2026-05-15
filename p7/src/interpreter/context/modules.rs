use crate::bytecode::{Instruction, Module, ResolvedExternalCall, ResolvedExternalVar};
use crate::errors::RuntimeError;

use super::Context;
use super::ForeignCarrierMethod;
use super::data::{Data, StackFrame};

impl Context {
    pub fn load_module(&mut self, mut module: Module) {
        // Push the main module first to ensure it's at index 0
        let module_idx_for_foreign = self.modules.len();
        self.build_vtable(module_idx_for_foreign, &module);
        self.discover_foreign_carriers(module_idx_for_foreign, &module);

        // Extract imported modules and init address before pushing the main module
        let imported_modules = std::mem::take(&mut module.imported_modules);
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
        self.resolve_external_lookups(module_idx)
            .expect("external lookups should resolve before execution");

        // Run module-level init code if present
        if let Some(addr) = init_address {
            self.run_module_init(module_idx, addr as usize);
        }
    }

    /// Helper to load a module and recursively load its dependencies.
    /// Registers each module in imported_modules if not already present.
    fn load_module_internal(&mut self, mut module: Module) {
        let module_idx_for_foreign = self.modules.len();
        self.build_vtable(module_idx_for_foreign, &module);
        self.discover_foreign_carriers(module_idx_for_foreign, &module);

        // Extract imported modules and init address before pushing this module
        let imported_modules = std::mem::take(&mut module.imported_modules);
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
        self.resolve_external_lookups(module_idx)
            .expect("external lookups should resolve before execution");

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

                // Track every (module, carrier, proto) tuple for diagnostics
                // and so foreign value boxing can pick a registered carrier
                // for an arbitrary frame. With origin_module_idx-keyed vtable
                // dispatch the receiver always resolves in its own module, so
                // no cross-module vtable entries are required here.
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

                    let bucket = self.foreign_carrier_methods.entry(tag).or_default();
                    bucket.extend(new_rows);
                }
            }
        }
    }

    fn resolve_external_lookups(&mut self, module_idx: usize) -> Result<(), RuntimeError> {
        let decoded_len = self.modules[module_idx].decoded_len();
        for inst_index in 0..decoded_len {
            let instruction = self.modules[module_idx]
                .decoded_instruction(inst_index)
                .cloned()
                .ok_or_else(|| {
                    RuntimeError::Other(format!(
                        "instruction index {} out of bounds in module {}",
                        inst_index, module_idx
                    ))
                })?;

            match instruction {
                Instruction::LdExtModVar(module_path_sid, var_name_sid)
                | Instruction::StExtModVar(module_path_sid, var_name_sid) => {
                    let module_path =
                        self.string_constant(module_idx, module_path_sid, "module path")?;
                    let var_name = self.string_constant(module_idx, var_name_sid, "var name")?;
                    let target_module_idx =
                        *self.imported_modules.get(&module_path).ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Module '{}' not found in imported modules",
                                module_path
                            ))
                        })?;
                    let var_id = self.modules[target_module_idx]
                        .module_variable_by_name(&var_name, false)
                        .map(|v| v.var_id)
                        .ok_or_else(|| {
                            RuntimeError::VariableNotFound(format!(
                                "Module variable '{}' not found in module '{}'",
                                var_name, module_path
                            ))
                        })?;
                    self.modules[module_idx].external_var_targets[inst_index] =
                        Some(ResolvedExternalVar {
                            module_idx: target_module_idx,
                            var_id,
                        });
                }
                Instruction::CallExternal(module_path_sid, symbol_name_sid) => {
                    let module_path =
                        self.string_constant(module_idx, module_path_sid, "module path")?;
                    let symbol_name =
                        self.string_constant(module_idx, symbol_name_sid, "symbol name")?;
                    let target_module_idx =
                        *self.imported_modules.get(&module_path).ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Module '{}' not found in imported modules",
                                module_path
                            ))
                        })?;
                    let symbol_id = self.resolve_external_symbol_id(
                        target_module_idx,
                        &module_path,
                        &symbol_name,
                    )?;
                    let crate::bytecode::ResolvedDispatch::Function {
                        target_pc,
                        args_len,
                    } = self.modules[target_module_idx]
                        .symbol_dispatch(symbol_id)
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Symbol '{}' in module '{}' is not a function",
                                symbol_name, module_path
                            ))
                        })?
                    else {
                        return Err(RuntimeError::Other(format!(
                            "Symbol '{}' in module '{}' is not a function",
                            symbol_name, module_path
                        )));
                    };
                    self.modules[module_idx].external_call_targets[inst_index] =
                        Some(ResolvedExternalCall {
                            module_idx: target_module_idx,
                            target_pc: *target_pc,
                            args_len: *args_len,
                        });
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn string_constant(
        &self,
        module_idx: usize,
        string_id: u32,
        purpose: &str,
    ) -> Result<String, RuntimeError> {
        self.modules[module_idx]
            .string_constants
            .get(string_id as usize)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::Other(format!(
                    "Invalid string constant index for {}: {}",
                    purpose, string_id
                ))
            })
    }

    fn resolve_external_symbol_id(
        &self,
        target_module_idx: usize,
        module_path: &str,
        symbol_name: &str,
    ) -> Result<u32, RuntimeError> {
        let module = &self.modules[target_module_idx];
        let root_symbol = module.symbols.first().ok_or_else(|| {
            RuntimeError::Other(format!("Module '{}' has no root symbol", module_path))
        })?;

        if let Some((type_name, method_name)) = symbol_name.split_once('.') {
            let type_sym_id = root_symbol.children.get(type_name).ok_or_else(|| {
                RuntimeError::Other(format!(
                    "Type '{}' not found in module '{}'",
                    type_name, module_path
                ))
            })?;
            let type_sym = module.symbols.get(*type_sym_id as usize).ok_or_else(|| {
                RuntimeError::Other(format!("Invalid symbol id: {}", type_sym_id))
            })?;
            let method_sym_id = type_sym.children.get(method_name).ok_or_else(|| {
                RuntimeError::Other(format!(
                    "Method '{}' not found on type '{}' in module '{}'",
                    method_name, type_name, module_path
                ))
            })?;
            Ok(*method_sym_id)
        } else {
            root_symbol.children.get(symbol_name).copied().ok_or_else(|| {
                RuntimeError::Other(format!(
                    "Symbol '{}' not found in module '{}'",
                    symbol_name, module_path
                ))
            })
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
    /// Build vtable for dynamic dispatch by mapping
    /// (origin_module_idx, concrete_type_id, method_name) -> symbol_id.
    fn build_vtable(&mut self, module_idx: usize, module: &Module) {
        use crate::semantic::{SymbolKind, TypeDefinition};

        let type_symbols: std::collections::HashMap<u32, _> = module
            .symbols
            .iter()
            .filter_map(|symbol| match symbol.kind {
                SymbolKind::Type(type_id) => Some((type_id, symbol)),
                _ => None,
            })
            .collect();

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
                            if let Some(struct_symbol) = type_symbols.get(&struct_type_id) {
                                // Look for the method in the struct's children
                                if let Some(&method_symbol_id) = struct_symbol.children.get(method_name) {
                                    // Hash the method name for fast lookup
                                    let method_hash = Self::hash_method_name(method_name);

                                    self.vtable.insert(
                                        (module_idx as u32, struct_type_id, method_hash),
                                        method_symbol_id,
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
