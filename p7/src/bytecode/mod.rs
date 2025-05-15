pub mod builder;
pub mod codegen;

use binrw::binrw;

#[binrw]
#[brw(little)]
#[derive(Debug)]
pub enum Instruction {
    #[brw(magic = 0u8)]
    Ldi(u32),

    #[brw(magic = 1u8)]
    Ldf(f64),

    #[brw(magic = 2u8)]
    Ldvar(u32),

    #[brw(magic = 3u8)]
    Stvar(u32),

    #[brw(magic = 4u8)]
    Addi,

    #[brw(magic = 5u8)]
    Subi,

    #[brw(magic = 6u8)]
    Muli,

    #[brw(magic = 7u8)]
    Divi,

    #[brw(magic = 8u8)]
    Mod,

    #[brw(magic = 9u8)]
    Addf,

    #[brw(magic = 10u8)]
    Subf,

    #[brw(magic = 11u8)]
    Mulf,

    #[brw(magic = 12u8)]
    Divf,

    #[brw(magic = 13u8)]
    Neg,

    #[brw(magic = 14u8)]
    And,

    #[brw(magic = 15u8)]
    Or,

    #[brw(magic = 16u8)]
    Not,

    #[brw(magic = 17u8)]
    Eq,

    #[brw(magic = 18u8)]
    Neq,

    #[brw(magic = 19u8)]
    Lt,

    #[brw(magic = 20u8)]
    Gt,

    #[brw(magic = 21u8)]
    Lte,

    #[brw(magic = 22u8)]
    Gte,

    #[brw(magic = 23u8)]
    Jmp(u32),

    #[brw(magic = 24u8)]
    Jif(u32),

    #[brw(magic = 25u8)]
    Call(u32),

    #[brw(magic = 26u8)]
    Ret,

    #[brw(magic = 27u8)]
    Pop,

    #[brw(magic = 28u8)]
    Throw,
}
