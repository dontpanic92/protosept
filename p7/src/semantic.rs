use std::{collections::HashMap, fmt::Debug};

#[derive(Debug, Clone)]
pub enum Constant {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
}

#[derive(Debug, Clone)]
pub enum SymbolKind {
    Constant(Constant),
    Function { type_id: TypeId, address: u32 },
    Enum(TypeId),
    Struct(TypeId),
    Module,
}

pub type SymbolId = u32;

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

    pub fn get_type_id(&self) -> Option<TypeId> {
        match &self.kind {
            SymbolKind::Function { type_id, .. } => Some(*type_id),
            SymbolKind::Enum(type_id) => Some(*type_id),
            SymbolKind::Struct(type_id) => Some(*type_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Function {
    pub qualified_name: String,
    pub args: Vec<Type>,
    pub arg_names: Vec<String>,
    pub return_type: Type,
}

#[derive(Debug, Clone)]
pub struct Enum {
    pub qualified_name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Struct {
    pub qualified_name: String,
    pub fields: Vec<(String, Type)>,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum PrimitiveType {
    Int,
    Float,
    Bool,
    Char,
    String,
    Unit,
}

pub type TypeId = u32;

#[derive(Debug)]
pub enum Type {
    Primitive(PrimitiveType),
    Reference(Box<Type>),
    Array(Box<Type>),
    Function(TypeId),
    Enum(TypeId),
    Struct(TypeId),
}

impl Clone for Type {
    fn clone(&self) -> Self {
        match self {
            Type::Primitive(primitive_type) => Type::Primitive(*primitive_type),
            Type::Reference(r) => Type::Reference(r.clone()),
            Type::Array(a) => Type::Array(a.clone()),
            Type::Function(f) => Type::Function(*f),
            Type::Enum(e) => Type::Enum(*e),
            Type::Struct(s) => Type::Struct(*s),
        }
    }
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Type::Primitive(a), Type::Primitive(b)) => a == b,
            (Type::Reference(a), Type::Reference(b)) => *a == *b,
            (Type::Array(a), Type::Array(b)) => *a == *b,
            (Type::Function(a), Type::Function(b)) => *a == *b,
            (Type::Enum(a), Type::Enum(b)) => *a == *b,
            (Type::Struct(a), Type::Struct(b)) => *a == *b,
            _ => false,
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
            Type::Reference(r) => format!("&{}", r.to_string()),
            Type::Array(a) => format!("[{}]", a.to_string()),
            Type::Function(f) => format!("function({})", f.to_string()),
            Type::Enum(e) => format!("enum({})", e.to_string()),
            Type::Struct(s) => format!("struct({})", s.to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum UserDefinedType {
    Function(Function),
    Enum(Enum),
    Struct(Struct),
}

pub struct SymbolTable {
    pub symbols: Vec<Symbol>,
    pub types: Vec<UserDefinedType>,

    pub symbol_chain: Vec<SymbolId>,
}

impl SymbolTable {
    pub fn new() -> Self {
        let root = Symbol::new("$root".to_string(), "$root".to_string(), SymbolKind::Module);

        SymbolTable {
            symbols: vec![root],
            types: Vec::new(),
            symbol_chain: vec![0],
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
        // Special handling: inside a struct's scope, `Self` refers to the enclosing struct type.
        if name == "Self" {
            if let Some(current) = self.get_current_symbol() {
                match current.kind {
                    SymbolKind::Struct(id) => return Some(Type::Struct(id)),
                    _ => {}
                }
            }
        }

        let primitive_type = Self::to_primitive_type(name);
        if primitive_type.is_some() {
            return primitive_type;
        }

        let symbol_id = self.find_symbol_in_scope(name)?;
        let symbol = self.get_symbol(symbol_id)?;
        match symbol.kind {
            SymbolKind::Enum(id) => Some(Type::Enum(id)),
            SymbolKind::Struct(id) => Some(Type::Struct(id)),
            SymbolKind::Function { type_id, .. } => Some(Type::Function(type_id)),
            _ => None,
        }
    }

    pub fn pop_symbol(&mut self) {
        self.symbol_chain.pop();
    }

    pub fn get_new_symbol_qualified_name(&self, name: String) -> String {
        self.get_current_symbol()
            .and_then(|symbol| Some(symbol.qualified_name.clone()))
            .unwrap_or_default()
            + "."
            + &name
    }

    pub fn add_udt(&mut self, udt: UserDefinedType) -> TypeId {
        let type_id = self.types.len() as TypeId;
        self.types.push(udt);
        type_id
    }

    pub fn get_udt(&self, type_id: TypeId) -> &UserDefinedType {
        &self.types[type_id as usize]
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
}

#[derive(Debug)]
pub struct LexicalScope {
    symbols: Vec<Symbol>,
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

    pub fn add_variable(&mut self, name: String, var_type: Type) -> LocalSymbolScopeResult<u32> {
        if self.scopes.is_empty() {
            Err(LocalSymbolScopeError::NoScopePushed)
        } else {
            let var_id = self.locals.len() as u32;
            self.locals.push(Variable { name, ty: var_type });
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

    pub fn push_scope(&mut self) {
        let scope = LexicalScope {
            symbols: Vec::new(),
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
