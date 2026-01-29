use crate::lexer::Token;

#[derive(Debug, PartialEq, Clone)]
pub struct Identifier {
    pub name: String,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, PartialEq, Clone)]
pub struct TypeParameter {
    pub name: Identifier,
    pub bound: Option<Identifier>, // e.g., T: Printable
}

#[derive(Debug, PartialEq, Clone)]
pub enum Type {
    Identifier(Identifier),
    Reference(Box<Type>),
    Array(Box<Type>),
    Generic {
        base: Identifier,
        type_args: Vec<Type>,
    },
}

impl Type {
    pub fn get_name(&self) -> String {
        match self {
            Type::Identifier(identifier) => identifier.name.clone(),
            Type::Reference(r) => {
                format!("ref<{}>", r.get_name())
            }
            Type::Array(a) => {
                format!("{}[]", a.get_name())
            }
            Type::Generic { base, type_args } => {
                let args = type_args
                    .iter()
                    .map(|t| t.get_name())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}<{}>", base.name, args)
            }
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Attribute {
    pub name: Identifier,
    pub arguments: Vec<(Option<Identifier>, Expression)>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct Parameter {
    pub name: Identifier,
    pub arg_type: Type,
    pub default_value: Option<Expression>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct FunctionCall {
    pub callee: Box<Expression>,
    pub arguments: Vec<(Option<Identifier>, Expression)>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct FunctionDeclaration {
    pub is_pub: bool,
    pub name: Identifier,
    pub attributes: Vec<Attribute>,
    pub effects: Vec<Identifier>,
    pub type_parameters: Vec<TypeParameter>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<Type>,
    pub body: StatementBlock,
}

#[derive(Debug, PartialEq, Clone)]
pub struct StructField {
    pub is_pub: bool,
    pub name: Option<Identifier>, // None for tuple struct fields
    pub field_type: Type,
    pub default_value: Option<Expression>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct StructMethod {
    pub is_pub: bool,
    pub function: FunctionDeclaration,
}

#[derive(Debug, PartialEq, Clone)]
pub struct ProtoMethod {
    pub name: Identifier,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<Type>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<Type>, // Empty for unit variants, contains types for payload variants
}

pub type StatementBlock = Vec<Statement>;

#[derive(Debug, PartialEq, Clone)]
pub enum Expression {
    Identifier(Identifier),
    IntegerLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(String),
    BooleanLiteral(bool),
    Unary {
        operator: Token,
        right: Box<Expression>,
    },
    Binary {
        left: Box<Expression>,
        operator: Token,
        right: Box<Expression>,
    },
    If {
        condition: Box<Expression>,
        then_branch: Box<Expression>,
        else_branch: Option<Box<Expression>>,
        pos: (usize, usize),
    },
    FunctionCall(FunctionCall),
    FieldAccess {
        object: Box<Expression>,
        field: Identifier,
    },
    Block(StatementBlock),
    Try {
        try_block: Box<Expression>,
        else_arms: Vec<MatchArm>,
    },
    Match {
        scrutinee: Box<Expression>,
        arms: Vec<MatchArm>,
    },
    // Generic type instantiation with explicit type arguments (e.g., Container<int>)
    GenericInstantiation {
        base: Identifier,
        type_args: Vec<Type>,
    },

    Ref(Box<Expression>),

    BlockValue(Box<Expression>),
    
    // Type cast expression (e.g., expr as box<Proto>)
    Cast {
        expression: Box<Expression>,
        target_type: Type,
    },
    
    // Loop expression (infinite loop)
    Loop {
        body: Box<Expression>,
        pos: (usize, usize),
    },
    
    // While expression (conditional loop)
    While {
        condition: Box<Expression>,
        body: Box<Expression>,
        pos: (usize, usize),
    },
    
    // Break expression (with optional value for future use)
    Break {
        value: Option<Box<Expression>>,
        pos: (usize, usize),
    },
    
    // Continue expression
    Continue {
        pos: (usize, usize),
    },
}

#[derive(Debug, PartialEq, Clone)]
pub struct MatchArm {
    pub pattern: NamedPattern,
    pub expression: Expression,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Pattern {
    Identifier(Identifier),
    IntegerLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(String),
    BooleanLiteral(bool),
    FieldAccess {
        object: Box<Pattern>,
        field: Identifier,
    },
}

impl Pattern {
    pub fn is_wildcard(&self) -> bool {
        matches!(self, Pattern::Identifier(id) if id.name == "_")
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct NamedPattern {
    pub name: Option<Identifier>,
    pub pattern: Pattern,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Statement {
    Let {
        identifier: Identifier,
        type_annotation: Option<Type>,
        expression: Expression,
    },
    Expression(Expression),
    FunctionDeclaration(FunctionDeclaration),
    Throw(Expression),
    EnumDeclaration {
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        conformance: Vec<Identifier>, // Protocol conformances: enum[Proto1, Proto2]
        type_parameters: Vec<TypeParameter>,
        values: Vec<EnumVariant>,
        methods: Vec<StructMethod>,
    },
    StructDeclaration {
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        conformance: Vec<Identifier>, // Protocol conformances: struct[Proto1, Proto2]
        type_parameters: Vec<TypeParameter>,
        fields: Vec<StructField>,
        methods: Vec<StructMethod>,
    },
    ProtoDeclaration {
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        methods: Vec<ProtoMethod>,
    },
    TypeDeclaration {
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        conformance: Vec<Identifier>, // Protocol conformances: type[Proto1, Proto2]
        type_parameters: Vec<TypeParameter>,
        representation: Option<Type>, // The underlying type in type Name(ReprType)
        methods: Vec<StructMethod>,
    },
    Return(Box<Expression>),
    Import {
        module_path: String,
        alias: Option<String>,
    },
}

impl Expression {
    pub fn get_name(&self) -> String {
        match self {
            Expression::Identifier(identifier) => identifier.name.clone(),
            Expression::FunctionCall(function_call) => function_call.callee.get_name(),
            Expression::FieldAccess { object, field } => {
                format!("{}.{}", object.get_name(), field.name)
            }
            Expression::Ref(expr) => format!("ref({})", expr.get_name()),
            Expression::GenericInstantiation { base, type_args } => {
                let args = type_args
                    .iter()
                    .map(|t| t.get_name())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}<{}>", base.name, args)
            }
            Expression::Cast { expression, target_type } => {
                format!("{} as {}", expression.get_name(), target_type.get_name())
            }
            Expression::Loop { .. } => "loop".to_string(),
            Expression::While { .. } => "while".to_string(),
            Expression::Break { .. } => "break".to_string(),
            Expression::Continue { .. } => "continue".to_string(),
            _ => "".to_string(),
        }
    }

    pub fn get_pos(&self) -> (usize, usize) {
        match self {
            Expression::Identifier(identifier) => (identifier.line, identifier.col),
            Expression::FunctionCall(function_call) => function_call.callee.get_pos(),
            Expression::FieldAccess { object: _, field } => (field.line, field.col),
            Expression::Ref(expr) => expr.get_pos(),
            Expression::GenericInstantiation { base, .. } => (base.line, base.col),
            Expression::Cast { expression, .. } => expression.get_pos(),
            Expression::Loop { pos, .. } => *pos,
            Expression::While { pos, .. } => *pos,
            Expression::Break { pos, .. } => *pos,
            Expression::Continue { pos, .. } => *pos,
            _ => (0, 0),
        }
    }
}
