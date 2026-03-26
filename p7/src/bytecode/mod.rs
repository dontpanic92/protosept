pub mod builder;
pub mod codegen;

use binrw::{BinRead, binrw};

use crate::semantic::{Symbol, SymbolKind};

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub enum Instruction {
    #[brw(magic = 0u8)]
    Ldi(i64),

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

    // Call a host function by name.
    // Expects: [..., args] -> pops args, calls host function, pushes result
    // Parameters: string_index (index into Module.string_constants for function name)
    #[brw(magic = 38u8)]
    InvokeHost(u32),

    // Call an external function from another module.
    // Expects: [..., args] -> pops args, calls external function, pushes result
    // Parameters: (module_path_idx, symbol_name_idx) where both are indices into Module.string_constants
    #[brw(magic = 39u8)]
    CallExternal(u32, u32),

    // Load null value onto the stack.
    // Expects: [...] -> [..., null]
    #[brw(magic = 40u8)]
    Ldnull,

    // Wrap a value into a nullable Some variant.
    // Expects: [..., value] -> [..., Some(value)]
    #[brw(magic = 41u8)]
    WrapNullable,

    // Check if nullable value is null.
    // Expects: [..., nullable] -> [..., bool] (1 if null, 0 if Some)
    #[brw(magic = 42u8)]
    IsNull,

    // Force unwrap: get inner value or trap.
    // Expects: [..., nullable] -> [..., value] (or trap if null)
    #[brw(magic = 43u8)]
    ForceUnwrap,

    // Null-coalescing: return value if Some, default if null.
    // Expects: [..., nullable, default] -> [..., value or default]
    #[brw(magic = 44u8)]
    NullCoalesce,

    /// Create a closure value from a function address and captured values.
    /// Pops capture_count values from the stack, creates Closure with func_addr.
    #[brw(magic = 45u8)]
    MakeClosure(u32, u32), // (func_addr, capture_count)

    /// Call a closure value. Pops arg_count arguments, then the closure value.
    #[brw(magic = 46u8)]
    CallClosure(u32), // arg_count

    #[brw(magic = 47u8)]
    BitAnd,

    #[brw(magic = 48u8)]
    BitOr,

    #[brw(magic = 49u8)]
    BitXor,

    /// Load a module-level variable onto the stack.
    /// Parameters: var_id (index into module-level variable storage)
    #[brw(magic = 50u8)]
    LdModVar(u32),

    /// Store top of stack into a module-level variable.
    /// Parameters: var_id (index into module-level variable storage)
    #[brw(magic = 51u8)]
    StModVar(u32),

    /// Load a module-level variable from an imported module onto the stack.
    /// Parameters: (module_path_string_id, var_name_string_id)
    #[brw(magic = 52u8)]
    LdExtModVar(u32, u32),

    /// Store top of stack into a mutable module-level variable in an imported module.
    /// Parameters: (module_path_string_id, var_name_string_id)
    #[brw(magic = 53u8)]
    StExtModVar(u32, u32),
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Module {
    pub instructions: Vec<u8>,
    pub symbols: Vec<Symbol>,
    pub functions: Vec<crate::semantic::Function>,
    pub types: Vec<crate::semantic::TypeDefinition>,
    pub string_constants: Vec<String>,
    pub imported_modules: std::collections::HashMap<String, Box<Module>>,
    /// Number of module-level variables (thread-local bindings)
    pub module_var_count: u32,
    /// Bytecode address where module-level init code begins (None if no init code)
    pub module_init_address: Option<u32>,
    /// Exported module-level variable metadata (for cross-module access)
    pub module_variables: Vec<codegen::ModuleVariable>,
}

impl Module {
    pub fn get_function(&self, name: &str) -> Option<&Symbol> {
        self.symbols
            .iter()
            .find(|sym| sym.name == name && matches!(sym.kind, SymbolKind::Function { .. }))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("Module serialization failed")
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        bincode::deserialize(bytes).map_err(|e| format!("Module deserialization failed: {e}"))
    }
}
