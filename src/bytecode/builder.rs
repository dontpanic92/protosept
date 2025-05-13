use crate::bytecode::OpCode;

pub struct ByteCodeBuilder {
    bytecode: Vec<u8>,
}

impl ByteCodeBuilder {
    pub fn new() -> Self {
        ByteCodeBuilder {
            bytecode: Vec::new(),
        }
    }

    pub fn next_address(&self) -> u32 {
        self.bytecode.len() as u32
    }

    pub fn add_instruction(&mut self, opcode: OpCode) {
        self.bytecode.push(opcode as u8);
    }

    pub fn ldi(&mut self, value: i32) {
        self.add_instruction(OpCode::LDI);
        self.bytecode.extend_from_slice(&value.to_le_bytes());
    }

    pub fn ldf(&mut self, value: f32) {
        self.add_instruction(OpCode::LDF);
        self.bytecode.extend_from_slice(&value.to_le_bytes());
    }

    pub fn ldvar(&mut self, var_id: u32) {
        self.add_instruction(OpCode::LDVAR);
        self.bytecode.extend_from_slice(&var_id.to_le_bytes());
    }

    pub fn stvar(&mut self, var_id: u32) {
        self.add_instruction(OpCode::STVAR);
        self.bytecode.extend_from_slice(&var_id.to_le_bytes());
    }

    pub fn addi(&mut self) {
        self.add_instruction(OpCode::ADDI);
    }

    pub fn subi(&mut self) {
        self.add_instruction(OpCode::SUBI);
    }

    pub fn muli(&mut self) {
        self.add_instruction(OpCode::MULI);
    }

    pub fn divi(&mut self) {
        self.add_instruction(OpCode::DIVI);
    }

    pub fn modi(&mut self) {
        self.add_instruction(OpCode::MOD);
    }

    pub fn addf(&mut self) {
        self.add_instruction(OpCode::ADDF);
    }

    pub fn subf(&mut self) {
        self.add_instruction(OpCode::SUBF);
    }

    pub fn mulf(&mut self) {
        self.add_instruction(OpCode::MULF);
    }

    pub fn divf(&mut self) {
        self.add_instruction(OpCode::DIVF);
    }

    pub fn neg(&mut self) {
        self.add_instruction(OpCode::NEG);
    }

    pub fn and(&mut self) {
        self.add_instruction(OpCode::AND);
    }

    pub fn or(&mut self) {
        self.add_instruction(OpCode::OR);
    }

    pub fn not(&mut self) {
        self.add_instruction(OpCode::NOT);
    }

    pub fn eq(&mut self) {
        self.add_instruction(OpCode::EQ);
    }

    pub fn neq(&mut self) {
        self.add_instruction(OpCode::NEQ);
    }

    pub fn lt(&mut self) {
        self.add_instruction(OpCode::LT);
    }

    pub fn gt(&mut self) {
        self.add_instruction(OpCode::GT);
    }

    pub fn lte(&mut self) {
        self.add_instruction(OpCode::LTE);
    }

    pub fn gte(&mut self) {
        self.add_instruction(OpCode::GTE);
    }

    pub fn jmp(&mut self, address: u32) {
        self.add_instruction(OpCode::JMP);
        self.bytecode.extend_from_slice(&address.to_le_bytes());
    }

    pub fn jif(&mut self, address: u32) {
        self.add_instruction(OpCode::JIF);
        self.bytecode.extend_from_slice(&address.to_le_bytes());
    }

    pub fn call(&mut self, address: u32) {
        self.add_instruction(OpCode::CALL);
        self.bytecode.extend_from_slice(&address.to_le_bytes());
    }

    pub fn ret(&mut self) {
        self.add_instruction(OpCode::RET);
    }

    pub fn pop(&mut self) {
        self.add_instruction(OpCode::POP);
    }

    pub fn patch_jump_address(&mut self, instruction_address: u32, jump_target_address: u32) {
        let address_bytes = jump_target_address.to_le_bytes();
        let start_of_address = instruction_address as usize + 1;
        self.bytecode[start_of_address..start_of_address + 4].copy_from_slice(&address_bytes);
    }

    pub fn get_bytecode(&self) -> Vec<u8> {
        self.bytecode.clone()
    }
}
