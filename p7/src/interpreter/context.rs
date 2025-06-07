use core::panic;
use std::io::Cursor;

use binrw::BinRead;

use crate::bytecode::{Instruction, Module};

#[derive(Debug)]
pub enum ContextError {
    NoStackFrame,
    EntryPointNotFound,
    StackUnderflow,
    FunctionNotFound,
    VariableNotFound,
}

pub type ContextResult<T> = std::result::Result<T, ContextError>;

#[derive(Debug, Clone)]
pub enum Data {
    Int(i32),
    Float(f64),
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

macro_rules! binary_op {
    ($self: ident, $op:tt, $int_res_ty:ident, $float_res_ty:ident) => {
        let b = $self.stack_frame_mut()?.stack.pop().ok_or(ContextError::StackUnderflow)?;
        let a = $self.stack_frame_mut()?.stack.pop().ok_or(ContextError::StackUnderflow)?;
        match (a, b) {
            (Data::Int(a), Data::Int(b)) => $self.stack_frame_mut()?.stack.push(Data::from((a $op b) as $int_res_ty)),
            (Data::Float(a), Data::Float(b)) => $self.stack_frame_mut()?.stack.push(Data::from((a $op b) as $float_res_ty)),
            _ => {
                panic!("Invalid types for binary operation");
            }
        }
    };
}

macro_rules! arithmetic_op {
    ($self: ident, $op:tt) => {
        binary_op!($self, $op, i32, f64);
    };
}

macro_rules! comparison_op {
    ($self: ident, $op:tt) => {
        binary_op!($self, $op, i32, i32);
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

pub struct Context {
    pub stack: Vec<StackFrame>,
    modules: Vec<Module>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            stack: vec![StackFrame::new()],
            modules: Vec::new(),
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
            return Err(ContextError::EntryPointNotFound);
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
                        return Err(ContextError::VariableNotFound);
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
                        return Err(ContextError::StackUnderflow);
                    }
                }
                Instruction::Ldpar(param_id) => {
                    if (param_id as usize) < self.stack_frame_mut()?.params.len() {
                        let param = self.stack_frame_mut()?.params[param_id as usize].clone();
                        self.stack_frame_mut()?.stack.push(param);
                    } else {
                        return Err(ContextError::VariableNotFound);
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
                            .ok_or(ContextError::FunctionNotFound)?;

                        let address = function
                            .get_function_address()
                            .ok_or(ContextError::FunctionNotFound)?;

                        let udt = function
                            .get_type_id()
                            .and_then(|function_type_id| {
                                self.modules[0].types.get(function_type_id as usize)
                            })
                            .ok_or(ContextError::FunctionNotFound)?;

                        let function_type = match udt {
                            crate::semantic::UserDefinedType::Function(function_type) => {
                                function_type
                            }
                            _ => return Err(ContextError::FunctionNotFound),
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
        self.stack.last().ok_or(ContextError::NoStackFrame)
    }

    fn stack_frame_mut(&mut self) -> ContextResult<&mut StackFrame> {
        self.stack.last_mut().ok_or(ContextError::NoStackFrame)
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
