use std::io::Cursor;

use binrw::BinRead;

use crate::bytecode::Instruction;
use crate::errors::RuntimeError;

use super::Context;
use super::data::{ContextResult, Data, StackFrame, Struct};

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
        stack_frame.pc = addr as usize;

        self.stack.push(stack_frame);
    }

    pub fn resume(&mut self) -> ContextResult<()> {
        if self.stack_frame()?.pc == std::usize::MAX {
            return Err(RuntimeError::EntryPointNotFound);
        }

        self.run_interpreter_loop()?;

        // Current function has finished executing. Pop the stack frame, and push return value if any.
        if self.stack.len() > 1 {
            let return_value = self.stack_frame_mut()?.stack.pop();
            self.stack.pop();
            if let Some(value) = return_value {
                self.stack_frame_mut()?.stack.push(value);
            }
        }

        Ok(())
    }

    /// Invoke a closure value synchronously and return its result.
    /// Used by higher-order host functions (map, filter, etc.) to call p7 closures.
    pub fn call_closure(&mut self, closure: &Data, args: Vec<Data>) -> ContextResult<Data> {
        let (func_addr, captures) = match closure {
            Data::Closure { func_addr, captures } => (*func_addr, captures.clone()),
            _ => {
                return Err(RuntimeError::Other(
                    "call_closure: expected closure value".to_string(),
                ))
            }
        };

        let base_depth = self.stack.len();
        let current_module_idx = self.stack.last().map(|f| f.module_idx).unwrap_or(0);

        let mut params = captures;
        params.extend(args);
        let frame = StackFrame {
            params,
            locals: Vec::new(),
            stack: Vec::new(),
            pc: func_addr as usize,
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
            Data::Closure { func_addr, captures } => (*func_addr, captures.clone()),
            _ => {
                return Err(RuntimeError::Other(
                    "call_closure_void: expected closure value".to_string(),
                ))
            }
        };

        let base_depth = self.stack.len();
        let current_module_idx = self.stack.last().map(|f| f.module_idx).unwrap_or(0);

        let mut params = captures;
        params.extend(args);
        let frame = StackFrame {
            params,
            locals: Vec::new(),
            stack: Vec::new(),
            pc: func_addr as usize,
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
                && self.stack.len() <= depth {
                    break;
                }

            let module_idx = self.stack_frame()?.module_idx;
            let pc = self.stack_frame()?.pc;

            // Check if we've reached the end of the current module's instructions
            if pc >= self.modules[module_idx].instructions.len() {
                break;
            }

            let mut reader = Cursor::new(&self.modules[module_idx].instructions[pc..]);
            let instruction = Instruction::read(&mut reader).unwrap();

            self.stack_frame_mut()?.pc += reader.position() as usize;

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
                        .push(Data::String(string_const));
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
                    let b = self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
                    let a = self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
                    match (a, b) {
                        (Data::Int(a), Data::Int(b)) => {
                            self.stack_frame_mut()?.stack.push(Data::Int(a & b));
                        }
                        _ => return Err(RuntimeError::Other("Bitwise AND requires int operands".to_string())),
                    }
                }
                Instruction::BitOr => {
                    let b = self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
                    let a = self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
                    match (a, b) {
                        (Data::Int(a), Data::Int(b)) => {
                            self.stack_frame_mut()?.stack.push(Data::Int(a | b));
                        }
                        _ => return Err(RuntimeError::Other("Bitwise OR requires int operands".to_string())),
                    }
                }
                Instruction::BitXor => {
                    let b = self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
                    let a = self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
                    match (a, b) {
                        (Data::Int(a), Data::Int(b)) => {
                            self.stack_frame_mut()?.stack.push(Data::Int(a ^ b));
                        }
                        _ => return Err(RuntimeError::Other("Bitwise XOR requires int operands".to_string())),
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
                                var_id, vars.len(), pc
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
                    let module_path = self.modules[current_module_idx]
                        .string_constants
                        .get(module_path_sid as usize)
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Invalid string constant index for module path: {}",
                                module_path_sid
                            ))
                        })?
                        .clone();
                    let var_name = self.modules[current_module_idx]
                        .string_constants
                        .get(var_name_sid as usize)
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Invalid string constant index for var name: {}",
                                var_name_sid
                            ))
                        })?
                        .clone();

                    let target_module_idx =
                        *self.imported_modules.get(&module_path).ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Module '{}' not found in imported modules",
                                module_path
                            ))
                        })?;

                    let var_id = self.modules[target_module_idx]
                        .module_variables
                        .iter()
                        .find(|v| v.name == var_name)
                        .map(|v| v.var_id)
                        .ok_or_else(|| {
                            RuntimeError::VariableNotFound(format!(
                                "Module variable '{}' not found in module '{}'",
                                var_name, module_path
                            ))
                        })?;

                    let val = self.module_vars[target_module_idx]
                        .get(var_id as usize)
                        .ok_or_else(|| {
                            RuntimeError::VariableNotFound(format!(
                                "Module variable index {} out of bounds in module '{}'",
                                var_id, module_path
                            ))
                        })?
                        .clone();
                    self.stack_frame_mut()?.stack.push(val);
                }
                Instruction::StExtModVar(module_path_sid, var_name_sid) => {
                    let current_module_idx = self.stack_frame()?.module_idx;
                    let module_path = self.modules[current_module_idx]
                        .string_constants
                        .get(module_path_sid as usize)
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Invalid string constant index for module path: {}",
                                module_path_sid
                            ))
                        })?
                        .clone();
                    let var_name = self.modules[current_module_idx]
                        .string_constants
                        .get(var_name_sid as usize)
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Invalid string constant index for var name: {}",
                                var_name_sid
                            ))
                        })?
                        .clone();

                    let target_module_idx =
                        *self.imported_modules.get(&module_path).ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Module '{}' not found in imported modules",
                                module_path
                            ))
                        })?;

                    let var_id = self.modules[target_module_idx]
                        .module_variables
                        .iter()
                        .find(|v| v.name == var_name)
                        .map(|v| v.var_id)
                        .ok_or_else(|| {
                            RuntimeError::VariableNotFound(format!(
                                "Module variable '{}' not found in module '{}'",
                                var_name, module_path
                            ))
                        })?;

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
                                    "Cannot negate struct reference (ref {})", r
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
                                    "Cannot apply logical NOT to struct reference (ref {})", r
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
                Instruction::Jmp(addr) => self.stack_frame_mut()?.pc = addr as usize,
                Instruction::Jif(addr) => {
                    if let Some(Data::Int(condition)) = self.stack_frame_mut()?.stack.pop() {
                        if condition != 0 {
                            self.stack_frame_mut()?.pc = addr as usize;
                        }
                    } else {
                        unimplemented!();
                    }
                }
                Instruction::Call(symbol_id) => {
                    let module_idx = self.stack_frame()?.module_idx;
                    let (address, args_len) = {
                        let symbol = self.modules[module_idx]
                            .symbols
                            .get(symbol_id as usize)
                            .ok_or(RuntimeError::FunctionNotFound)?;

                        let (func_id, address) = match &symbol.kind {
                            crate::semantic::SymbolKind::Function { func_id, address } => {
                                (*func_id, *address)
                            }
                            _ => return Err(RuntimeError::FunctionNotFound),
                        };

                        let function_type = self.modules[module_idx]
                            .functions
                            .get(func_id as usize)
                            .ok_or(RuntimeError::FunctionNotFound)?;

                        let args_len = function_type.params.len();
                        (address, args_len)
                    };

                    let mut new_frame = StackFrame::new();
                    new_frame.module_idx = module_idx; // Stay in the same module
                    let stack = &mut self.stack_frame_mut()?.stack;
                    new_frame.params = stack.split_off(stack.len() - args_len);
                    new_frame.pc = address as usize;

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
                                        ref_id, self.heap.len(), pc
                                    )));
                                }
                                let struct_fields = &self.heap[ref_usize].fields;
                                if (field_idx as usize) >= struct_fields.len() {
                                    return Err(RuntimeError::VariableNotFound(format!(
                                        "field index {} out of bounds (struct has {} fields) at pc {}",
                                        field_idx, struct_fields.len(), pc
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
                                    field_idx, std::mem::discriminant(&other), pc
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
                                    ref_id, self.heap.len(), pc
                                )));
                            }
                            if (field_idx as usize) >= self.heap[ref_usize].fields.len() {
                                return Err(RuntimeError::VariableNotFound(format!(
                                    "field index {} out of bounds (struct has {} fields) in Stfield at pc {}",
                                    field_idx, self.heap[ref_usize].fields.len(), pc
                                )));
                            }
                            self.heap[ref_usize].fields[field_idx as usize] = field_value;
                        }
                        other => {
                            return Err(RuntimeError::VariableNotFound(format!(
                                "cannot store field {} on {:?} value in Stfield at pc {}",
                                field_idx, std::mem::discriminant(&other), pc
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
                        && let Ok(frame) = self.stack_frame_mut() {
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
                            self.stack_frame_mut()?.pc = jump_addr as usize;
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
                        self.collect_garbage();
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
                    // Dynamic dispatch: look up method in vtable based on concrete type
                    // The receiver should be a ProtoBoxRef on the stack with arguments after it

                    // We need to peek at the receiver to get the concrete type
                    // The receiver is at the bottom of the arguments
                    // For now, we'll assume the receiver is the first argument (self parameter)

                    // First, let's find the function signature to know how many args there are
                    // We'll need to look up the proto method to get param count
                    let module_idx = self.stack_frame()?.module_idx;
                    let proto_type = self.modules[module_idx]
                        .types
                        .get(proto_id as usize)
                        .ok_or(RuntimeError::Other("Proto type not found".to_string()))?;

                    let method_name = self.get_method_name_from_hash(proto_id, method_hash)?;

                    let param_count =
                        if let crate::semantic::TypeDefinition::Proto(proto) = proto_type {
                            proto
                                .methods
                                .iter()
                                .find(|(name, _, _)| Self::hash_method_name(name) == method_hash)
                                .map(|(_, params, _)| params.len())
                                .ok_or(RuntimeError::Other("Method not found in proto".to_string()))?
                        } else {
                            return Err(RuntimeError::Other("Expected proto type".to_string()));
                        };

                    // The receiver (self) is at position stack.len() - param_count
                    let stack_len = self.stack_frame()?.stack.len();
                    if stack_len < param_count {
                        return Err(RuntimeError::StackUnderflow);
                    }

                    let receiver_idx = stack_len - param_count;
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

                    // Look up the method in the vtable
                    let method_symbol_id = self
                        .vtable
                        .get(&(concrete_type_id, proto_id, method_hash))
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Method '{}' not found in vtable for type {} implementing proto {}",
                                method_name, concrete_type_id, proto_id
                            ))
                        })?;

                    // Now call the method using the standard Call instruction logic
                    let (address, args_len) = {
                        let symbol = self.modules[module_idx]
                            .symbols
                            .get(*method_symbol_id as usize)
                            .ok_or(RuntimeError::FunctionNotFound)?;

                        let (func_id, address) = match &symbol.kind {
                            crate::semantic::SymbolKind::Function { func_id, address } => {
                                (*func_id, *address)
                            }
                            _ => return Err(RuntimeError::FunctionNotFound),
                        };

                        let function_type = self.modules[module_idx]
                            .functions
                            .get(func_id as usize)
                            .ok_or(RuntimeError::FunctionNotFound)?;

                        let args_len = function_type.params.len();
                        (address, args_len)
                    };

                    let mut new_frame = StackFrame::new();
                    new_frame.module_idx = module_idx; // Stay in the same module
                    let stack = &mut self.stack_frame_mut()?.stack;
                    new_frame.params = stack.split_off(stack.len() - args_len);
                    new_frame.pc = address as usize;

                    self.stack.push(new_frame);
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
                    // Look up the module path and symbol name from string constants
                    let current_module_idx = self.stack_frame()?.module_idx;
                    let module_path = self.modules[current_module_idx]
                        .string_constants
                        .get(module_path_idx as usize)
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Invalid string constant index for module path: {}",
                                module_path_idx
                            ))
                        })?
                        .clone();

                    let symbol_name = self.modules[current_module_idx]
                        .string_constants
                        .get(symbol_name_idx as usize)
                        .ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Invalid string constant index for symbol name: {}",
                                symbol_name_idx
                            ))
                        })?
                        .clone();

                    // Look up the module
                    let target_module_idx =
                        *self.imported_modules.get(&module_path).ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Module '{}' not found in imported modules",
                                module_path
                            ))
                        })?;

                    // Find the function symbol in the imported module
                    let module = &self.modules[target_module_idx];
                    let root_symbol = module.symbols.first().ok_or_else(|| {
                        RuntimeError::Other(format!("Module '{}' has no root symbol", module_path))
                    })?;

                    // Support dotted names for method calls (e.g. "Tab.load_into")
                    let symbol = if symbol_name.contains('.') {
                        let parts: Vec<&str> = symbol_name.splitn(2, '.').collect();
                        let type_sym_id = root_symbol.children.get(parts[0]).ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Type '{}' not found in module '{}'",
                                parts[0], module_path
                            ))
                        })?;
                        let type_sym = module.symbols.get(*type_sym_id as usize).ok_or_else(|| {
                            RuntimeError::Other(format!("Invalid symbol id: {}", type_sym_id))
                        })?;
                        let method_sym_id = type_sym.children.get(parts[1]).ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Method '{}' not found on type '{}' in module '{}'",
                                parts[1], parts[0], module_path
                            ))
                        })?;
                        module.symbols.get(*method_sym_id as usize).ok_or_else(|| {
                            RuntimeError::Other(format!("Invalid symbol id: {}", method_sym_id))
                        })?
                    } else {
                        let symbol_id = root_symbol.children.get(&symbol_name).ok_or_else(|| {
                            RuntimeError::Other(format!(
                                "Symbol '{}' not found in module '{}'",
                                symbol_name, module_path
                            ))
                        })?;
                        module.symbols.get(*symbol_id as usize).ok_or_else(|| {
                            RuntimeError::Other(format!("Invalid symbol id: {}", symbol_id))
                        })?
                    };

                    // Extract function information
                    let (func_id, address) = match &symbol.kind {
                        crate::semantic::SymbolKind::Function { func_id, address } => {
                            (*func_id, *address)
                        }
                        _ => {
                            return Err(RuntimeError::Other(format!(
                                "Symbol '{}' in module '{}' is not a function",
                                symbol_name, module_path
                            )));
                        }
                    };

                    let function_def = module.functions.get(func_id as usize).ok_or_else(|| {
                        RuntimeError::Other(format!("Function definition not found: {}", func_id))
                    })?;

                    let args_len = function_def.params.len();

                    // Create new stack frame with arguments
                    let mut new_frame = StackFrame::new();
                    new_frame.module_idx = target_module_idx; // Execute in the context of the target module
                    let stack = &mut self.stack_frame_mut()?.stack;
                    new_frame.params = stack.split_off(stack.len() - args_len);
                    new_frame.pc = address as usize;

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
                    self.stack_frame_mut()?
                        .stack
                        .push(Data::Some(Box::new(value)));
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
                            self.stack_frame_mut()?.stack.push(*value);
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
                            self.stack_frame_mut()?.stack.push(*value);
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
                    self.stack_frame_mut()?.stack.push(Data::Closure {
                        func_addr,
                        captures,
                    });
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
                        Data::Closure { func_addr, captures } => {
                            let current_module_idx =
                                self.stack.last().map(|f| f.module_idx).unwrap_or(0);
                            // Create new frame. Params = captures + args
                            let mut params = captures;
                            params.extend(args);
                            let frame = StackFrame {
                                params,
                                locals: Vec::new(),
                                stack: Vec::new(),
                                pc: func_addr as usize,
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

    fn binary_op_int<F>(&mut self, op: F) -> ContextResult<()>
    where
        F: Fn(i64, i64) -> i64,
    {
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
                self.stack_frame_mut()?.stack.push(Data::Int(op(a, b)));
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
