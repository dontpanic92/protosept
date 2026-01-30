pub mod builder;
pub mod codegen;
mod helpers;
mod type_check;
mod monomorph;
mod stmt_gen;
mod expr_gen;

use binrw::{BinRead, binrw};

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

    // Duplicate the top value on the stack
    // Expects: [..., value] -> [..., value, value]
    #[brw(magic = 31u8)]
    Dup,

    #[brw(magic = 25u8)]
    Throw,

    // Check if top of stack is an exception. If so, jump to address.
    // Used in try-else blocks to detect exceptions.
    #[brw(magic = 26u8)]
    CheckException(u32),

    // Convert Exception back to its inner value for pattern matching.
    // Used in else blocks to extract the exception value.
    #[brw(magic = 27u8)]
    UnwrapException,

    // Load a field from a struct value on the stack.
    // Expects: [..., struct_value] -> pops struct_value and pushes the requested field value.
    #[brw(magic = 28u8)]
    Ldfield(u32),

    // Store a field into a struct value on the stack.
    // Expects: [..., struct_value, field_value] -> pops both and pushes updated struct_value.
    #[brw(magic = 29u8)]
    Stfield(u32),

    // Create a new struct on the heap.
    // Expects: [..., field0, field1, ..., fieldN] (N = field_count) on stack
    // Pops N field values, creates struct on heap, pushes StructRef
    #[brw(magic = 30u8)]
    NewStruct(u32),
    
    // Allocate a box on the heap and store the top stack value in it.
    // Expects: [..., value] -> pops value, allocates box, stores value, pushes BoxRef
    #[brw(magic = 32u8)]
    BoxAlloc,
    
    // Dereference a box and push its contained value.
    // Expects: [..., BoxRef] -> pops BoxRef, pushes the contained value
    #[brw(magic = 33u8)]
    BoxDeref,
    
    // Convert a box<T> to a proto box box<P> for dynamic dispatch.
    // Expects: [..., BoxRef] -> pops BoxRef, pushes ProtoBoxRef with type_id
    // Parameters: (struct_type_id, proto_type_id)
    #[brw(magic = 34u8)]
    BoxToProto(u32, u32),
    
    // Call a proto method with dynamic dispatch.
    // Expects: [..., ProtoBoxRef/ProtoRefRef, args] -> performs dynamic method lookup and calls impl
    // Parameters: (proto_id, method_name_hash)
    #[brw(magic = 35u8)]
    CallProtoMethod(u32, u32),
    
    // Convert a ref<T> to a proto ref ref<P> for dynamic dispatch.
    // Expects: [..., StructRef] -> pops StructRef, pushes ProtoRefRef with type_id
    // Parameters: (struct_type_id, proto_type_id)
    #[brw(magic = 36u8)]
    RefToProto(u32, u32),
    
    // Load a string constant from the string table.
    // Expects: [...] -> pushes string value
    // Parameters: string_index (index into Module.string_constants)
    #[brw(magic = 37u8)]
    Lds(u32),
    
    // Get the byte length of a string (UTF-8 encoded).
    // Expects: [..., string] -> pops string, pushes int (byte length)
    #[brw(magic = 38u8)]
    StringLenBytes,
    
    // Dereference a ref<T> and push its referenced value.
    // Expects: [..., Ref] -> pops Ref, pushes the referenced value (copy for copy-treated types)
    #[brw(magic = 39u8)]
    Deref,
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
    pub string_constants: Vec<String>,
}

impl Module {
    pub fn get_function(&self, name: &str) -> Option<&Symbol> {
        self.symbols
            .iter()
            .find(|sym| sym.name == name && matches!(sym.kind, SymbolKind::Function { .. }))
    }
}
