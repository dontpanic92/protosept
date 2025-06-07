use std::io::Cursor;

use binrw::BinRead;

use crate::bytecode::{Instruction, Module};

#[derive(Debug)]
pub enum ContextError {
    NoStackFrame,
}

pub type ContextResult<T> = std::result::Result<T, ContextError>;

#[derive(Debug)]
pub enum Data {
    Int(i32),
    Float(f64),
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
            stack: Vec::new(),
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
                        self.stack_frame_mut()?
                            .stack
                            .push(local);
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
                Instruction::Addi => self.binary_op_int(|a, b| a + b)?,
                Instruction::Subi => self.binary_op_int(|a, b| a - b)?,
                Instruction::Muli => self.binary_op_int(|a, b| a * b)?,
                Instruction::Divi => self.binary_op_int(|a, b| a / b)?,
                Instruction::Mod => self.binary_op_int(|a, b| a % b)?,
                Instruction::Addf => self.binary_op_float(|a, b| a + b)?,
                Instruction::Subf => self.binary_op_float(|a, b| a - b)?,
                Instruction::Mulf => self.binary_op_float(|a, b| a * b)?,
                Instruction::Divf => self.binary_op_float(|a, b| a / b)?,
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
                Instruction::Eq => self.comparison_op(|a, b| a == b)?,
                Instruction::Neq => self.comparison_op(|a, b| a != b)?,
                Instruction::Lt => self.comparison_op(|a, b| a < b)?,
                Instruction::Gt => self.comparison_op(|a, b| a > b)?,
                Instruction::Lte => self.comparison_op(|a, b| a <= b)?,
                Instruction::Gte => self.comparison_op(|a, b| a >= b)?,
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
                    unimplemented!();
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

    fn binary_op_float<F>(&mut self, op: F) -> ContextResult<()>
    where
        F: Fn(f64, f64) -> f64,
    {
        if let (Some(Data::Float(b)), Some(Data::Float(a))) = (
            self.stack_frame_mut()?.stack.pop(),
            self.stack_frame_mut()?.stack.pop(),
        ) {
            self.stack_frame_mut()?.stack.push(Data::Float(op(a, b)));
        } else {
            // Handle error: stack underflow or invalid types
        }
        
        Ok(())
    }

    fn comparison_op<F>(&mut self, op: F) -> ContextResult<()>
    where
        F: Fn(Data, Data) -> bool,
    {
        if let (Some(b), Some(a)) = (
            self.stack_frame_mut()?.stack.pop(),
            self.stack_frame_mut()?.stack.pop(),
        ) {
            match (a, b) {
                (Data::Int(a_i), Data::Int(b_i)) => self
                    .stack_frame_mut()?
                    .stack
                    .push(Data::Int(op(Data::Int(a_i), Data::Int(b_i)) as i32)),
                (Data::Float(a_f), Data::Float(b_f)) => self
                    .stack_frame_mut()?
                    .stack
                    .push(Data::Int(op(Data::Float(a_f), Data::Float(b_f)) as i32)),
                (Data::Int(a_i), Data::Float(b_f)) => {
                    self.stack_frame_mut()?
                        .stack
                        .push(Data::Int(
                            op(Data::Float(a_i as f64), Data::Float(b_f)) as i32
                        ))
                }
                (Data::Float(a_f), Data::Int(b_i)) => {
                    self.stack_frame_mut()?
                        .stack
                        .push(Data::Int(
                            op(Data::Float(a_f), Data::Float(b_i as f64)) as i32
                        ))
                }
            }
        } else {
            unimplemented!();
        }

        Ok(())
    }
}

impl Clone for Data {
    fn clone(&self) -> Self {
        match self {
            Data::Int(i) => Data::Int(*i),
            Data::Float(f) => Data::Float(*f),
        }
    }
}

impl PartialEq for Data {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Data::Int(a), Data::Int(b)) => a == b,
            (Data::Float(a), Data::Float(b)) => a == b,
            (Data::Int(a), Data::Float(b)) => (*a as f64) == *b,
            (Data::Float(a), Data::Int(b)) => *a == (*b as f64),
        }
    }
}

impl PartialOrd for Data {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Data::Int(a), Data::Int(b)) => a.partial_cmp(b),
            (Data::Float(a), Data::Float(b)) => a.partial_cmp(b),
            (Data::Int(a), Data::Float(b)) => (*a as f64).partial_cmp(b),
            (Data::Float(a), Data::Int(b)) => a.partial_cmp(&(*b as f64)),
        }
    }
}
