use num_derive::FromPrimitive;

pub mod builder;
pub mod codegen;

#[repr(u8)]
#[derive(PartialEq, FromPrimitive)]
pub enum OpCode {
    // Load integer (4 bytes IMM) onto stack
    LDI = 0,

    // Load float (4 bytes IMM) onto stack
    LDF,

    // Load local variable (4 bytes var id) onto stack
    LDVAR,

    // Load local variable (4 bytes var id) onto stack
    STVAR,

    // Pop and add the 2 integers from the top of stack, push the result back
    ADDI,
    SUBI,
    MULI,
    DIVI,
    MOD,
    ADDF,
    SUBF,
    MULF,
    DIVF,
    NEG,
    AND,
    OR,
    NOT,
    EQ,
    NEQ,
    LT,
    GT,
    LTE,
    GTE,
    JMP,
    JIF,

    // Call the function at address
    CALL,
    RET,
    POP,
    THROW,
}
