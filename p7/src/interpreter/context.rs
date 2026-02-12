use core::panic;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;

use binrw::BinRead;

use crate::bytecode::{Instruction, Module};

use crate::errors::RuntimeError;
pub type ContextResult<T> = std::result::Result<T, RuntimeError>;

/// Type for host functions that can be called from p7 code
/// Takes a mutable reference to the context to access the stack
pub type HostFunction = fn(&mut Context) -> ContextResult<()>;

#[derive(Debug, Clone)]
pub enum Data {
    Int(i32),
    Float(f64),
    String(String),
    /// Reference to a heap-allocated struct (index into Context.heap).
    StructRef(u32),
    /// Reference to a heap-allocated box (index into Context.box_heap).
    /// For box<proto>, stores both the box index and the concrete type_id for dynamic dispatch.
    BoxRef(u32),
    /// Proto box reference: stores box index and concrete struct type_id for dynamic dispatch
    ProtoBoxRef {
        box_idx: u32,
        concrete_type_id: u32,
    },
    /// Proto ref reference: stores ref index and concrete struct type_id for dynamic dispatch
    ProtoRefRef {
        ref_idx: u32,
        concrete_type_id: u32,
    },
    /// Exception value (enum variant ID) - used for try-catch as special return value
    Exception(i32),
    /// Array value - immutable collection of Data values
    Array(Vec<Data>),
    /// Null value for nullable types
    Null,
    /// Some(value) for nullable types
    Some(Box<Data>),
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

impl From<String> for Data {
    fn from(value: String) -> Self {
        Data::String(value)
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
            (Data::String(_), _) | (_, Data::String(_)) => {
                // Arithmetic on strings is invalid.
                return Err(RuntimeError::Other("Arithmetic on string".to_string()));
            }
            (Data::StructRef(_), _) | (_, Data::StructRef(_)) => {
                // Arithmetic on struct references is invalid.
                return Err(RuntimeError::UnexpectedStructRef);
            }
            (Data::BoxRef(_), _) | (_, Data::BoxRef(_))
            | (Data::ProtoBoxRef { .. }, _) | (_, Data::ProtoBoxRef { .. })
            | (Data::ProtoRefRef { .. }, _) | (_, Data::ProtoRefRef { .. }) => {
                // Arithmetic on box/proto references is invalid.
                return Err(RuntimeError::Other("Arithmetic on box/proto reference".to_string()));
            }
            (Data::Exception(_), _) | (_, Data::Exception(_)) => {
                // Arithmetic on exceptions is invalid.
                return Err(RuntimeError::Other("Arithmetic on exception value".to_string()));
            }
            (Data::Array(_), _) | (_, Data::Array(_)) => {
                // Arithmetic on arrays is invalid.
                return Err(RuntimeError::Other("Arithmetic on array".to_string()));
            }
            (Data::Null, _) | (_, Data::Null) | (Data::Some(_), _) | (_, Data::Some(_)) => {
                // Arithmetic on nullable values is invalid.
                return Err(RuntimeError::Other("Arithmetic on nullable value".to_string()));
            }
        }
    };
}

macro_rules! comparison_op {
    ($self: ident, $op:tt) => {
        let b = $self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
        let a = $self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
        let is_equality_op = matches!(stringify!($op), "==" | "!=");
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
            (Data::String(a), Data::String(b)) if is_equality_op => {
                $self.stack_frame_mut()?.stack.push(Data::Int((a $op b) as i32));
            }
            (Data::String(_), _) | (_, Data::String(_)) => {
                return Err(RuntimeError::Other("Comparison on string".to_string()));
            }
            (Data::StructRef(_), _) | (_, Data::StructRef(_)) => {
                // Comparison with struct refs not supported
                return Err(RuntimeError::UnexpectedStructRef);
            }
            (Data::BoxRef(_), _) | (_, Data::BoxRef(_))
            | (Data::ProtoBoxRef { .. }, _) | (_, Data::ProtoBoxRef { .. })
            | (Data::ProtoRefRef { .. }, _) | (_, Data::ProtoRefRef { .. }) => {
                // Comparison with box/proto refs not supported
                return Err(RuntimeError::Other("Comparison on box/proto reference".to_string()));
            }
            (Data::Exception(_), _) | (_, Data::Exception(_)) => {
                // Comparison with exceptions not supported
                return Err(RuntimeError::Other("Comparison on exception value".to_string()));
            }
            (Data::Array(_), _) | (_, Data::Array(_)) => {
                // Comparison with arrays not supported
                return Err(RuntimeError::Other("Comparison on array".to_string()));
            }
            (Data::Null, _) | (_, Data::Null) | (Data::Some(_), _) | (_, Data::Some(_)) => {
                // Comparison with nullable values not supported directly
                return Err(RuntimeError::Other("Comparison on nullable value".to_string()));
            }
        }
    };
}

pub struct StackFrame {
    pub params: Vec<Data>,
    pub locals: Vec<Data>,
    pub stack: Vec<Data>,
    pub pc: usize,
    pub module_idx: usize, // Which module this frame is executing from
}

impl StackFrame {
    fn new() -> Self {
        Self {
            params: Vec::new(),
            locals: Vec::new(),
            stack: Vec::new(),
            pc: std::usize::MAX,
            module_idx: 0, // Default to main module
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
    // Vtable for dynamic dispatch: (concrete_type_id, proto_id, method_name_hash) -> symbol_id
    vtable: HashMap<(u32, u32, u32), u32>,
    // Host function registry: function_name -> host function
    host_functions: HashMap<String, HostFunction>,
    // Imported modules registry: module_path -> module_index in modules Vec
    imported_modules: HashMap<String, usize>,
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
        };

        // Register builtin host functions
        ctx.register_builtin_host_functions();
        ctx
    }

    /// Register all builtin host functions
    fn register_builtin_host_functions(&mut self) {
        super::builtin_impl::register_builtin_functions(self);
        super::std_impl::register_std_functions(self);
    }

    /// Register a custom host function
    pub fn register_host_function(&mut self, name: String, func: HostFunction) {
        self.host_functions.insert(name, func);
    }

    pub fn load_module(&mut self, module: Module) {
        // Push the main module first to ensure it's at index 0
        self.build_vtable(&module);

        // Extract imported modules before pushing the main module
        let imported_modules = module.imported_modules.clone();
        self.modules.push(module);

        // Now register and load all imported modules
        for (module_path, imported) in imported_modules {
            let imported_module_idx = self.modules.len();
            self.imported_modules
                .insert(module_path.clone(), imported_module_idx);
            self.load_module_internal(*imported);
        }
    }

    /// Helper to load a module and recursively load its dependencies.
    /// Registers each module in imported_modules if not already present.
    fn load_module_internal(&mut self, module: Module) {
        self.build_vtable(&module);

        // Extract imported modules before pushing this module
        let imported_modules = module.imported_modules.clone();
        self.modules.push(module);

        // Register imported modules of this module
        for (module_path, imported) in imported_modules {
            if !self.imported_modules.contains_key(&module_path) {
                let module_idx = self.modules.len();
                self.imported_modules
                    .insert(module_path.clone(), module_idx);
                self.load_module_internal(*imported);
            }
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
    fn hash_method_name(name: &str) -> u32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        hasher.finish() as u32
    }

    /// Get method name from hash for error messages (reverse lookup)
    fn get_method_name_from_hash(&self, proto_id: u32, method_hash: u32) -> ContextResult<String> {
        use crate::semantic::TypeDefinition;

        if let Some(TypeDefinition::Proto(proto)) = self.modules[0].types.get(proto_id as usize) {
            for (method_name, _, _) in &proto.methods {
                if Self::hash_method_name(method_name) == method_hash {
                    return Ok(method_name.clone());
                }
            }
        }
        Err(RuntimeError::Other(format!(
            "Method with hash {} not found in proto {}",
            method_hash, proto_id
        )))
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

        loop {
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
                            Data::String(_) => {
                                return Err(RuntimeError::Other(
                                    "Cannot negate string".to_string(),
                                ));
                            }
                            Data::StructRef(_) => {
                                return Err(RuntimeError::VariableNotFound);
                            }
                            Data::BoxRef(_)
                            | Data::ProtoBoxRef { .. }
                            | Data::ProtoRefRef { .. } => {
                                return Err(RuntimeError::Other(
                                    "Cannot negate box/proto reference".to_string(),
                                ));
                            }
                            Data::Exception(_) => {
                                return Err(RuntimeError::Other(
                                    "Cannot negate exception value".to_string(),
                                ));
                            }
                            Data::Array(_) => {
                                return Err(RuntimeError::Other("Cannot negate array".to_string()));
                            }
                            Data::Null | Data::Some(_) => {
                                return Err(RuntimeError::Other(
                                    "Cannot negate nullable value".to_string(),
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
                            Data::String(_) => {
                                return Err(RuntimeError::Other(
                                    "Cannot apply logical NOT to string".to_string(),
                                ));
                            }
                            Data::StructRef(_) => {
                                return Err(RuntimeError::VariableNotFound);
                            }
                            Data::BoxRef(_)
                            | Data::ProtoBoxRef { .. }
                            | Data::ProtoRefRef { .. } => {
                                return Err(RuntimeError::Other(
                                    "Cannot apply logical NOT to box/proto reference".to_string(),
                                ));
                            }
                            Data::Exception(_) => {
                                return Err(RuntimeError::Other(
                                    "Cannot negate exception value".to_string(),
                                ));
                            }
                            Data::Array(_) => {
                                return Err(RuntimeError::Other(
                                    "Cannot apply logical NOT to array".to_string(),
                                ));
                            }
                            Data::Null | Data::Some(_) => {
                                return Err(RuntimeError::Other(
                                    "Cannot apply logical NOT to nullable value".to_string(),
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
                    // Expect a StructRef, BoxRef, or ProtoRefRef on the stack; pop it and push the requested field value.
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
                                .ok_or(RuntimeError::Other(format!("Method not found in proto")))?
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
                    let root_symbol = module.symbols.get(0).ok_or_else(|| {
                        RuntimeError::Other(format!("Module '{}' has no root symbol", module_path))
                    })?;

                    let symbol_id = root_symbol.children.get(&symbol_name).ok_or_else(|| {
                        RuntimeError::Other(format!(
                            "Symbol '{}' not found in module '{}'",
                            symbol_name, module_path
                        ))
                    })?;

                    let symbol = module.symbols.get(*symbol_id as usize).ok_or_else(|| {
                        RuntimeError::Other(format!("Invalid symbol id: {}", symbol_id))
                    })?;

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

    pub fn stack_frame_mut(&mut self) -> ContextResult<&mut StackFrame> {
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
            Data::BoxRef(idx) | Data::ProtoBoxRef { box_idx: idx, .. } => {
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
        match data {
            Data::BoxRef(old_idx) => {
                match index_map.get(*old_idx as usize) {
                    Some(Some(new_idx)) => {
                        *old_idx = *new_idx;
                    }
                    Some(None) => {
                        // This box was garbage collected, but we're trying to update a reference to it.
                        // This should never happen if mark phase is correct.
                        panic!(
                            "BUG: Attempted to update reference to garbage-collected box at index {}",
                            old_idx
                        );
                    }
                    None => {
                        // Index out of bounds - this should never happen
                        panic!(
                            "BUG: BoxRef index {} is out of bounds (heap size: {})",
                            old_idx,
                            index_map.len()
                        );
                    }
                }
            }
            Data::ProtoBoxRef {
                box_idx: old_idx,
                concrete_type_id: _concrete_type_id,
            } => match index_map.get(*old_idx as usize) {
                Some(Some(new_idx)) => {
                    *old_idx = *new_idx;
                }
                Some(None) => {
                    panic!(
                        "BUG: Attempted to update reference to garbage-collected proto box at index {}",
                        old_idx
                    );
                }
                None => {
                    panic!(
                        "BUG: ProtoBoxRef index {} is out of bounds (heap size: {})",
                        old_idx,
                        index_map.len()
                    );
                }
            },
            _ => {}
        }
    }
}
