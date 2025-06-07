pub mod builder;
pub mod codegen;

use binrw::{binrw, BinRead};

use crate::semantic::{Symbol, SymbolKind};

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum Instruction {
    #[brw(magic = 0u8)]
    Ldi(i32),

    #[brw(magic = 1u8)]
    Ldf(f64),

    #[brw(magic = 2u8)]
    Ldvar(u32),

    #[brw(magic = 3u8)]
    Stvar(u32),

    #[brw(magic = 4u8)]
    Ldpar(u32),

    #[brw(magic = 5u8)]
    Add,

    #[brw(magic = 6u8)]
    Sub,

    #[brw(magic = 7u8)]
    Mul,

    #[brw(magic = 8u8)]
    Div,

    #[brw(magic = 9u8)]
    Mod,

    #[brw(magic = 10u8)]
    Neg,

    #[brw(magic = 11u8)]
    And,

    #[brw(magic = 12u8)]
    Or,

    #[brw(magic = 13u8)]
    Not,

    #[brw(magic = 14u8)]
    Eq,

    #[brw(magic = 15u8)]
    Neq,

    #[brw(magic = 16u8)]
    Lt,

    #[brw(magic = 17u8)]
    Gt,

    #[brw(magic = 18u8)]
    Lte,

    #[brw(magic = 19u8)]
    Gte,

    #[brw(magic = 20u8)]
    Jmp(u32),

    #[brw(magic = 21u8)]
    Jif(u32),

    #[brw(magic = 22u8)]
    Call(u32),

    #[brw(magic = 23u8)]
    Ret,

    #[brw(magic = 24u8)]
    Pop,

    #[brw(magic = 25u8)]
    Throw,
}

pub fn disassemble(instructions: &[u8]) -> Vec<Instruction> {
    let mut cursor = std::io::Cursor::new(instructions);
    let mut insts = Vec::new();

    while cursor.position() < instructions.len() as u64 {
        match Instruction::read(&mut cursor) {
            Ok(inst) => insts.push(inst),
            Err(e) => {
                eprintln!("Error reading instruction: {}", e);
                break;
            }
        }
    }

    insts
}

#[derive(Debug)]
pub struct Module {
    pub instructions: Vec<u8>,
    pub symbols: Vec<Symbol>,
    pub types: Vec<crate::semantic::UserDefinedType>,
}

impl Module {
    pub fn get_function(&self, name: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|sym| {
            sym.name == name
                && matches!(
                    sym.kind,
                    SymbolKind::Function { .. }
                )
        })
    }
}
