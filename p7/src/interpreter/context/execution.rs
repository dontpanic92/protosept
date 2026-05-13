use crate::bytecode::{Instruction, ResolvedDispatch};
use crate::errors::RuntimeError;

use super::Context;
use super::data::{ContextResult, Data, StackFrame, Struct};

/// Encode a `HostReturnTy` as a `Data::Array`-tagged tree so the host
/// dispatcher can pop+decode it without a custom serialization format.
/// Encoding (`first element of array` is the variant tag):
///   0 = Void, 1 = Int, 2 = Float, 3 = String,
///   4 = Foreign(type_tag),  5 = Optional(inner),  6 = Array(inner)
pub fn encode_return_ty(rt: &crate::semantic::HostReturnTy) -> Data {
    use crate::semantic::HostReturnTy as H;
    let arr = match rt {
        H::Void => vec![Data::Int(0)],
        H::Int => vec![Data::Int(1)],
        H::Float => vec![Data::Int(2)],
        H::String => vec![Data::Int(3)],
        H::Foreign { type_tag } => {
            vec![Data::Int(4), Data::string(type_tag)]
        }
        H::Optional(inner) => vec![Data::Int(5), encode_return_ty(inner)],
        H::Array(inner) => vec![Data::Int(6), encode_return_ty(inner)],
    };
    Data::array(arr)
}

impl Context {
    pub fn push_function(&mut self, name: &str, params: Vec<Data>) {
        if self.modules.is_empty() {
            panic!();
        }

        let addr = self.modules[0]
            .get_function(name)
            .unwrap()
            .get_function_address()
            .unwrap();

        let mut stack_frame = StackFrame::new();
        stack_frame.params = params;
        stack_frame.pc = self.modules[0]
            .bytecode_address_to_instruction_index(addr)
            .expect("function address should point to a decoded instruction");

        self.stack.push(stack_frame);
    }

    /// Returns true if the entry-point module (modules[0]) defines a
    /// function named `name`. Hosts use this to gate optional script
    /// callbacks (e.g. `activate`) without provoking the
    /// `push_function` panic on missing names.
    pub fn has_function(&self, name: &str) -> bool {
        self.modules
            .first()
            .and_then(|m| m.get_function(name))
            .is_some()
    }

    /// Push a script-defined proto method call so the host can invoke a method
    /// on a rooted script object without compiling synthetic wrapper code.
    pub fn push_proto_method(
        &mut self,
        receiver: Data,
        method_name: &str,
        args: Vec<Data>,
    ) -> ContextResult<()> {
        if self.modules.is_empty() {
            return Err(RuntimeError::EntryPointNotFound);
        }

        let concrete_type_id = match &receiver {
            Data::ProtoBoxRef {
                concrete_type_id, ..
            }
            | Data::ProtoRefRef {
                concrete_type_id, ..
            } => *concrete_type_id,
            other => {
                return Err(RuntimeError::Other(format!(
                    "push_proto_method: expected proto receiver for method '{}', got {:?}",
                    method_name, other
                )));
            }
        };

        let module_idx = self.stack.last().map(|f| f.module_idx).unwrap_or(0);
        let method_hash = Self::hash_method_name(method_name);
        let mut candidate_symbols = Vec::new();

        if let Some(crate::semantic::TypeDefinition::Struct(struct_def)) = self.modules[module_idx]
            .types
            .get(concrete_type_id as usize)
        {
            for &proto_id in &struct_def.conforming_to {
                let Some(crate::semantic::TypeDefinition::Proto(proto)) =
                    self.modules[module_idx].types.get(proto_id as usize)
                else {
                    continue;
                };
                if proto
                    .methods
                    .iter()
                    .any(|(name, _, _)| name.as_str() == method_name)
                    && let Some(&symbol_id) =
                        self.vtable.get(&(concrete_type_id, proto_id, method_hash))
                    && !candidate_symbols.contains(&symbol_id)
                {
                    candidate_symbols.push(symbol_id);
                }
            }
        }

        if candidate_symbols.is_empty() {
            for (&(candidate_type_id, _, candidate_hash), &symbol_id) in &self.vtable {
                if candidate_type_id == concrete_type_id
                    && candidate_hash == method_hash
                    && !candidate_symbols.contains(&symbol_id)
                {
                    candidate_symbols.push(symbol_id);
                }
            }
        }

        let method_symbol_id = match candidate_symbols.as_slice() {
            [symbol_id] => *symbol_id,
            [] => {
                return Err(RuntimeError::Other(format!(
                    "Method '{}' not found in vtable for type {}",
                    method_name, concrete_type_id
                )));
            }
            _ => {
                return Err(RuntimeError::Other(format!(
                    "Method '{}' is ambiguous for type {}",
                    method_name, concrete_type_id
                )));
            }
        };

        let (address, args_len) = {
            let symbol = self.modules[module_idx]
                .symbols
                .get(method_symbol_id as usize)
                .ok_or(RuntimeError::FunctionNotFound)?;

            let (func_id, address) = match &symbol.kind {
                crate::semantic::SymbolKind::Function { func_id, address } => (*func_id, *address),
                crate::semantic::SymbolKind::HostMethod { .. } => {
                    return Err(RuntimeError::Other(format!(
                        "push_proto_method: host-dispatched method '{}' is not supported",
                        method_name
                    )));
                }
                _ => return Err(RuntimeError::FunctionNotFound),
            };

            let function_type = self.modules[module_idx]
                .functions
                .get(func_id as usize)
                .ok_or(RuntimeError::FunctionNotFound)?;
            (address, function_type.params.len())
        };

        if args_len != args.len() + 1 {
            return Err(RuntimeError::Other(format!(
                "Method '{}' expects {} argument(s) plus receiver, got {}",
                method_name,
                args_len.saturating_sub(1),
                args.len()
            )));
        }

        let stack = &mut self.stack_frame_mut()?.stack;
        stack.push(receiver);
        stack.extend(args);

        let mut new_frame = StackFrame::new();
        new_frame.module_idx = module_idx;
        new_frame.params = stack.split_off(stack.len() - args_len);
        new_frame.pc =
            self.resolve_instruction_address(module_idx, address, "proto method address")?;
        self.stack.push(new_frame);
        Ok(())
    }

    pub fn resume(&mut self) -> ContextResult<()> {
        if self.stack_frame()?.pc == std::usize::MAX {
            return Err(RuntimeError::EntryPointNotFound);
        }

        // Stop when the frame the caller just pushed pops, so a re-entrant
        // `resume()` (driven by a host service invoked from inside another
        // `resume()`) returns after running only its own frame instead of
        // continuing to execute the outer caller's instructions.
        let base_depth = self.stack.len().saturating_sub(1);
        let prev_stop = self.stop_depth;
        self.stop_depth = Some(base_depth);

        let result = self.run_interpreter_loop();
        self.stop_depth = prev_stop;
        result?;

        // Defensive cleanup: if the just-pushed function exited by running
        // past the end of its module's instructions rather than via `Ret`
        // (legacy bytecode shape), the frame is still live — pop it and
        // thread its top-of-stack onto the caller's frame.
        if self.stack.len() > base_depth {
            let return_value = self.stack_frame_mut()?.stack.pop();
            self.stack.pop();
            if let Some(value) = return_value
                && let Ok(frame) = self.stack_frame_mut()
            {
                frame.stack.push(value);
            }
        }

        Ok(())
    }

    /// Invoke a closure value synchronously and return its result.
    /// Used by higher-order host functions (map, filter, etc.) to call p7 closures.
    pub fn call_closure(&mut self, closure: &Data, args: Vec<Data>) -> ContextResult<Data> {
        let (func_addr, captures) = match closure {
            Data::Closure {
                func_addr,
                captures,
            } => (*func_addr, captures.clone()),
            _ => {
                return Err(RuntimeError::Other(
                    "call_closure: expected closure value".to_string(),
                ));
            }
        };

        let base_depth = self.stack.len();
        let current_module_idx = self.stack.last().map(|f| f.module_idx).unwrap_or(0);

        let mut params = captures.as_ref().clone();
        params.extend(args);
        let target_pc =
            self.resolve_instruction_address(current_module_idx, func_addr, "closure address")?;
        let frame = StackFrame {
            params,
            locals: Vec::new(),
            stack: Vec::new(),
            pc: target_pc,
            module_idx: current_module_idx,
        };
        self.stack.push(frame);

        let prev_stop = self.stop_depth;
        self.stop_depth = Some(base_depth);

        let result = self.run_interpreter_loop();

        self.stop_depth = prev_stop;
        result?;

        self.stack_frame_mut()?
            .stack
            .pop()
            .ok_or(RuntimeError::Other(
                "call_closure: closure returned no value".to_string(),
            ))
    }

    /// Invoke a closure that returns no value (unit).
    pub fn call_closure_void(&mut self, closure: &Data, args: Vec<Data>) -> ContextResult<()> {
        let (func_addr, captures) = match closure {
            Data::Closure {
                func_addr,
                captures,
            } => (*func_addr, captures.clone()),
            _ => {
                return Err(RuntimeError::Other(
                    "call_closure_void: expected closure value".to_string(),
                ));
            }
        };

        let base_depth = self.stack.len();
        let current_module_idx = self.stack.last().map(|f| f.module_idx).unwrap_or(0);

        let mut params = captures.as_ref().clone();
        params.extend(args);
        let target_pc =
            self.resolve_instruction_address(current_module_idx, func_addr, "closure address")?;
        let frame = StackFrame {
            params,
            locals: Vec::new(),
            stack: Vec::new(),
            pc: target_pc,
            module_idx: current_module_idx,
        };
        self.stack.push(frame);

        let prev_stop = self.stop_depth;
        self.stop_depth = Some(base_depth);

        let result = self.run_interpreter_loop();

        self.stop_depth = prev_stop;
        result
    }

    pub(super) fn run_interpreter_loop(&mut self) -> ContextResult<()> {
        loop {
            // When running a closure invocation, stop once the closure frame has returned
            if let Some(depth) = self.stop_depth
                && self.stack.len() <= depth
            {
                break;
            }

            let module_idx = self.stack_frame()?.module_idx;
            let inst_pc = self.stack_frame()?.pc;

            // Check if we've reached the end of the current module's instructions
            if inst_pc >= self.modules[module_idx].decoded_len() {
                break;
            }

            let pc = self.modules[module_idx]
                .instruction_index_to_bytecode_address(inst_pc)
                .unwrap_or(inst_pc as u32) as usize;
            let instruction = self.modules[module_idx]
                .decoded_instruction(inst_pc)
                .cloned()
                .ok_or_else(|| {
                    RuntimeError::Other(format!(
                        "instruction index {} out of bounds in module {}",
                        inst_pc, module_idx
                    ))
                })?;

            self.stack_frame_mut()?.pc += 1;

            match instruction {
                Instruction::Ldi(val) => self.stack_frame_mut()?.stack.push(Data::Int(val)),
                Instruction::Ldf(val) => self.stack_frame_mut()?.stack.push(Data::Float(val)),
                Instruction::Lds(string_index) => {
                    let module_idx = self.stack_frame()?.module_idx;
                    let string_const = self.modules[module_idx]
                        .string_constants
                        .get(string_index as usize)
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "String constant index {} out of bounds",
                                string_index
                            ))
                        })?
                        .clone();
                    self.stack_frame_mut()?
                        .stack
                        .push(Data::string(string_const));
                }
                Instruction::Ldvar(idx) => {
                    let locals_len = self.stack_frame()?.locals.len();
                    if (idx as usize) < locals_len {
                        let local = self.stack_frame_mut()?.locals[idx as usize].clone();
                        self.stack_frame_mut()?.stack.push(local);
                    } else {
                        return Err(RuntimeError::VariableNotFound(format!(
                            "local variable index {} out of bounds (only {} locals allocated) at pc {}",
                            idx, locals_len, pc
                        )));
                    }
                }
                Instruction::Stvar(idx) => {
                    if let Some(data) = self.stack_frame_mut()?.stack.pop() {
                        if idx as usize >= self.stack_frame_mut()?.locals.len() {
                            self.stack_frame_mut()?
                                .locals
                                .resize(idx as usize + 1, Data::Int(0)); // Resize with a default value
                        }
                        self.stack_frame_mut()?.locals[idx as usize] = data;
                    } else {
                        return Err(RuntimeError::StackUnderflow);
                    }
                }
                Instruction::Ldpar(param_id) => {
                    let params_len = self.stack_frame()?.params.len();
                    if (param_id as usize) < params_len {
                        let param = self.stack_frame_mut()?.params[param_id as usize].clone();
                        self.stack_frame_mut()?.stack.push(param);
                    } else {
                        return Err(RuntimeError::VariableNotFound(format!(
                            "parameter index {} out of bounds (only {} parameters) at pc {}",
                            param_id, params_len, pc
                        )));
                    }
                }
                Instruction::Add => {
                    arithmetic_op!(self, +);
                }
                Instruction::Sub => {
                    arithmetic_op!(self, -);
                }
                Instruction::Mul => {
                    arithmetic_op!(self, *);
                }
                Instruction::Div => {
                    arithmetic_op!(self, /);
                }
                Instruction::Mod => {
                    arithmetic_op!(self, %);
                }
                Instruction::BitAnd => {
                    let b = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    let a = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    match (a, b) {
                        (Data::Int(a), Data::Int(b)) => {
                            self.stack_frame_mut()?.stack.push(Data::Int(a & b));
                        }
                        _ => {
                            return Err(RuntimeError::Other(
                                "Bitwise AND requires int operands".to_string(),
                            ));
                        }
                    }
                }
                Instruction::BitOr => {
                    let b = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    let a = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    match (a, b) {
                        (Data::Int(a), Data::Int(b)) => {
                            self.stack_frame_mut()?.stack.push(Data::Int(a | b));
                        }
                        _ => {
                            return Err(RuntimeError::Other(
                                "Bitwise OR requires int operands".to_string(),
                            ));
                        }
                    }
                }
                Instruction::BitXor => {
                    let b = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    let a = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    match (a, b) {
                        (Data::Int(a), Data::Int(b)) => {
                            self.stack_frame_mut()?.stack.push(Data::Int(a ^ b));
                        }
                        _ => {
                            return Err(RuntimeError::Other(
                                "Bitwise XOR requires int operands".to_string(),
                            ));
                        }
                    }
                }
                Instruction::LdModVar(var_id) => {
                    let module_idx = self.stack_frame()?.module_idx;
                    if module_idx < self.module_vars.len() {
                        let vars = &self.module_vars[module_idx];
                        if (var_id as usize) < vars.len() {
                            let val = vars[var_id as usize].clone();
                            self.stack_frame_mut()?.stack.push(val);
                        } else {
                            return Err(RuntimeError::VariableNotFound(format!(
                                "module variable index {} out of bounds (only {} module vars) at pc {}",
                                var_id,
                                vars.len(),
                                pc
                            )));
                        }
                    } else {
                        return Err(RuntimeError::VariableNotFound(format!(
                            "module index {} out of bounds for module_vars at pc {}",
                            module_idx, pc
                        )));
                    }
                }
                Instruction::StModVar(var_id) => {
                    let module_idx = self.stack_frame()?.module_idx;
                    if let Some(data) = self.stack_frame_mut()?.stack.pop() {
                        if module_idx < self.module_vars.len() {
                            let vars = &mut self.module_vars[module_idx];
                            if (var_id as usize) >= vars.len() {
                                vars.resize(var_id as usize + 1, Data::Int(0));
                            }
                            vars[var_id as usize] = data;
                        } else {
                            return Err(RuntimeError::VariableNotFound(format!(
                                "module index {} out of bounds for module_vars at pc {}",
                                module_idx, pc
                            )));
                        }
                    } else {
                        return Err(RuntimeError::StackUnderflow);
                    }
                }
                Instruction::LdExtModVar(module_path_sid, var_name_sid) => {
                    let current_module_idx = self.stack_frame()?.module_idx;
                    let target = self.modules[current_module_idx]
                        .external_var_target(inst_pc)
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Unresolved external variable load at instruction {} (module_path_sid={}, var_name_sid={})",
                                inst_pc, module_path_sid, var_name_sid
                            ))
                        })?;

                    let val = self.module_vars[target.module_idx]
                        .get(target.var_id as usize)
                        .ok_or_else(|| {
                            RuntimeError::VariableNotFound(format!(
                                "Module variable index {} out of bounds in module index {}",
                                target.var_id, target.module_idx
                            ))
                        })?
                        .clone();
                    self.stack_frame_mut()?.stack.push(val);
                }
                Instruction::StExtModVar(module_path_sid, var_name_sid) => {
                    let current_module_idx = self.stack_frame()?.module_idx;
                    let target = self.modules[current_module_idx]
                        .external_var_target(inst_pc)
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Unresolved external variable store at instruction {} (module_path_sid={}, var_name_sid={})",
                                inst_pc, module_path_sid, var_name_sid
                            ))
                        })?;
                    let target_module_idx = target.module_idx;
                    let var_id = target.var_id;

                    let data = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    let vars = &mut self.module_vars[target_module_idx];
                    if (var_id as usize) >= vars.len() {
                        vars.resize(var_id as usize + 1, Data::Int(0));
                    }
                    vars[var_id as usize] = data;
                }
                Instruction::Neg => {
                    if let Some(data) = self.stack_frame_mut()?.stack.pop() {
                        match data {
                            Data::Int(i) => self.stack_frame_mut()?.stack.push(Data::Int(-i)),
                            Data::Float(f) => self.stack_frame_mut()?.stack.push(Data::Float(-f)),
                            Data::String(_) => {
                                return Err(RuntimeError::Other(
                                    "Cannot negate string".to_string(),
                                ));
                            }
                            Data::StructRef(r) => {
                                return Err(RuntimeError::Other(format!(
                                    "Cannot negate struct reference (ref {})",
                                    r
                                )));
                            }
                            _ => {
                                return Err(RuntimeError::Other(
                                    "Cannot negate this value type".to_string(),
                                ));
                            }
                        }
                    } else {
                        unimplemented!();
                    }
                }
                Instruction::And => self.binary_op_int(|a, b| (a != 0 && b != 0) as i64)?,
                Instruction::Or => self.binary_op_int(|a, b| (a != 0 || b != 0) as i64)?,
                Instruction::Not => {
                    if let Some(data) = self.stack_frame_mut()?.stack.pop() {
                        match data {
                            Data::Int(i) => self
                                .stack_frame_mut()?
                                .stack
                                .push(Data::Int((i == 0) as i64)),
                            Data::Float(f) => self
                                .stack_frame_mut()?
                                .stack
                                .push(Data::Int((f == 0.0) as i64)),
                            Data::String(_) => {
                                return Err(RuntimeError::Other(
                                    "Cannot apply logical NOT to string".to_string(),
                                ));
                            }
                            Data::StructRef(r) => {
                                return Err(RuntimeError::Other(format!(
                                    "Cannot apply logical NOT to struct reference (ref {})",
                                    r
                                )));
                            }
                            _ => {
                                return Err(RuntimeError::Other(
                                    "Cannot apply logical NOT to this value type".to_string(),
                                ));
                            }
                        }
                    } else {
                        return Err(RuntimeError::StackUnderflow);
                    }
                }
                Instruction::Eq => {
                    comparison_op!(self, ==);
                }
                Instruction::Neq => {
                    comparison_op!(self, !=);
                }
                Instruction::Lt => {
                    comparison_op!(self, <);
                }
                Instruction::Gt => {
                    comparison_op!(self, >);
                }
                Instruction::Lte => {
                    comparison_op!(self, <=);
                }
                Instruction::Gte => {
                    comparison_op!(self, >=);
                }
                Instruction::Jmp(addr) => {
                    self.stack_frame_mut()?.pc =
                        self.resolve_instruction_address(module_idx, addr, "jump target")?;
                }
                Instruction::Jif(addr) => {
                    if let Some(Data::Int(condition)) = self.stack_frame_mut()?.stack.pop() {
                        if condition != 0 {
                            self.stack_frame_mut()?.pc = self.resolve_instruction_address(
                                module_idx,
                                addr,
                                "conditional jump target",
                            )?;
                        }
                    } else {
                        unimplemented!();
                    }
                }
                Instruction::Call(_) => {
                    let module_idx = self.stack_frame()?.module_idx;
                    let call_target = self.modules[module_idx]
                        .direct_call_target(inst_pc)
                        .ok_or(RuntimeError::FunctionNotFound)?
                        .clone();

                    let mut new_frame = StackFrame::new();
                    new_frame.module_idx = module_idx; // Stay in the same module
                    let stack = &mut self.stack_frame_mut()?.stack;
                    new_frame.params = stack.split_off(stack.len() - call_target.args_len);
                    new_frame.pc = call_target.target_pc;

                    self.stack.push(new_frame);
                }
                Instruction::Ldfield(field_idx) => {
                    // Expect a StructRef, BoxRef, ProtoRefRef, or Int (enum tag) on the stack.
                    if let Some(data) = self.stack_frame_mut()?.stack.pop() {
                        // Resolve BoxRef/ProtoBoxRef/ProtoRefRef to the underlying value
                        let resolved_data = match &data {
                            Data::BoxRef(idx) | Data::ProtoBoxRef { box_idx: idx, .. } => {
                                self.box_heap.get(*idx as usize).cloned().ok_or_else(|| {
                                    RuntimeError::Other(format!("Invalid box reference: {}", idx))
                                })?
                            }
                            Data::ProtoRefRef { ref_idx, .. } => {
                                // ProtoRefRef points to heap location like StructRef
                                Data::StructRef(*ref_idx)
                            }
                            other => other.clone(),
                        };

                        match resolved_data {
                            Data::StructRef(ref_id) => {
                                let ref_usize = ref_id as usize;
                                if ref_usize >= self.heap.len() {
                                    return Err(RuntimeError::VariableNotFound(format!(
                                        "struct ref {} out of bounds (heap size {}) at pc {}",
                                        ref_id,
                                        self.heap.len(),
                                        pc
                                    )));
                                }
                                let struct_fields = &self.heap[ref_usize].fields;
                                if (field_idx as usize) >= struct_fields.len() {
                                    return Err(RuntimeError::VariableNotFound(format!(
                                        "field index {} out of bounds (struct has {} fields) at pc {}",
                                        field_idx,
                                        struct_fields.len(),
                                        pc
                                    )));
                                }
                                let field_value = struct_fields[field_idx as usize].clone();
                                self.stack_frame_mut()?.stack.push(field_value);
                            }
                            // Int values represent no-payload enum variants where the
                            // Int itself IS the variant tag. ldfield(0) extracts the tag.
                            Data::Int(tag) if field_idx == 0 => {
                                self.stack_frame_mut()?.stack.push(Data::Int(tag));
                            }
                            other => {
                                return Err(RuntimeError::VariableNotFound(format!(
                                    "cannot load field {} from {:?} value at pc {}",
                                    field_idx,
                                    std::mem::discriminant(&other),
                                    pc
                                )));
                            }
                        }
                    } else {
                        return Err(RuntimeError::StackUnderflow);
                    }
                }
                Instruction::Stfield(field_idx) => {
                    // Expect: [..., struct_ref, field_value] (field_value on top).
                    // Pop field_value then struct_ref, update heap, do not push a value (assignment yields unit).
                    let field_value_opt = self.stack_frame_mut()?.stack.pop();
                    let struct_ref_opt = self.stack_frame_mut()?.stack.pop();
                    if field_value_opt.is_none() || struct_ref_opt.is_none() {
                        return Err(RuntimeError::StackUnderflow);
                    }
                    let field_value = field_value_opt.unwrap();
                    let struct_ref_data = struct_ref_opt.unwrap();

                    // Resolve BoxRef/ProtoBoxRef/ProtoRefRef to the underlying StructRef
                    let resolved_ref = match &struct_ref_data {
                        Data::BoxRef(idx) | Data::ProtoBoxRef { box_idx: idx, .. } => {
                            self.box_heap.get(*idx as usize).cloned().ok_or_else(|| {
                                RuntimeError::Other(format!("Invalid box reference: {}", idx))
                            })?
                        }
                        Data::ProtoRefRef { ref_idx, .. } => Data::StructRef(*ref_idx),
                        other => other.clone(),
                    };

                    match resolved_ref {
                        Data::StructRef(ref_id) => {
                            let ref_usize = ref_id as usize;
                            if ref_usize >= self.heap.len() {
                                return Err(RuntimeError::VariableNotFound(format!(
                                    "struct ref {} out of bounds (heap size {}) in Stfield at pc {}",
                                    ref_id,
                                    self.heap.len(),
                                    pc
                                )));
                            }
                            if (field_idx as usize) >= self.heap[ref_usize].fields.len() {
                                return Err(RuntimeError::VariableNotFound(format!(
                                    "field index {} out of bounds (struct has {} fields) in Stfield at pc {}",
                                    field_idx,
                                    self.heap[ref_usize].fields.len(),
                                    pc
                                )));
                            }
                            self.heap[ref_usize].fields[field_idx as usize] = field_value;
                        }
                        other => {
                            return Err(RuntimeError::VariableNotFound(format!(
                                "cannot store field {} on {:?} value in Stfield at pc {}",
                                field_idx,
                                std::mem::discriminant(&other),
                                pc
                            )));
                        }
                    }
                }
                Instruction::NewStruct(field_count) => {
                    // Pop field values from stack in reverse order
                    let mut fields = Vec::with_capacity(field_count as usize);
                    for _ in 0..field_count {
                        if let Some(value) = self.stack_frame_mut()?.stack.pop() {
                            fields.push(value);
                        } else {
                            return Err(RuntimeError::StackUnderflow);
                        }
                    }

                    // Reverse to get fields in definition order
                    fields.reverse();

                    // Allocate struct on heap
                    let struct_ref = self.heap.len() as u32;
                    self.heap.push(Struct { fields });

                    // Push reference onto stack
                    self.stack_frame_mut()?
                        .stack
                        .push(Data::StructRef(struct_ref));
                }
                Instruction::Ret => {
                    let return_value = self.stack_frame_mut()?.stack.pop();
                    self.stack.pop();
                    if let Some(value) = return_value
                        && let Ok(frame) = self.stack_frame_mut()
                    {
                        frame.stack.push(value);
                    }
                }
                Instruction::Pop => {
                    self.stack_frame_mut()?.stack.pop();
                }
                Instruction::Dup => {
                    // Duplicate the top value on the stack
                    if let Some(value) = self.stack_frame()?.stack.last() {
                        let duplicated = value.clone();
                        self.stack_frame_mut()?.stack.push(duplicated);
                    } else {
                        return Err(RuntimeError::StackUnderflow);
                    }
                }
                Instruction::Throw => {
                    // Pop the value from stack and convert it to an Exception
                    if let Some(value) = self.stack_frame_mut()?.stack.pop() {
                        let exception_value = match value {
                            Data::Int(i) => i,
                            Data::Exception(e) => e, // Already an exception
                            _ => {
                                return Err(RuntimeError::Other(
                                    "Can only throw integer/enum values".to_string(),
                                ));
                            }
                        };
                        // Pop the current stack frame (return from function)
                        self.stack.pop();
                        // Push exception to caller's frame
                        if let Ok(frame) = self.stack_frame_mut() {
                            frame.stack.push(Data::Exception(exception_value));
                        }
                        // Don't return Ok(()), continue execution in caller
                    } else {
                        return Err(RuntimeError::StackUnderflow);
                    }
                }
                Instruction::CheckException(jump_addr) => {
                    // Check if top of stack is an exception
                    if let Some(value) = self.stack_frame()?.stack.last() {
                        if matches!(value, Data::Exception(_)) {
                            // It's an exception, jump to handler
                            self.stack_frame_mut()?.pc = self.resolve_instruction_address(
                                module_idx,
                                jump_addr,
                                "exception handler target",
                            )?;
                        }
                        // Otherwise, continue normally
                    } else {
                        return Err(RuntimeError::StackUnderflow);
                    }
                }
                Instruction::UnwrapException => {
                    // Convert Exception back to regular value for pattern matching
                    if let Some(Data::Exception(value)) = self.stack_frame_mut()?.stack.pop() {
                        self.stack_frame_mut()?.stack.push(Data::Int(value));
                    } else {
                        return Err(RuntimeError::Other(
                            "Expected exception on stack".to_string(),
                        ));
                    }
                }
                Instruction::BoxAlloc => {
                    // Pop value from stack, allocate a box, store value, push BoxRef
                    let value = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    let box_idx = self.box_heap.len() as u32;
                    self.box_heap.push(value);
                    self.stack_frame_mut()?.stack.push(Data::BoxRef(box_idx));

                    // Trigger GC if threshold is reached
                    self.allocation_count += 1;
                    if self.allocation_count >= self.gc_threshold {
                        self.collect_garbage()?;
                        self.allocation_count = 0;
                    }
                }
                Instruction::BoxDeref => {
                    // Pop BoxRef from stack, load value from box, push value
                    let box_ref = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    match box_ref {
                        Data::BoxRef(idx) | Data::ProtoBoxRef { box_idx: idx, .. } => {
                            let value = self
                                .box_heap
                                .get(idx as usize)
                                .ok_or_else(|| {
                                    RuntimeError::Other(format!("Invalid box reference: {}", idx))
                                })?
                                .clone();
                            self.stack_frame_mut()?.stack.push(value);
                        }
                        _ => {
                            return Err(RuntimeError::Other(
                                "Expected BoxRef on stack for deref".to_string(),
                            ));
                        }
                    }
                }
                Instruction::BoxToProto(struct_type_id, _proto_type_id) => {
                    // Convert box<T> to box<P> for dynamic dispatch
                    // Pop BoxRef, push ProtoBoxRef with type info
                    let box_ref = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;

                    if let Data::BoxRef(box_idx) = box_ref {
                        // Create a ProtoBoxRef with the concrete type information
                        self.stack_frame_mut()?.stack.push(Data::ProtoBoxRef {
                            box_idx,
                            concrete_type_id: struct_type_id,
                        });
                    } else {
                        return Err(RuntimeError::Other(format!(
                            "Expected BoxRef on stack for BoxToProto, found {:?}",
                            box_ref
                        )));
                    }
                }
                Instruction::RefToProto(struct_type_id, _proto_type_id) => {
                    // Convert ref<T> to ref<P> for dynamic dispatch
                    // Pop StructRef, push ProtoRefRef with type info
                    let struct_ref = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;

                    if let Data::StructRef(ref_idx) = struct_ref {
                        // Create a ProtoRefRef with the concrete type information
                        self.stack_frame_mut()?.stack.push(Data::ProtoRefRef {
                            ref_idx,
                            concrete_type_id: struct_type_id,
                        });
                    } else {
                        return Err(RuntimeError::Other(format!(
                            "Expected StructRef on stack for RefToProto, found {:?}",
                            struct_ref
                        )));
                    }
                }
                Instruction::CallProtoMethod(proto_id, method_hash) => {
                    let module_idx = self.stack_frame()?.module_idx;
                    let method_meta = self.modules[module_idx]
                        .proto_method_meta(proto_id, method_hash)
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Method with hash {} not found in proto {}",
                                method_hash, proto_id
                            ))
                        })?
                        .clone();

                    // The receiver (self) is at position stack.len() - param_count
                    let stack_len = self.stack_frame()?.stack.len();
                    if stack_len < method_meta.param_count {
                        return Err(RuntimeError::StackUnderflow);
                    }

                    let receiver_idx = stack_len - method_meta.param_count;
                    let receiver = self
                        .stack_frame()?
                        .stack
                        .get(receiver_idx)
                        .ok_or(RuntimeError::StackUnderflow)?;

                    let concrete_type_id = match receiver {
                        Data::ProtoBoxRef {
                            concrete_type_id, ..
                        } => *concrete_type_id,
                        Data::ProtoRefRef {
                            concrete_type_id, ..
                        } => *concrete_type_id,
                        _ => {
                            return Err(RuntimeError::Other(format!(
                                "Expected ProtoBoxRef or ProtoRefRef as receiver for proto method call, found {:?}",
                                receiver
                            )));
                        }
                    };

                    let method_symbol_id = self
                        .vtable
                        .get(&(concrete_type_id, proto_id, method_hash))
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Method '{}' not found in vtable for type {} implementing proto {}",
                                method_meta.method_name, concrete_type_id, proto_id
                            ))
                        })?;

                    let dispatch_kind = self.modules[module_idx]
                        .symbol_dispatch(*method_symbol_id)
                        .ok_or(RuntimeError::FunctionNotFound)?
                        .clone();

                    match dispatch_kind {
                        ResolvedDispatch::Function {
                            target_pc,
                            args_len,
                        } => {
                            let mut new_frame = StackFrame::new();
                            new_frame.module_idx = module_idx;
                            let stack = &mut self.stack_frame_mut()?.stack;
                            new_frame.params = stack.split_off(stack.len() - args_len);
                            new_frame.pc = target_pc;
                            self.stack.push(new_frame);
                        }
                        ResolvedDispatch::Host {
                            dispatcher_name,
                            method_name,
                            type_tag,
                            vtable_slot,
                            return_ty,
                        } => {
                            // Convention (top → bottom):
                            //   type_tag, method_name, vtable_slot, return_ty,
                            //   args (last decl on top), receiver
                            // The dispatcher pops its metadata first, then
                            // walks the args using the runtime shape of each
                            // `Data` value.
                            let return_ty_marker = encode_return_ty(&return_ty);
                            self.stack_frame_mut()?.stack.push(return_ty_marker);
                            self.stack_frame_mut()?
                                .stack
                                .push(Data::Int(vtable_slot as i64));
                            self.stack_frame_mut()?
                                .stack
                                .push(Data::string(method_name));
                            self.stack_frame_mut()?.stack.push(Data::string(type_tag));
                            let host_fn =
                                self.host_functions.get(&dispatcher_name).ok_or_else(|| {
                                    RuntimeError::Other(format!(
                                        "Foreign-proto dispatcher host fn not found: {}",
                                        dispatcher_name
                                    ))
                                })?;
                            host_fn(self)?;
                        }
                    }
                }
                Instruction::InvokeHost(string_index) => {
                    // Look up the host function name from string constants
                    let module_idx = self.stack_frame()?.module_idx;
                    let function_name = self.modules[module_idx]
                        .string_constants
                        .get(string_index as usize)
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Invalid string constant index for host function: {}",
                                string_index
                            ))
                        })?;

                    // Look up and call the host function
                    let host_fn = self.host_functions.get(function_name).ok_or_else(|| {
                        RuntimeError::Other(format!("Host function not found: {}", function_name))
                    })?;

                    // Call the host function
                    host_fn(self)?;
                }
                Instruction::CallExternal(module_path_idx, symbol_name_idx) => {
                    let current_module_idx = self.stack_frame()?.module_idx;
                    let target = self.modules[current_module_idx]
                        .external_call_target(inst_pc)
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Unresolved external call at instruction {} (module_path_sid={}, symbol_name_sid={})",
                                inst_pc, module_path_idx, symbol_name_idx
                            ))
                        })?
                        .clone();

                    // Create new stack frame with arguments
                    let mut new_frame = StackFrame::new();
                    new_frame.module_idx = target.module_idx; // Execute in the context of the target module
                    let stack = &mut self.stack_frame_mut()?.stack;
                    new_frame.params = stack.split_off(stack.len() - target.args_len);
                    new_frame.pc = target.target_pc;

                    self.stack.push(new_frame);
                }
                Instruction::Ldnull => {
                    self.stack_frame_mut()?.stack.push(Data::Null);
                }
                Instruction::WrapNullable => {
                    let value = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    self.stack_frame_mut()?.stack.push(Data::some(value));
                }
                Instruction::IsNull => {
                    let nullable = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    let is_null = matches!(nullable, Data::Null);
                    self.stack_frame_mut()?
                        .stack
                        .push(Data::Int(if is_null { 1 } else { 0 }));
                }
                Instruction::ForceUnwrap => {
                    let nullable = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    match nullable {
                        Data::Some(value) => {
                            self.stack_frame_mut()?.stack.push(value.as_ref().clone());
                        }
                        Data::Null => {
                            return Err(RuntimeError::Other(
                                "Force unwrap on null value".to_string(),
                            ));
                        }
                        _ => {
                            return Err(RuntimeError::Other(
                                "Force unwrap on non-nullable value".to_string(),
                            ));
                        }
                    }
                }
                Instruction::NullCoalesce => {
                    let default = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    let nullable = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    match nullable {
                        Data::Some(value) => {
                            self.stack_frame_mut()?.stack.push(value.as_ref().clone());
                        }
                        Data::Null => {
                            self.stack_frame_mut()?.stack.push(default);
                        }
                        _ => {
                            return Err(RuntimeError::Other(
                                "Null coalesce on non-nullable value".to_string(),
                            ));
                        }
                    }
                }

                Instruction::MakeClosure(func_addr, capture_count) => {
                    let mut captures = Vec::new();
                    for _ in 0..capture_count {
                        let val = self
                            .stack_frame_mut()?
                            .stack
                            .pop()
                            .ok_or(RuntimeError::StackUnderflow)?;
                        captures.push(val);
                    }
                    captures.reverse();
                    self.stack_frame_mut()?
                        .stack
                        .push(Data::closure(func_addr, captures));
                }

                Instruction::CallClosure(arg_count) => {
                    // Pop arguments
                    let mut args = Vec::new();
                    for _ in 0..arg_count {
                        let val = self
                            .stack_frame_mut()?
                            .stack
                            .pop()
                            .ok_or(RuntimeError::StackUnderflow)?;
                        args.push(val);
                    }
                    args.reverse();

                    // Pop the closure value
                    let closure = self
                        .stack_frame_mut()?
                        .stack
                        .pop()
                        .ok_or(RuntimeError::StackUnderflow)?;

                    match closure {
                        Data::Closure {
                            func_addr,
                            captures,
                        } => {
                            let current_module_idx =
                                self.stack.last().map(|f| f.module_idx).unwrap_or(0);
                            // Create new frame. Params = captures + args
                            let mut params = captures.as_ref().clone();
                            params.extend(args);
                            let target_pc = self.resolve_instruction_address(
                                current_module_idx,
                                func_addr,
                                "closure address",
                            )?;
                            let frame = StackFrame {
                                params,
                                locals: Vec::new(),
                                stack: Vec::new(),
                                pc: target_pc,
                                module_idx: current_module_idx,
                            };
                            self.stack.push(frame);
                        }
                        _ => {
                            return Err(RuntimeError::Other(
                                "CallClosure on non-closure value".to_string(),
                            ));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub(super) fn stack_frame(&self) -> ContextResult<&StackFrame> {
        self.stack.last().ok_or(RuntimeError::NoStackFrame)
    }

    pub fn stack_frame_mut(&mut self) -> ContextResult<&mut StackFrame> {
        self.stack.last_mut().ok_or(RuntimeError::NoStackFrame)
    }

    fn resolve_instruction_address(
        &self,
        module_idx: usize,
        byte_address: u32,
        label: &str,
    ) -> ContextResult<usize> {
        let module = self.modules.get(module_idx).ok_or_else(|| {
            RuntimeError::Other(format!(
                "{} references missing module {}",
                label, module_idx
            ))
        })?;
        module
            .bytecode_address_to_instruction_index(byte_address)
            .ok_or_else(|| {
                RuntimeError::Other(format!(
                    "{} byte address {} does not point to a decoded instruction boundary in module {}",
                    label, byte_address, module_idx
                ))
            })
    }

    fn binary_op_int<F>(&mut self, op: F) -> ContextResult<()>
    where
        F: Fn(i64, i64) -> i64,
    {
        let frame = self.stack_frame_mut()?;
        let (a, b) = frame.pop2()?;

        match (a, b) {
            (Data::Int(a), Data::Int(b)) => {
                frame.push(Data::Int(op(a, b)));
            }
            (Data::Exception(_), _) | (_, Data::Exception(_)) => {
                return Err(RuntimeError::Other(
                    "Binary operation on exception value".to_string(),
                ));
            }
            _ => {
                return Err(RuntimeError::Other(
                    "Invalid types for binary operation".to_string(),
                ));
            }
        }

        Ok(())
    }
}
