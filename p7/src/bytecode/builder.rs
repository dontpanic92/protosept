use std::io::{Cursor, Write};

use binrw::BinWrite;

use crate::bytecode::Instruction;

pub struct ByteCodeBuilder {
    writer: Cursor<Vec<u8>>,
}

impl Default for ByteCodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ByteCodeBuilder {
    pub fn new() -> Self {
        ByteCodeBuilder {
            writer: Cursor::new(Vec::new()),
        }
    }

    pub fn next_address(&self) -> u32 {
        self.writer.position() as u32
    }

    pub fn add_instruction(&mut self, inst: Instruction) {
        inst.write(&mut self.writer).unwrap();
    }

    pub fn ldi(&mut self, value: i64) {
        self.add_instruction(Instruction::Ldi(value));
    }

    pub fn ldf(&mut self, value: f64) {
        self.add_instruction(Instruction::Ldf(value));
    }

    pub fn lds(&mut self, string_index: u32) {
        self.add_instruction(Instruction::Lds(string_index));
    }

    pub fn ldvar(&mut self, var_id: u32) {
        self.add_instruction(Instruction::Ldvar(var_id));
    }

    pub fn stvar(&mut self, var_id: u32) {
        self.add_instruction(Instruction::Stvar(var_id));
    }

    pub fn ldpar(&mut self, param_id: u32) {
        self.add_instruction(Instruction::Ldpar(param_id));
    }

    /// Load a field from a struct value (by field index).
    pub fn ldfield(&mut self, field_index: u32) {
        self.add_instruction(Instruction::Ldfield(field_index));
    }

    /// Store a field into a struct value (by field index).
    pub fn stfield(&mut self, field_index: u32) {
        self.add_instruction(Instruction::Stfield(field_index));
    }

    /// Create a new struct on the heap with the given number of fields.
    /// Expects field values on the stack (in order: field0, field1, ..., fieldN).
    pub fn newstruct(&mut self, field_count: u32) {
        self.add_instruction(Instruction::NewStruct(field_count));
    }

    pub fn addi(&mut self) {
        self.add_instruction(Instruction::Add);
    }

    pub fn subi(&mut self) {
        self.add_instruction(Instruction::Sub);
    }

    pub fn muli(&mut self) {
        self.add_instruction(Instruction::Mul);
    }

    pub fn divi(&mut self) {
        self.add_instruction(Instruction::Div);
    }

    pub fn modi(&mut self) {
        self.add_instruction(Instruction::Mod);
    }

    pub fn bitand(&mut self) {
        self.add_instruction(Instruction::BitAnd);
    }

    pub fn bitor(&mut self) {
        self.add_instruction(Instruction::BitOr);
    }

    pub fn bitxor(&mut self) {
        self.add_instruction(Instruction::BitXor);
    }

    // pub fn addf(&mut self) {
    //     self.add_instruction(Instruction::Addf);
    // }

    // pub fn subf(&mut self) {
    //     self.add_instruction(Instruction::Subf);
    // }

    // pub fn mulf(&mut self) {
    //     self.add_instruction(Instruction::Mulf);
    // }

    // pub fn divf(&mut self) {
    //     self.add_instruction(Instruction::Divf);
    // }

    pub fn neg(&mut self) {
        self.add_instruction(Instruction::Neg);
    }

    pub fn and(&mut self) {
        self.add_instruction(Instruction::And);
    }

    pub fn or(&mut self) {
        self.add_instruction(Instruction::Or);
    }

    pub fn not(&mut self) {
        self.add_instruction(Instruction::Not);
    }

    pub fn eq(&mut self) {
        self.add_instruction(Instruction::Eq);
    }

    pub fn neq(&mut self) {
        self.add_instruction(Instruction::Neq);
    }

    pub fn lt(&mut self) {
        self.add_instruction(Instruction::Lt);
    }

    pub fn gt(&mut self) {
        self.add_instruction(Instruction::Gt);
    }

    pub fn lte(&mut self) {
        self.add_instruction(Instruction::Lte);
    }

    pub fn gte(&mut self) {
        self.add_instruction(Instruction::Gte);
    }

    pub fn jmp(&mut self, address: u32) {
        self.add_instruction(Instruction::Jmp(address));
    }

    pub fn jif(&mut self, address: u32) {
        self.add_instruction(Instruction::Jif(address));
    }

    pub fn call(&mut self, symbol_id: u32) {
        self.add_instruction(Instruction::Call(symbol_id));
    }

    pub fn throw(&mut self) {
        self.add_instruction(Instruction::Throw);
    }

    pub fn check_exception(&mut self, address: u32) {
        self.add_instruction(Instruction::CheckException(address));
    }

    pub fn unwrap_exception(&mut self) {
        self.add_instruction(Instruction::UnwrapException);
    }

    pub fn patch_jump_address(&mut self, instruction_address: u32, jump_target_address: u32) {
        let current_pos = self.writer.position();
        self.writer.set_position(instruction_address as u64 + 1);
        self.writer
            .write_all(&jump_target_address.to_le_bytes())
            .unwrap();
        self.writer.set_position(current_pos);
    }

    pub fn ret(&mut self) {
        self.add_instruction(Instruction::Ret);
    }

    pub fn pop(&mut self) {
        self.add_instruction(Instruction::Pop);
    }

    pub fn dup(&mut self) {
        self.add_instruction(Instruction::Dup);
    }

    /// Allocate a box on the heap and store the top stack value in it.
    pub fn box_alloc(&mut self) {
        self.add_instruction(Instruction::BoxAlloc);
    }

    /// Dereference a box and push its contained value.
    pub fn box_deref(&mut self) {
        self.add_instruction(Instruction::BoxDeref);
    }

    /// Call a host function by name.
    /// The function name is a string constant at the given index.
    pub fn call_host_function(&mut self, string_index: u32) {
        self.add_instruction(Instruction::InvokeHost(string_index));
    }

    /// Call an external function from another module.
    /// Both module_path_idx and symbol_name_idx are indices into the string constants table.
    pub fn call_external(&mut self, module_path_idx: u32, symbol_name_idx: u32) {
        self.add_instruction(Instruction::CallExternal(module_path_idx, symbol_name_idx));
    }

    /// Load null onto the stack.
    pub fn ldnull(&mut self) {
        self.add_instruction(Instruction::Ldnull);
    }

    /// Wrap a value into a nullable Some variant.
    pub fn wrap_nullable(&mut self) {
        self.add_instruction(Instruction::WrapNullable);
    }

    /// Check if nullable value is null.
    pub fn is_null(&mut self) {
        self.add_instruction(Instruction::IsNull);
    }

    /// Force unwrap: get inner value or trap if null.
    pub fn force_unwrap(&mut self) {
        self.add_instruction(Instruction::ForceUnwrap);
    }

    /// Null-coalescing: return value if Some, default if null.
    pub fn null_coalesce(&mut self) {
        self.add_instruction(Instruction::NullCoalesce);
    }

    pub fn make_closure(&mut self, func_addr: u32, capture_count: u32) {
        self.add_instruction(Instruction::MakeClosure(func_addr, capture_count));
    }

    pub fn call_closure(&mut self, arg_count: u32) {
        self.add_instruction(Instruction::CallClosure(arg_count));
    }

    pub fn ldmodvar(&mut self, var_id: u32) {
        self.add_instruction(Instruction::LdModVar(var_id));
    }

    pub fn stmodvar(&mut self, var_id: u32) {
        self.add_instruction(Instruction::StModVar(var_id));
    }

    pub fn ldextmodvar(&mut self, module_path_string_id: u32, var_name_string_id: u32) {
        self.add_instruction(Instruction::LdExtModVar(module_path_string_id, var_name_string_id));
    }

    pub fn stextmodvar(&mut self, module_path_string_id: u32, var_name_string_id: u32) {
        self.add_instruction(Instruction::StExtModVar(module_path_string_id, var_name_string_id));
    }

    pub fn get_bytecode(&self) -> Vec<u8> {
        self.writer.get_ref().clone()
    }
}
