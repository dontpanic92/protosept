use core::panic;
use std::io::Cursor;

use binrw::BinRead;

use crate::bytecode::{Instruction, Module};

#[derive(Debug)]
pub enum ContextError {
    NoStackFrame,
    StackUnderflow,
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
        let a = $self.stack_frame_mut()?.stack.pop().ok_or(ContextError::StackUnderflow)?;
        let b = $self.stack_frame_mut()?.stack.pop().ok_or(ContextError::StackUnderflow)?;
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
}

impl StackFrame {
    fn new() -> Self {
        Self {
            params: Vec::new(),
            locals: Vec::new(),
            stack: Vec::new(),
        }
    }
}

pub struct Context {
    pub stack: Vec<StackFrame>,
    pc: usize,
    modules: Vec<Module>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            stack: vec![StackFrame::new()],
            pc: 0,
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

        let mut stack_frame = StackFrame::new();
        stack_frame.params = params;
        self.stack.push(stack_frame);

        let addr = self.modules[0]
            .get_function(name)
            .unwrap()
            .get_function_address()
            .unwrap();

        self.pc = addr as usize;
    }

    pub fn resume(&mut self) -> ContextResult<()> {
        while self.pc < self.modules[0].instructions.len() {
            let mut reader = Cursor::new(&self.modules[0].instructions[self.pc..]);
            let instruction = Instruction::read(&mut reader).unwrap();

            self.pc += reader.position() as usize;

            match instruction {
                Instruction::Ldi(val) => self.stack_frame_mut()?.stack.push(Data::Int(val)),
                Instruction::Ldf(val) => self.stack_frame_mut()?.stack.push(Data::Float(val)),
                Instruction::Ldvar(idx) => {
                    if (idx as usize) < self.stack_frame_mut()?.locals.len() {
                        let local = self.stack_frame_mut()?.locals[idx as usize].clone();
                        self.stack_frame_mut()?.stack.push(local);
                    } else {
                        // Handle error: variable index out of bounds
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
                        unimplemented!();
                    }
                }
                Instruction::Addi => {
                    arithmetic_op!(self, +);
                }
                Instruction::Subi => {
                    arithmetic_op!(self, -);
                }
                Instruction::Muli => {
                    arithmetic_op!(self, *);
                }
                Instruction::Divi => {
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
                Instruction::Jmp(addr) => self.pc = addr as usize,
                Instruction::Jif(addr) => {
                    if let Some(Data::Int(condition)) = self.stack_frame_mut()?.stack.pop() {
                        if condition != 0 {
                            self.pc = addr as usize;
                        }
                    } else {
                        unimplemented!();
                    }
                }
                Instruction::Call(_) => {
                    unimplemented!();
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
                    self.stack.pop();
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
