use std::{collections::HashMap, fmt::Debug};

#[derive(Debug, Clone)]
pub enum Constant {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
}

/// Unique identifier for functions in the symbol table
pub type FunctionId = u32;

/// Unique identifier for types (Struct, Enum, Proto) in the symbol table
pub type TypeId = u32;

/// Unique identifier for symbols in the symbol table
pub type SymbolId = u32;

/// Unique identifier for modules in the symbol table
pub type ModuleId = u32;

#[derive(Debug, Clone)]
pub enum SymbolKind {
    Constant(Constant),
    Function { func_id: FunctionId, address: u32 },
    Type(TypeId), // Unified for Struct, Enum, Proto
    Module(ModuleId),
}

impl SymbolKind {
    pub fn discriminant(&self) -> std::mem::Discriminant<SymbolKind> {
        std::mem::discriminant(self)
    }

    pub fn discriminant_of_function() -> std::mem::Discriminant<SymbolKind> {
        std::mem::discriminant(&SymbolKind::Function {
            func_id: 0,
            address: 0,
        })
    }

    pub fn discriminant_of_type() -> std::mem::Discriminant<SymbolKind> {
        std::mem::discriminant(&SymbolKind::Type(0))
    }

    // Deprecated: use discriminant_of_type() instead
    // Kept for backward compatibility during refactoring
    pub fn discriminant_of_struct() -> std::mem::Discriminant<SymbolKind> {
        std::mem::discriminant(&SymbolKind::Type(0))
    }

    pub fn discriminant_of_enum() -> std::mem::Discriminant<SymbolKind> {
        std::mem::discriminant(&SymbolKind::Type(0))
    }

    pub fn discriminant_of_proto() -> std::mem::Discriminant<SymbolKind> {
        std::mem::discriminant(&SymbolKind::Type(0))
    }
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub children: HashMap<String, SymbolId>,
}

impl Symbol {
    pub fn new(name: String, qualified_name: String, kind: SymbolKind) -> Self {
        Symbol {
            name,
            qualified_name,
            kind,
            children: HashMap::new(),
        }
    }

    pub fn get_function_address(&self) -> Option<u32> {
        match &self.kind {
            SymbolKind::Function { address, .. } => Some(*address),
            _ => None,
        }
    }

    pub fn get_func_id(&self) -> Option<FunctionId> {
        match &self.kind {
            SymbolKind::Function { func_id, .. } => Some(*func_id),
            _ => None,
        }
    }

    pub fn get_type_id(&self) -> Option<TypeId> {
        match &self.kind {
            SymbolKind::Type(type_id) => Some(*type_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Function {
    pub qualified_name: String,
    pub params: Vec<Type>,
    pub param_names: Vec<String>,
    pub param_defaults: Vec<Option<crate::ast::Expression>>,
    pub return_type: Type,
    pub attributes: Vec<crate::ast::Attribute>,
    // Intrinsic name if this is an intrinsic function (e.g., "box_new" for box() constructor)
    // Extracted from @intrinsic("name") attribute for special compiler-recognized functions
    pub intrinsic_name: Option<String>,
    // For generic functions: stores the original type parameter names
    pub type_parameters: Vec<String>,
    // For generic functions: stores the original parsed parameter types (before substitution)
    pub generic_param_types: Option<Vec<crate::ast::Type>>,
    // For generic functions: stores the original parsed return type (before substitution)
    pub generic_return_type: Option<crate::ast::Type>,
    // For generic functions: stores the function body AST for monomorphization
    pub generic_body: Option<Vec<crate::ast::Statement>>,
    // For monomorphized functions: stores the base generic function's FunctionId and concrete type arguments
    pub monomorphization: Option<(FunctionId, Vec<Type>)>,
}

#[derive(Debug, Clone)]
pub struct Enum {
    pub qualified_name: String,
    pub variants: Vec<(String, Vec<Type>)>, // (variant_name, field_types)
    pub attributes: Vec<crate::ast::Attribute>,
    // For generic enums: stores the original type parameter names
    pub type_parameters: Vec<String>,
    // For generic enums: stores the original parsed variant field types (before substitution)
    pub generic_variant_types: Option<Vec<Vec<crate::ast::Type>>>,
    // For monomorphized enums: stores the base generic enum's TypeId and concrete type arguments
    pub monomorphization: Option<(TypeId, Vec<Type>)>,
    // Protocol conformances
    pub conforming_to: Vec<TypeId>,
    // Associated methods
    pub methods: Vec<FunctionId>,
}

#[derive(Debug, Clone)]
pub struct Struct {
    pub qualified_name: String,
    pub fields: Vec<(String, Type)>,
    pub field_defaults: Vec<Option<crate::ast::Expression>>,
    pub attributes: Vec<crate::ast::Attribute>,
    // For generic structs: stores the original type parameter names
    pub type_parameters: Vec<String>,
    // For generic structs: stores the original parsed field types (before substitution)
    pub generic_field_types: Option<Vec<crate::ast::Type>>,
    // For monomorphized structs: stores the base generic struct's TypeId and concrete type arguments
    pub monomorphization: Option<(TypeId, Vec<Type>)>,
    // Protocol conformances
    pub conforming_to: Vec<TypeId>,
    // Associated methods
    pub methods: Vec<FunctionId>,
}

#[derive(Debug, Clone)]
pub struct Proto {
    pub qualified_name: String,
    pub methods: Vec<(String, Vec<Type>, Option<Type>)>, // (name, params, return_type)
    pub attributes: Vec<crate::ast::Attribute>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    Int,
    Float,
    Bool,
    Char,
    String,
    Unit,
}

#[derive(Debug)]
pub enum Type {
    Primitive(PrimitiveType),
    Reference(Box<Type>),
    Array(Box<Type>),
    BoxType(Box<Type>),
    Enum(TypeId),
    Struct(TypeId),
    Proto(TypeId),
    Nullable(Box<Type>),
    /// Function type: fn(T1, T2) -> R
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },
}

impl Type {
    pub fn is_struct(&self) -> bool {
        matches!(self, Type::Struct(_))
    }

    /// Check if this type is "copy-treated" according to the spec (§6.3):
    /// - Primitives (int, float, bool, char, string, unit) are copy-treated
    /// - ref<T> is copy-treated (view/handle copy)
    /// - box<T> is copy-treated (handle copy)
    /// - User-defined structs/enums are copy-treated ONLY if they conform to Copy proto
    pub fn is_copy_treated(&self, symbol_table: &SymbolTable) -> bool {
        match self {
            // All primitives are copy-treated by default
            Type::Primitive(_) => true,
            // ref<T> and box<T> are copy-treated (handle/view copy)
            Type::Reference(_) | Type::BoxType(_) => true,
            // ?T is copy-treated iff T is copy-treated
            Type::Nullable(inner) => inner.is_copy_treated(symbol_table),
            // User-defined structs: check for Copy proto conformance
            Type::Struct(type_id) => {
                if let TypeDefinition::Struct(s) = symbol_table.get_type(*type_id) {
                    // Check if struct conforms to Copy proto
                    s.conforming_to.iter().any(|proto_id| {
                        if let Some(TypeDefinition::Proto(proto)) =
                            symbol_table.types.get(*proto_id as usize)
                        {
                            proto.qualified_name.ends_with(".Copy")
                                || proto.qualified_name == "Copy"
                        } else {
                            false
                        }
                    })
                } else {
                    false
                }
            }
            // User-defined enums: check for Copy proto conformance
            Type::Enum(type_id) => {
                if let TypeDefinition::Enum(e) = symbol_table.get_type(*type_id) {
                    e.conforming_to.iter().any(|proto_id| {
                        if let Some(TypeDefinition::Proto(proto)) =
                            symbol_table.types.get(*proto_id as usize)
                        {
                            proto.qualified_name.ends_with(".Copy")
                                || proto.qualified_name == "Copy"
                        } else {
                            false
                        }
                    })
                } else {
                    false
                }
            }
            // Arrays and Protos: not copy-treated by default in v1
            Type::Array(_) | Type::Proto(_) => false,
            // Function types (closures): copy-treated only if all captures are Copy
            // For now (non-capturing closures), always copy-treated
            Type::Function { .. } => true,
        }
    }
}

impl Clone for Type {
    fn clone(&self) -> Self {
        match self {
            Type::Primitive(primitive_type) => Type::Primitive(*primitive_type),
            Type::Reference(r) => Type::Reference(r.clone()),
            Type::Array(a) => Type::Array(a.clone()),
            Type::BoxType(b) => Type::BoxType(b.clone()),
            Type::Enum(e) => Type::Enum(*e),
            Type::Struct(s) => Type::Struct(*s),
            Type::Proto(p) => Type::Proto(*p),
            Type::Nullable(n) => Type::Nullable(n.clone()),
            Type::Function { params, return_type } => Type::Function {
                params: params.clone(),
                return_type: return_type.clone(),
            },
        }
    }
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Type::Primitive(a), Type::Primitive(b)) => a == b,
            (Type::Reference(a), Type::Reference(b)) => *a == *b,
            (Type::Array(a), Type::Array(b)) => *a == *b,
            (Type::BoxType(a), Type::BoxType(b)) => *a == *b,
            (Type::Enum(a), Type::Enum(b)) => *a == *b,
            (Type::Struct(a), Type::Struct(b)) => *a == *b,
            (Type::Proto(a), Type::Proto(b)) => *a == *b,
            (Type::Nullable(a), Type::Nullable(b)) => *a == *b,
            (
                Type::Function { params: pa, return_type: ra },
                Type::Function { params: pb, return_type: rb },
            ) => pa == pb && ra == rb,
            _ => false,
        }
    }
}

impl Eq for Type {}

impl std::hash::Hash for Type {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Type::Primitive(p) => p.hash(state),
            Type::Reference(r) => r.hash(state),
            Type::Array(a) => a.hash(state),
            Type::BoxType(b) => b.hash(state),
            Type::Enum(e) => e.hash(state),
            Type::Struct(s) => s.hash(state),
            Type::Proto(p) => p.hash(state),
            Type::Nullable(n) => n.hash(state),
            Type::Function { params, return_type } => {
                params.hash(state);
                return_type.hash(state);
            }
        }
    }
}

impl ToString for Type {
    fn to_string(&self) -> String {
        match self {
            Type::Primitive(primitive_type) => match primitive_type {
                PrimitiveType::Int => "int".to_string(),
                PrimitiveType::Float => "float".to_string(),
                PrimitiveType::Bool => "bool".to_string(),
                PrimitiveType::Char => "char".to_string(),
                PrimitiveType::String => "string".to_string(),
                PrimitiveType::Unit => "unit".to_string(),
            },
            Type::Reference(r) => format!("ref<{}>", r.to_string()),
            Type::Array(a) => format!("[{}]", a.to_string()),
            Type::BoxType(b) => format!("box<{}>", b.to_string()),
            Type::Enum(e) => format!("enum({})", e.to_string()),
            Type::Struct(s) => format!("struct({})", s.to_string()),
            Type::Proto(p) => format!("proto({})", p.to_string()),
            Type::Nullable(n) => format!("?{}", n.to_string()),
            Type::Function { params, return_type } => {
                let param_strs: Vec<String> = params.iter().map(|p| p.to_string()).collect();
                format!("fn({}) -> {}", param_strs.join(", "), return_type.to_string())
            }
        }
    }
}

/// Type definitions (structs, enums, protocols) - does NOT include functions
#[derive(Debug, Clone)]
pub enum TypeDefinition {
    Enum(Enum),
    Struct(Struct),
    Proto(Proto),
}

pub struct ModuleInfo {
    pub path: String,
    pub root_symbol_id: SymbolId,
}

pub struct SymbolTable {
    pub symbols: Vec<Symbol>,
    pub functions: Vec<Function>,
    pub types: Vec<TypeDefinition>,

    pub modules: Vec<ModuleInfo>,
    pub module_path_to_id: HashMap<String, ModuleId>,

    pub symbol_chain: Vec<SymbolId>,

    // Cache for monomorphized types: (base_type_id, type_args) -> monomorphized_type_id
    pub monomorphization_cache: HashMap<(TypeId, Vec<Type>), TypeId>,
    // Cache for monomorphized functions: (base_func_id, type_args) -> monomorphized_func_id
    pub function_monomorphization_cache: HashMap<(FunctionId, Vec<Type>), FunctionId>,
}

impl SymbolTable {
    /// Finds the nearest symbol in the current chain that matches the given SymbolKind discriminant.
    pub fn find_nearest_symbol_id_by_kind(
        &self,
        kind: std::mem::Discriminant<SymbolKind>,
    ) -> Option<&SymbolId> {
        for symbol_id in self.symbol_chain.iter().rev() {
            let symbol = &self.symbols[*symbol_id as usize];
            if symbol.kind.discriminant() == kind {
                return Some(symbol_id);
            }
        }
        None
    }

    pub fn new() -> Self {
        let root = Symbol::new(
            "$root".to_string(),
            "$root".to_string(),
            SymbolKind::Module(0),
        );

        let mut module_path_to_id = HashMap::new();
        module_path_to_id.insert("$root".to_string(), 0);

        SymbolTable {
            symbols: vec![root],
            functions: Vec::new(),
            types: Vec::new(),
            modules: vec![ModuleInfo {
                path: "$root".to_string(),
                root_symbol_id: 0,
            }],
            module_path_to_id,
            symbol_chain: vec![0],
            monomorphization_cache: HashMap::new(),
            function_monomorphization_cache: HashMap::new(),
        }
    }

    pub fn push_symbol(&mut self, symbol: Symbol) {
        let current_id = *self.symbol_chain.last().unwrap();
        let symbol_id = self.symbols.len() as SymbolId;
        let symbol_name = symbol.name.clone();
        self.symbols.push(symbol);

        self.symbols[current_id as usize]
            .children
            .insert(symbol_name, symbol_id);
        self.symbol_chain.push(symbol_id);
    }

    /// Insert a symbol into the current scope without pushing it onto the symbol_chain
    pub fn insert_symbol(&mut self, symbol: Symbol) -> SymbolId {
        let current_id = *self.symbol_chain.last().unwrap();
        let symbol_id = self.symbols.len() as SymbolId;
        let symbol_name = symbol.name.clone();
        self.symbols.push(symbol);

        self.symbols[current_id as usize]
            .children
            .insert(symbol_name, symbol_id);
        symbol_id
    }

    pub fn find_symbol_in_scope(&self, name: &str) -> Option<SymbolId> {
        for symbol_id in self.symbol_chain.iter().rev() {
            let symbol = &self.symbols[*symbol_id as usize];
            if symbol.name == name {
                return Some(*symbol_id);
            }

            if let Some(child_id) = symbol.children.get(name) {
                return Some(*child_id);
            }
        }

        None
    }

    pub fn get_symbol(&self, symbol_id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(symbol_id as usize)
    }

    /// Find the symbol representing a type_id within this symbol table, matching qualified name to avoid collisions.
    pub fn find_symbol_for_type(&self, type_id: TypeId) -> Option<SymbolId> {
        let type_def = self.types.get(type_id as usize)?;
        let target_qualified_name = match type_def {
            TypeDefinition::Struct(s) => &s.qualified_name,
            TypeDefinition::Enum(e) => &e.qualified_name,
            TypeDefinition::Proto(p) => &p.qualified_name,
        };
        self.symbols.iter().enumerate().find_map(|(i, s)| {
            if let SymbolKind::Type(tid) = s.kind {
                if tid == type_id && &s.qualified_name == target_qualified_name {
                    return Some(i as SymbolId);
                }
            }
            None
        })
    }

    pub fn to_primitive_type(name: &str) -> Option<Type> {
        match name {
            "int" => Some(Type::Primitive(PrimitiveType::Int)),
            "float" => Some(Type::Primitive(PrimitiveType::Float)),
            "bool" => Some(Type::Primitive(PrimitiveType::Bool)),
            "string" => Some(Type::Primitive(PrimitiveType::String)),
            "unit" => Some(Type::Primitive(PrimitiveType::Unit)),
            _ => None,
        }
    }

    pub fn find_type_in_scope(&self, name: &str) -> Option<Type> {
        // Special handling: inside a struct's scope, `Self` refers to the enclosing type.
        if name == "Self" {
            if let Some(enclose_type) =
                self.find_nearest_symbol_id_by_kind(SymbolKind::discriminant_of_type())
            {
                let type_symbol = self.get_symbol(*enclose_type).unwrap();
                if let SymbolKind::Type(id) = type_symbol.kind {
                    if let Some(type_def) = self.types.get(id as usize) {
                        // Determine the actual type kind
                        return match type_def {
                            TypeDefinition::Struct(_) => Some(Type::Struct(id)),
                            TypeDefinition::Enum(_) => Some(Type::Enum(id)),
                            TypeDefinition::Proto(_) => Some(Type::Proto(id)),
                        };
                    }
                }
            }
        }

        let primitive_type = Self::to_primitive_type(name);
        if primitive_type.is_some() {
            return primitive_type;
        }

        let symbol_id = self.find_symbol_in_scope(name)?;
        let symbol = self.get_symbol(symbol_id)?;
        match &symbol.kind {
            SymbolKind::Type(id) => {
                if let Some(type_def) = self.types.get(*id as usize) {
                    // Determine the actual type kind
                    match type_def {
                        TypeDefinition::Struct(_) => Some(Type::Struct(*id)),
                        TypeDefinition::Enum(_) => Some(Type::Enum(*id)),
                        TypeDefinition::Proto(_) => Some(Type::Proto(*id)),
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn find_module_id(&self, path: &str) -> Option<ModuleId> {
        self.module_path_to_id.get(path).cloned()
    }

    pub fn get_module(&self, module_id: ModuleId) -> Option<&ModuleInfo> {
        self.modules.get(module_id as usize)
    }

    pub fn register_module(&mut self, path: String, root_symbol_id: SymbolId) -> ModuleId {
        if let Some(id) = self.module_path_to_id.get(&path) {
            return *id;
        }
        let id = self.modules.len() as ModuleId;
        self.modules.push(ModuleInfo {
            path: path.clone(),
            root_symbol_id,
        });
        self.module_path_to_id.insert(path, id);
        id
    }

    pub fn pop_symbol(&mut self) {
        self.symbol_chain.pop();
    }

    /// Push an existing symbol (by ID) onto the symbol chain for scoping.
    /// Used in two-pass compilation: pass 1 registers the symbol, pass 2 pushes
    /// it back onto the chain to generate the body.
    pub fn push_existing_symbol(&mut self, symbol_id: SymbolId) {
        self.symbol_chain.push(symbol_id);
    }

    /// Find a symbol by name in the current scope chain, returning its ID.
    pub fn find_symbol(&self, name: &str) -> Option<SymbolId> {
        self.find_symbol_in_scope(name)
    }

    pub fn find_symbol_by_qualified_name(&self, qualified_name: &str) -> Option<&Symbol> {
        self.symbols
            .iter()
            .find(|s| s.qualified_name == qualified_name)
    }

    pub fn get_new_symbol_qualified_name(&self, name: String) -> String {
        self.get_current_symbol()
            .and_then(|symbol| Some(symbol.qualified_name.clone()))
            .unwrap_or_default()
            + "."
            + &name
    }

    // Function management
    pub fn add_function(&mut self, func: Function) -> FunctionId {
        let func_id = self.functions.len() as FunctionId;
        self.functions.push(func);
        func_id
    }

    pub fn get_function(&self, func_id: FunctionId) -> &Function {
        &self.functions[func_id as usize]
    }

    pub fn get_function_mut(&mut self, func_id: FunctionId) -> &mut Function {
        &mut self.functions[func_id as usize]
    }

    // Type management
    pub fn add_type(&mut self, ty: TypeDefinition) -> TypeId {
        let type_id = self.types.len() as TypeId;
        self.types.push(ty);
        type_id
    }

    pub fn get_type(&self, type_id: TypeId) -> &TypeDefinition {
        &self.types[type_id as usize]
    }

    pub fn get_type_checked(&self, type_id: TypeId) -> Option<&TypeDefinition> {
        self.types.get(type_id as usize)
    }

    pub fn get_type_mut(&mut self, type_id: TypeId) -> &mut TypeDefinition {
        &mut self.types[type_id as usize]
    }

    // Backward compatibility methods
    #[deprecated(note = "Use add_type() or add_function() instead")]
    pub fn add_udt(&mut self, udt: TypeDefinition) -> TypeId {
        self.add_type(udt)
    }

    #[deprecated(note = "Use get_type() instead")]
    pub fn get_udt(&self, type_id: TypeId) -> &TypeDefinition {
        self.get_type(type_id)
    }

    pub fn get_current_symbol(&self) -> Option<&Symbol> {
        self.symbol_chain
            .last()
            .and_then(|id| self.symbols.get(*id as usize))
    }

    pub fn get_current_symbol_mut(&mut self) -> Option<&mut Symbol> {
        self.symbol_chain
            .last()
            .and_then(|id| self.symbols.get_mut(*id as usize))
    }

    /// Add a method to a struct's method list
    pub fn add_method_to_struct(&mut self, struct_type_id: TypeId, func_id: FunctionId) {
        if let TypeDefinition::Struct(s) = &mut self.types[struct_type_id as usize] {
            s.methods.push(func_id);
        }
    }

    /// Add a method to an enum's method list
    pub fn add_method_to_enum(&mut self, enum_type_id: TypeId, func_id: FunctionId) {
        if let TypeDefinition::Enum(e) = &mut self.types[enum_type_id as usize] {
            e.methods.push(func_id);
        }
    }
}

#[derive(Debug)]
pub enum LocalSymbolScopeError {
    NoScopePushed,
}

pub type LocalSymbolScopeResult<T> = Result<T, LocalSymbolScopeError>;

#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    pub ty: Type,
    pub is_mutable: bool,
}

#[derive(Debug)]
pub struct LexicalScope {
    var_ids: Vec<u32>,
}

#[derive(Debug)]
pub struct LocalSymbolScope {
    pub scopes: Vec<LexicalScope>,
    pub locals: Vec<Variable>,
    pub params: Vec<Variable>,
}

impl LocalSymbolScope {
    pub fn new(params: Vec<Variable>) -> Self {
        LocalSymbolScope {
            scopes: Vec::new(),
            locals: Vec::new(),
            params,
        }
    }

    pub fn set_params(&mut self, params: Vec<Variable>) {
        self.params = params;
    }

    pub fn add_variable(
        &mut self,
        name: String,
        var_type: Type,
        is_mutable: bool,
    ) -> LocalSymbolScopeResult<u32> {
        if self.scopes.is_empty() {
            Err(LocalSymbolScopeError::NoScopePushed)
        } else {
            let var_id = self.locals.len() as u32;
            self.locals.push(Variable {
                name,
                ty: var_type,
                is_mutable,
            });
            self.scopes.last_mut().unwrap().var_ids.push(var_id);
            Ok(var_id)
        }
    }

    pub fn find_variable(&self, name: &str) -> Option<u32> {
        for scope in self.scopes.iter().rev() {
            for var in scope.var_ids.iter() {
                if self.locals[*var as usize].name == name {
                    return Some(*var);
                }
            }
        }

        None
    }

    pub fn find_param(&self, name: &str) -> Option<u32> {
        for (id, param) in self.params.iter().enumerate() {
            if param.name == name {
                return Some(id as u32);
            }
        }

        None
    }

    pub fn get_variable_type(&self, var_id: u32) -> Type {
        self.locals[var_id as usize].ty.clone()
    }

    pub fn get_param_type(&self, param_id: u32) -> Type {
        self.params[param_id as usize].ty.clone()
    }

    pub fn is_variable_mutable(&self, var_id: u32) -> bool {
        self.locals[var_id as usize].is_mutable
    }

    pub fn push_scope(&mut self) {
        let scope = LexicalScope {
            var_ids: Vec::new(),
        };
        self.scopes.push(scope);
    }

    pub fn pop_scope(&mut self) {
        if !self.scopes.is_empty() {
            self.scopes.pop();
        } else {
            panic!("No scope to pop");
        }
    }
}
