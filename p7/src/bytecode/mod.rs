pub mod builder;
pub mod codegen;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use binrw::{BinRead, binrw};

use crate::semantic::{Symbol, SymbolKind};

#[derive(Debug, Clone)]
pub struct ResolvedCall {
    pub target_pc: usize,
    pub args_len: usize,
}

#[derive(Debug, Clone)]
pub struct ResolvedExternalCall {
    pub module_idx: usize,
    pub target_pc: usize,
    pub args_len: usize,
}

#[derive(Debug, Clone)]
pub struct ResolvedExternalVar {
    pub module_idx: usize,
    pub var_id: u32,
}

#[derive(Debug, Clone)]
pub enum ResolvedDispatch {
    Function {
        target_pc: usize,
        args_len: usize,
    },
    Host {
        dispatcher_name: String,
        method_name: String,
        type_tag: String,
        vtable_slot: u32,
        return_ty: crate::semantic::HostReturnTy,
    },
}

#[derive(Debug, Clone)]
pub struct ProtoMethodMeta {
    pub method_name: String,
    pub param_count: usize,
}

pub fn hash_method_name(name: &str) -> u32 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    hasher.finish() as u32
}

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

    /// Convert top-of-stack int value to float (spec §15.1.2, `as float`).
    /// Expects: [..., Data::Int(i)] -> [..., Data::Float(i as f64)]
    #[brw(magic = 54u8)]
    IntToFloat,
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
    /// Runtime-only decoded instruction cache. Not serialized; rebuilt before execution.
    #[serde(skip)]
    pub decoded_instructions: Vec<Instruction>,
    /// Runtime-only map from bytecode byte offsets to decoded instruction indices.
    #[serde(skip)]
    pub byte_to_instruction: HashMap<u32, usize>,
    /// Runtime-only map from decoded instruction indices back to bytecode byte offsets.
    #[serde(skip)]
    pub instruction_to_byte: Vec<u32>,
    /// Runtime-only resolved direct call targets keyed by instruction index.
    #[serde(skip)]
    pub call_targets: Vec<Option<ResolvedCall>>,
    /// Runtime-only dispatch records keyed by symbol id.
    #[serde(skip)]
    pub symbol_dispatch: Vec<Option<ResolvedDispatch>>,
    /// Runtime-only proto method metadata keyed by (proto id, method hash).
    #[serde(skip)]
    pub proto_method_metas: HashMap<(u32, u32), ProtoMethodMeta>,
    /// Runtime-only resolved external variable targets keyed by instruction index.
    #[serde(skip)]
    pub external_var_targets: Vec<Option<ResolvedExternalVar>>,
    /// Runtime-only resolved external function targets keyed by instruction index.
    #[serde(skip)]
    pub external_call_targets: Vec<Option<ResolvedExternalCall>>,
    /// Runtime-only module variable name index.
    #[serde(skip)]
    pub module_variable_ids: HashMap<String, u32>,
    /// Runtime-only shared string constants for allocation-free literal loads.
    #[serde(skip)]
    pub shared_string_constants: Vec<Rc<str>>,
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

    pub fn prepare_execution(&mut self) -> Result<(), String> {
        self.decoded_instructions.clear();
        self.byte_to_instruction.clear();
        self.instruction_to_byte.clear();
        self.call_targets.clear();
        self.symbol_dispatch.clear();
        self.proto_method_metas.clear();
        self.external_var_targets.clear();
        self.external_call_targets.clear();
        self.module_variable_ids.clear();
        self.shared_string_constants.clear();

        let mut cursor = std::io::Cursor::new(&self.instructions);
        while cursor.position() < self.instructions.len() as u64 {
            let start = cursor.position() as u32;
            let inst_index = self.decoded_instructions.len();
            let inst = Instruction::read(&mut cursor)
                .map_err(|e| format!("failed to decode instruction at byte offset {start}: {e}"))?;
            self.byte_to_instruction.insert(start, inst_index);
            self.instruction_to_byte.push(start);
            self.decoded_instructions.push(inst);
        }
        self.call_targets = vec![None; self.decoded_instructions.len()];
        self.external_var_targets = vec![None; self.decoded_instructions.len()];
        self.external_call_targets = vec![None; self.decoded_instructions.len()];
        self.rebuild_module_variable_ids();
        self.rebuild_shared_string_constants();
        self.build_execution_metadata()?;

        Ok(())
    }

    pub fn decoded_instruction(&self, inst_index: usize) -> Option<&Instruction> {
        self.decoded_instructions.get(inst_index)
    }

    pub fn decoded_len(&self) -> usize {
        self.decoded_instructions.len()
    }

    pub fn bytecode_address_to_instruction_index(&self, address: u32) -> Option<usize> {
        if address == self.instructions.len() as u32 {
            Some(self.decoded_instructions.len())
        } else {
            self.byte_to_instruction.get(&address).copied()
        }
    }

    pub fn instruction_index_to_bytecode_address(&self, inst_index: usize) -> Option<u32> {
        if inst_index == self.decoded_instructions.len() {
            Some(self.instructions.len() as u32)
        } else {
            self.instruction_to_byte.get(inst_index).copied()
        }
    }

    pub fn direct_call_target(&self, inst_index: usize) -> Option<&ResolvedCall> {
        self.call_targets.get(inst_index)?.as_ref()
    }

    pub fn symbol_dispatch(&self, symbol_id: u32) -> Option<&ResolvedDispatch> {
        self.symbol_dispatch.get(symbol_id as usize)?.as_ref()
    }

    pub fn proto_method_meta(&self, proto_id: u32, method_hash: u32) -> Option<&ProtoMethodMeta> {
        self.proto_method_metas.get(&(proto_id, method_hash))
    }

    pub fn external_var_target(&self, inst_index: usize) -> Option<&ResolvedExternalVar> {
        self.external_var_targets.get(inst_index)?.as_ref()
    }

    pub fn external_call_target(&self, inst_index: usize) -> Option<&ResolvedExternalCall> {
        self.external_call_targets.get(inst_index)?.as_ref()
    }

    pub fn module_variable_by_name(
        &self,
        name: &str,
        require_public: bool,
    ) -> Option<&codegen::ModuleVariable> {
        if let Some(var_id) = self.module_variable_ids.get(name) {
            return self
                .module_variables
                .get(*var_id as usize)
                .filter(|var| !require_public || var.is_pub);
        }

        self.module_variables
            .iter()
            .find(|var| var.name == name && (!require_public || var.is_pub))
    }

    fn rebuild_module_variable_ids(&mut self) {
        self.module_variable_ids = self
            .module_variables
            .iter()
            .map(|var| (var.name.to_string(), var.var_id))
            .collect();
    }

    fn rebuild_shared_string_constants(&mut self) {
        self.shared_string_constants = self
            .string_constants
            .iter()
            .map(|value| Rc::<str>::from(value.as_str()))
            .collect();
    }

    fn build_execution_metadata(&mut self) -> Result<(), String> {
        self.symbol_dispatch = vec![None; self.symbols.len()];
        for (symbol_id, symbol) in self.symbols.iter().enumerate() {
            self.symbol_dispatch[symbol_id] = match &symbol.kind {
                SymbolKind::Function { func_id, address } => {
                    let Some(target_pc) = self.bytecode_address_to_instruction_index(*address)
                    else {
                        continue;
                    };
                    let function = self.functions.get(*func_id as usize).ok_or_else(|| {
                        format!("function metadata missing for symbol id {symbol_id}")
                    })?;
                    Some(ResolvedDispatch::Function {
                        target_pc,
                        args_len: function.params.len(),
                    })
                }
                SymbolKind::HostMethod {
                    dispatcher_name,
                    method_name,
                    type_tag,
                    vtable_slot,
                    return_ty,
                    ..
                } => Some(ResolvedDispatch::Host {
                    dispatcher_name: dispatcher_name.to_string(),
                    method_name: method_name.to_string(),
                    type_tag: type_tag.to_string(),
                    vtable_slot: *vtable_slot,
                    return_ty: return_ty.clone(),
                }),
                _ => None,
            };
        }

        for (inst_index, inst) in self.decoded_instructions.iter().enumerate() {
            if let Instruction::Call(symbol_id) = inst
                && let Some(ResolvedDispatch::Function {
                    target_pc,
                    args_len,
                }) = self.symbol_dispatch(*symbol_id)
            {
                self.call_targets[inst_index] = Some(ResolvedCall {
                    target_pc: *target_pc,
                    args_len: *args_len,
                });
            }
        }

        for (proto_id, ty) in self.types.iter().enumerate() {
            let crate::semantic::TypeDefinition::Proto(proto) = ty else {
                continue;
            };
            // Non-generic protos populate `methods`; generic protos populate
            // `method_templates` only, since their parameter types are not
            // resolvable without per-use-site type args. Both lists carry
            // the same method names and parameter counts, so use whichever
            // is populated to seed the runtime dispatch metadata.
            if !proto.methods.is_empty() {
                for (method_name, params, _) in &proto.methods {
                    self.proto_method_metas.insert(
                        (proto_id as u32, hash_method_name(method_name)),
                        ProtoMethodMeta {
                            method_name: method_name.to_string(),
                            param_count: params.len(),
                        },
                    );
                }
            } else {
                for (method_name, params, _) in &proto.method_templates {
                    self.proto_method_metas.insert(
                        (proto_id as u32, hash_method_name(method_name)),
                        ProtoMethodMeta {
                            method_name: method_name.to_string(),
                            param_count: params.len(),
                        },
                    );
                }
            }
        }

        Ok(())
    }
}
