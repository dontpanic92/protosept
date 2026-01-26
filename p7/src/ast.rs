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
                format!("ref {}", r.get_name())
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
    pub name: Identifier,
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
pub struct EnumValue {
    pub name: String,
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
        else_block: Option<Box<Expression>>,
    },

    Ref(Identifier),

    BlockValue(Box<Expression>),
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

#[derive(Debug, PartialEq, Clone)]
pub struct NamedPattern {
    pub name: Option<Identifier>,
    pub pattern: Pattern,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Statement {
    Let {
        identifier: Identifier,
        expression: Expression,
    },
    Expression(Expression),
    FunctionDeclaration(FunctionDeclaration),
    Throw(Expression),
    EnumDeclaration {
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
        type_parameters: Vec<TypeParameter>,
        values: Vec<EnumValue>,
    },
    StructDeclaration {
        is_pub: bool,
        name: Identifier,
        attributes: Vec<Attribute>,
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
    Branch {
        named_pattern: NamedPattern,
        expression: Expression,
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
            Expression::Ref(identifier) => format!("ref {}", identifier.name),
            _ => "".to_string(),
        }
    }

    pub fn get_pos(&self) -> (usize, usize) {
        match self {
            Expression::Identifier(identifier) => (identifier.line, identifier.col),
            Expression::FunctionCall(function_call) => function_call.callee.get_pos(),
            Expression::FieldAccess { object: _, field } => (field.line, field.col),
            Expression::Ref(identifier) => (identifier.line, identifier.col),
            _ => (0, 0),
        }
    }
}
