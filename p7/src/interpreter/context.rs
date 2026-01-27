use core::panic;
use std::collections::HashSet;
use std::io::Cursor;

use binrw::BinRead;

use crate::bytecode::{Instruction, Module};

use crate::errors::RuntimeError;
pub type ContextResult<T> = std::result::Result<T, RuntimeError>;

#[derive(Debug, Clone)]
pub enum Data {
    Int(i32),
    Float(f64),
    /// Reference to a heap-allocated struct (index into Context.heap).
    StructRef(u32),
    /// Reference to a heap-allocated box (index into Context.box_heap).
    BoxRef(u32),
    /// Exception value (enum variant ID) - used for try-catch as special return value
    Exception(i32),
}

impl From<i32> for Data {
    fn from(value: i32) -> Self {
        Data::Int(value)
    }
}

impl From<f64> for Data {
    fn from(value: f64) -> Self {
        Data::Float(value)
    }
}

macro_rules! arithmetic_op {
    ($self: ident, $op:tt) => {
        let b = $self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
        let a = $self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
        match (a, b) {
            (Data::Int(a), Data::Int(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Int(a $op b));
            }
            (Data::Float(a), Data::Float(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Float(a $op b));
            }
            (Data::Int(a), Data::Float(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Float((a as f64) $op b));
            }
            (Data::Float(a), Data::Int(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Float(a $op (b as f64)));
            }
            (Data::StructRef(_), _) | (_, Data::StructRef(_)) => {
                // Arithmetic on struct references is invalid.
                return Err(RuntimeError::UnexpectedStructRef);
            }
            (Data::BoxRef(_), _) | (_, Data::BoxRef(_)) => {
                // Arithmetic on box references is invalid.
                return Err(RuntimeError::Other("Arithmetic on box reference".to_string()));
            }
            (Data::Exception(_), _) | (_, Data::Exception(_)) => {
                // Arithmetic on exceptions is invalid.
                return Err(RuntimeError::Other("Arithmetic on exception value".to_string()));
            }
        }
    };
}

macro_rules! comparison_op {
    ($self: ident, $op:tt) => {
        let b = $self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
        let a = $self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
        match (a, b) {
            (Data::Int(a), Data::Int(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Int((a $op b) as i32));
            }
            (Data::Float(a), Data::Float(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Int((a $op b) as i32));
            }
            (Data::Int(a), Data::Float(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Int(((a as f64) $op b) as i32));
            }
            (Data::Float(a), Data::Int(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Int((a $op (b as f64)) as i32));
            }
            (Data::StructRef(_), _) | (_, Data::StructRef(_)) => {
                // Comparison with struct refs not supported
                return Err(RuntimeError::UnexpectedStructRef);
            }
            (Data::BoxRef(_), _) | (_, Data::BoxRef(_)) => {
                // Comparison with box refs not supported
                return Err(RuntimeError::Other("Comparison on box reference".to_string()));
            }
            (Data::Exception(_), _) | (_, Data::Exception(_)) => {
                // Comparison with exceptions not supported
                return Err(RuntimeError::Other("Comparison on exception value".to_string()));
            }
        }
    };
}

pub struct StackFrame {
    pub params: Vec<Data>,
    pub locals: Vec<Data>,
    pub stack: Vec<Data>,
    pub pc: usize,
}

impl StackFrame {
    fn new() -> Self {
        Self {
            params: Vec::new(),
            locals: Vec::new(),
            stack: Vec::new(),
            pc: std::usize::MAX,
        }
    }
}

pub struct Struct {
    pub fields: Vec<Data>,
}

pub struct Context {
    pub stack: Vec<StackFrame>,
    modules: Vec<Module>,
    pub heap: Vec<Struct>,
    pub box_heap: Vec<Data>,
    // GC state
    allocation_count: usize,
    gc_threshold: usize,
}

impl Context {
    pub fn new() -> Self {
        Self {
            stack: vec![StackFrame::new()],
            modules: Vec::new(),
            heap: Vec::new(),
            box_heap: Vec::new(),
            allocation_count: 0,
            gc_threshold: 100, // Run GC after every 100 allocations
        }
    }

    pub fn load_module(&mut self, module: Module) {
        self.modules.push(module);
    }

    pub fn push_function(&mut self, name: &str, params: Vec<Data>) {
        if self.modules.len() == 0 {
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

        while self.stack_frame()?.pc < self.modules[0].instructions.len() {
            let pc = self.stack_frame()?.pc;
            let mut reader = Cursor::new(&self.modules[0].instructions[pc..]);
            let instruction = Instruction::read(&mut reader).unwrap();

            self.stack_frame_mut()?.pc += reader.position() as usize;

            match instruction {
                Instruction::Ldi(val) => self.stack_frame_mut()?.stack.push(Data::Int(val)),
                Instruction::Ldf(val) => self.stack_frame_mut()?.stack.push(Data::Float(val)),
                Instruction::Ldvar(idx) => {
                    if (idx as usize) < self.stack_frame_mut()?.locals.len() {
                        let local = self.stack_frame_mut()?.locals[idx as usize].clone();
                        self.stack_frame_mut()?.stack.push(local);
                    } else {
                        return Err(RuntimeError::VariableNotFound);
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
                    if (param_id as usize) < self.stack_frame_mut()?.params.len() {
                        let param = self.stack_frame_mut()?.params[param_id as usize].clone();
                        self.stack_frame_mut()?.stack.push(param);
                    } else {
                        return Err(RuntimeError::VariableNotFound);
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
                Instruction::Neg => {
                    if let Some(data) = self.stack_frame_mut()?.stack.pop() {
                        match data {
                            Data::Int(i) => self.stack_frame_mut()?.stack.push(Data::Int(-i)),
                            Data::Float(f) => self.stack_frame_mut()?.stack.push(Data::Float(-f)),
                            Data::StructRef(_) => {
                                return Err(RuntimeError::VariableNotFound);
                            }
                            Data::BoxRef(_) => {
                                return Err(RuntimeError::Other(
                                    "Cannot negate box reference".to_string(),
                                ));
                            }
                            Data::Exception(_) => {
                                return Err(RuntimeError::Other(
                                    "Cannot negate exception value".to_string(),
                                ));
                            }
                        }
                    } else {
                        unimplemented!();
                    }
                }
                Instruction::And => self.binary_op_int(|a, b| (a != 0 && b != 0) as i32)?,
                Instruction::Or => self.binary_op_int(|a, b| (a != 0 || b != 0) as i32)?,
                Instruction::Not => {
                    if let Some(data) = self.stack_frame_mut()?.stack.pop() {
                        match data {
                            Data::Int(i) => self
                                .stack_frame_mut()?
                                .stack
                                .push(Data::Int((i == 0) as i32)),
                            Data::Float(f) => self
                                .stack_frame_mut()?
                                .stack
                                .push(Data::Int((f == 0.0) as i32)),
                            Data::StructRef(_) => {
                                return Err(RuntimeError::VariableNotFound);
                            }
                            Data::BoxRef(_) => {
                                return Err(RuntimeError::Other(
                                    "Cannot apply logical NOT to box reference".to_string(),
                                ));
                            }
                            Data::Exception(_) => {
                                return Err(RuntimeError::Other(
                                    "Cannot negate exception value".to_string(),
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
                    let (address, args_len) = {
                        let function = self.modules[0]
                            .symbols
                            .get(symbol_id as usize)
                            .ok_or(RuntimeError::FunctionNotFound)?;

                        let address = function
                            .get_function_address()
                            .ok_or(RuntimeError::FunctionNotFound)?;

                        let udt = function
                            .get_type_id()
                            .and_then(|function_type_id| {
                                self.modules[0].types.get(function_type_id as usize)
                            })
                            .ok_or(RuntimeError::FunctionNotFound)?;

                        let function_type = match udt {
                            crate::semantic::UserDefinedType::Function(function_type) => {
                                function_type
                            }
                            _ => return Err(RuntimeError::FunctionNotFound),
                        };

                        let args_len = function_type.params.len();
                        (address, args_len)
                    };

                    let mut new_frame = StackFrame::new();
                    let stack = &mut self.stack_frame_mut()?.stack;
                    new_frame.params = stack.split_off(stack.len() - args_len);
                    new_frame.pc = address as usize;

                    self.stack.push(new_frame);
                }
                Instruction::Ldfield(field_idx) => {
                    // Expect a StructRef on the stack; pop it and push the requested field value.
                    if let Some(data) = self.stack_frame_mut()?.stack.pop() {
                        match data {
                            Data::StructRef(ref_id) => {
                                let ref_usize = ref_id as usize;
                                if ref_usize >= self.heap.len() {
                                    return Err(RuntimeError::VariableNotFound);
                                }
                                let struct_fields = &self.heap[ref_usize].fields;
                                if (field_idx as usize) >= struct_fields.len() {
                                    return Err(RuntimeError::VariableNotFound);
                                }
                                let field_value = struct_fields[field_idx as usize].clone();
                                self.stack_frame_mut()?.stack.push(field_value);
                            }
                            _ => {
                                return Err(RuntimeError::VariableNotFound);
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
                    match struct_ref_opt.unwrap() {
                        Data::StructRef(ref_id) => {
                            let ref_usize = ref_id as usize;
                            if ref_usize >= self.heap.len() {
                                return Err(RuntimeError::VariableNotFound);
                            }
                            if (field_idx as usize) >= self.heap[ref_usize].fields.len() {
                                return Err(RuntimeError::VariableNotFound);
                            }
                            self.heap[ref_usize].fields[field_idx as usize] = field_value;
                        }
                        _ => {
                            return Err(RuntimeError::VariableNotFound);
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
                    if let Some(value) = return_value {
                        if let Ok(frame) = self.stack_frame_mut() {
                            frame.stack.push(value);
                        }
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
                    let value = self.stack_frame_mut()?.stack.pop()
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
                    let box_ref = self.stack_frame_mut()?.stack.pop()
                        .ok_or(RuntimeError::StackUnderflow)?;
                    if let Data::BoxRef(idx) = box_ref {
                        let value = self.box_heap.get(idx as usize)
                            .ok_or_else(|| RuntimeError::Other(
                                format!("Invalid box reference: {}", idx)
                            ))?.clone();
                        self.stack_frame_mut()?.stack.push(value);
                    } else {
                        return Err(RuntimeError::Other(
                            "Expected BoxRef on stack for deref".to_string()
                        ));
                    }
                }
            }
        }

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

    fn stack_frame(&self) -> ContextResult<&StackFrame> {
        self.stack.last().ok_or(RuntimeError::NoStackFrame)
    }

    fn stack_frame_mut(&mut self) -> ContextResult<&mut StackFrame> {
        self.stack.last_mut().ok_or(RuntimeError::NoStackFrame)
    }

    fn binary_op_int<F>(&mut self, op: F) -> ContextResult<()>
    where
        F: Fn(i32, i32) -> i32,
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

    /// Mark-and-sweep garbage collector for box heap
    /// This method performs a full GC cycle
    pub fn collect_garbage(&mut self) {
        // Mark phase: identify all reachable boxes
        let mut marked = HashSet::new();
        self.mark_reachable(&mut marked);

        // Sweep phase: remove unreachable boxes and compact the heap
        self.sweep(&marked);
    }

    /// Mark phase: traverse all roots and mark reachable boxes
    fn mark_reachable(&self, marked: &mut HashSet<u32>) {
        // Mark from all stack frames
        for frame in &self.stack {
            // Mark boxes in evaluation stack
            for data in &frame.stack {
                self.mark_data(data, marked);
            }
            // Mark boxes in local variables
            for data in &frame.locals {
                self.mark_data(data, marked);
            }
            // Mark boxes in parameters
            for data in &frame.params {
                self.mark_data(data, marked);
            }
        }

        // Mark from heap-allocated structs (they may contain box references)
        for struct_obj in &self.heap {
            for data in &struct_obj.fields {
                self.mark_data(data, marked);
            }
        }
    }

    /// Recursively mark a data value and any boxes it references
    fn mark_data(&self, data: &Data, marked: &mut HashSet<u32>) {
        match data {
            Data::BoxRef(idx) => {
                // If we haven't marked this box yet, mark it and recursively mark its contents
                if marked.insert(*idx) {
                    // Get the box contents and recursively mark
                    if let Some(box_data) = self.box_heap.get(*idx as usize) {
                        self.mark_data(box_data, marked);
                    }
                }
            }
            Data::StructRef(idx) => {
                // Structs on the heap are always reachable (they're not GC'd)
                // But we need to mark any boxes they contain
                if let Some(struct_obj) = self.heap.get(*idx as usize) {
                    for field_data in &struct_obj.fields {
                        self.mark_data(field_data, marked);
                    }
                }
            }
            // Other data types don't contain references
            _ => {}
        }
    }

    /// Sweep phase: remove unmarked boxes and update all references
    fn sweep(&mut self, marked: &HashSet<u32>) {
        // Build a mapping from old indices to new indices
        let mut index_map: Vec<Option<u32>> = vec![None; self.box_heap.len()];
        let mut new_heap = Vec::new();
        let mut new_idx = 0u32;

        for (old_idx, box_data) in self.box_heap.iter().enumerate() {
            if marked.contains(&(old_idx as u32)) {
                // This box is reachable, keep it
                index_map[old_idx] = Some(new_idx);
                new_heap.push(box_data.clone());
                new_idx += 1;
            }
            // Otherwise, this box is garbage and will be removed
        }

        // Replace the old heap with the compacted heap
        self.box_heap = new_heap;

        // Update all BoxRef references to point to new indices
        self.update_box_refs(&index_map);
    }

    /// Update all BoxRef references after compaction
    fn update_box_refs(&mut self, index_map: &[Option<u32>]) {
        // Update references in stack frames
        for frame in &mut self.stack {
            Self::update_data_vec(&mut frame.stack, index_map);
            Self::update_data_vec(&mut frame.locals, index_map);
            Self::update_data_vec(&mut frame.params, index_map);
        }

        // Update references in heap structs
        for struct_obj in &mut self.heap {
            Self::update_data_vec(&mut struct_obj.fields, index_map);
        }

        // Update references in box_heap itself (boxes can contain boxes)
        for box_data in &mut self.box_heap {
            Self::update_data(box_data, index_map);
        }
    }

    /// Update a vector of Data values with new box indices
    fn update_data_vec(data_vec: &mut [Data], index_map: &[Option<u32>]) {
        for data in data_vec {
            Self::update_data(data, index_map);
        }
    }

    /// Update a single Data value with new box index
    fn update_data(data: &mut Data, index_map: &[Option<u32>]) {
        if let Data::BoxRef(old_idx) = data {
            if let Some(Some(new_idx)) = index_map.get(*old_idx as usize) {
                *old_idx = *new_idx;
            }
        }
    }
}
