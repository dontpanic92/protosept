use core::panic;
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
}

impl Context {
    pub fn new() -> Self {
        Self {
            stack: vec![StackFrame::new()],
            modules: Vec::new(),
            heap: Vec::new(),
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
            let mut reader = Cursor::new(&self.modules[0].instructions[self.stack_frame()?.pc..]);
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
                        }
                    } else {
                        unimplemented!();
                    }
                }
                Instruction::And => self.binary_op_int(|a, b| (a != 0 && b != 0) as i32)?,
                Instruction::Or => self.binary_op_int(|a, b| (a != 0 || b != 0) as i32)?,
                Instruction::Not => {
                    unimplemented!();
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

                        let args_len = function_type.args.len();
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
                Instruction::Ret => {
                    if self.stack_frame()?.stack.len() > 0 {
                        let return_value = self.stack_frame_mut()?.stack.pop();
                        self.stack.pop();
                        if let Some(value) = return_value {
                            self.stack_frame_mut()?.stack.push(value);
                        }
                    }
                }
                Instruction::Pop => {
                    self.stack_frame_mut()?.stack.pop();
                }
                Instruction::Throw => {
                    unimplemented!();
                }
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
        if let (Some(Data::Int(b)), Some(Data::Int(a))) = (
            self.stack_frame_mut()?.stack.pop(),
            self.stack_frame_mut()?.stack.pop(),
        ) {
            self.stack_frame_mut()?.stack.push(Data::Int(op(a, b)));
        } else {
            // Handle error: stack underflow or invalid types
        }

        Ok(())
    }
}
